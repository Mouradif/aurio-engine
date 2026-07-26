use crate::audio_graph::{AudioGraph, AudioNode, MAX_BLOCK};
use crate::frame::Frame;
use crate::effect::{
    Bitcrusher, Delay, Distortion, Gain, Highpass, Lowpass, Pan, Reverb, Tremolo,
};
use crate::midi::{MidiNote, ScheduledEvent};
use crate::organ::{Organ, OrganCharacter, Percussion, VibratoChorus};
use crate::oscillator::{Oscillator, Wave};
use crate::param::{Modulator, Param};
use crate::phrase::{Phrase, PhraseEntry, PhraseHooks};
use crate::phrase_script::PhraseScript;
use crate::midi::{note_name_to_midi, parse_note_value};
use crate::rtttl::parse_rtttl;
use crate::sampler::{Sample, Sampler, Zone};
use crate::scripting::{LuaRuntime, notes_from_table};
use crate::state_graph::{StateGraph, run_timing_thread};
use crate::static_phrase::StaticPhrase;
use crate::synth::{ADSR, Synth};
use crate::transition::Transition;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use mlua::{Function, Lua, RegistryKey, Table, UserData, Value};
use ringbuf::{HeapProd, HeapRb};
use ringbuf::traits::{Consumer, Producer, Split};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub struct Song {
    bpm: f32,
    sockets: Vec<SocketEntry>,
    labels: HashMap<String, u32>,
    connections: Vec<(SocketRef, SocketRef)>,
    phrases: HashMap<String, PhraseSpec>,
    tracks: Vec<TrackSpec>,
    pending_bindings: Vec<PendingBinding>,
    #[cfg(any(feature = "clap-host", feature = "lv2-host"))]
    pending_plugin_bindings: Vec<PendingPluginBinding>,
    #[cfg(any(feature = "clap-host", feature = "lv2-host"))]
    pending_plugin_mods: Vec<PendingPluginMod>,
}

struct SocketEntry {
    id: u32,
    content: SocketContent,
}

enum SocketContent {
    Instrument {
        adsr: ADSR,
        oscillators: Vec<(Wave, ParamSpec)>,
    },
    Sampler {
        zones: Vec<SamplerZoneSpec>,
    },
    Organ(OrganSpec),
    Effect(EffectSpec),
    /// A CLAP plugin loaded from a `.clap` bundle, used as an instrument
    /// (source) or an effect (processor) depending on `is_instrument`.
    #[cfg(feature = "clap-host")]
    Plugin {
        path: String,
        plugin_id: String,
        is_instrument: bool,
        /// Initial values for named params, plus any MIDI bindings (recorded
        /// separately in `pending_plugin_bindings`).
        params: Vec<(String, f32)>,
    },
    /// An LV2 plugin loaded from a `.lv2` bundle directory, identified by its
    /// plugin URI. Like `Plugin` but for the LV2 format.
    #[cfg(feature = "lv2-host")]
    Lv2 {
        path: String,
        plugin_uri: String,
        is_instrument: bool,
        params: Vec<(String, f32)>,
    },
    Output,
}

/// A tonewheel organ as declared in Lua: nine drawbar params (each may be a
/// constant, an LFO, or a live `midi_connect` binding) plus the optional
/// Hammond-isms (key click, percussion, scanner vibrato/chorus, and the
/// leakage/drive/wobble character block).
struct OrganSpec {
    drawbars: [ParamSpec; 9],
    gain: ParamSpec,
    click: f32,
    percussion: Option<Percussion>,
    vibrato: Option<VibratoChorus>,
    character: OrganCharacter,
}

/// One zone of a sampler, as declared in Lua: a WAV file (already resolved to
/// an absolute path) mapped to a key range and pitched relative to `root`.
struct SamplerZoneSpec {
    file: String,
    root: u8,
    low: u8,
    high: u8,
}

enum EffectSpec {
    Gain {
        level: ParamSpec,
    },
    Reverb {
        delay: ParamSpec,
        feedback: ParamSpec,
        mix: ParamSpec,
    },
    Lowpass {
        cutoff: ParamSpec,
    },
    Highpass {
        cutoff: ParamSpec,
    },
    Distortion {
        drive: ParamSpec,
        mix: ParamSpec,
    },
    Tremolo {
        rate: ParamSpec,
        depth: ParamSpec,
    },
    Bitcrusher {
        bits: ParamSpec,
    },
    Pan {
        amount: ParamSpec,
    },
    Delay {
        time_l: DelayTimeSpec,
        time_r: DelayTimeSpec,
        feedback: ParamSpec,
        mix: ParamSpec,
        ping_pong: bool,
        glide: f32,
    },
}

enum SocketRef {
    Id(u32),
    Label(String),
}

struct PhraseSpec {
    duration_beats: f32,
    notes_source: NotesSourceSpec,
    on_enter: Option<RegistryKey>,
    on_loop: Option<RegistryKey>,
    on_exit: Option<RegistryKey>,
}

enum NotesSourceSpec {
    Static(Vec<MidiNote>),
    Dynamic(RegistryKey),
}

struct TrackSpec {
    targets: Vec<SocketRef>,
    initial: String,
    transitions: Vec<TransitionSpec>,
}

struct TransitionSpec {
    from: String,
    to: String,
    condition: Option<RegistryKey>,
}

/// Lua handle returned by `midi_connect(...)`. Carries the binding spec
/// only — the actual atomic exchange happens through the param ringbuf
/// once the MIDI thread is spawned.
#[derive(Clone)]
pub struct LiveParam {
    pub cc: u8,
    pub channel: Option<u8>, // None = match any channel
    pub initial: f32,
    pub range: (f32, f32),
}

impl UserData for LiveParam {}

/// A `LiveParam` reference attached to an effect's named param at lua-eval
/// time. Resolved into a `ResolvedBinding` once the audio graph is built
/// and node indices are known.
#[derive(Clone)]
struct PendingBinding {
    cc: u8,
    channel: Option<u8>,
    range: (f32, f32),
    socket_id: u32,
    param_idx: u8,
}

#[derive(Clone)]
struct ResolvedBinding {
    cc: u8,
    channel: Option<u8>,
    range: (f32, f32),
    node_idx: usize,
    param_idx: u8,
}

/// A MIDI binding onto a plugin parameter, addressed by *name* at Lua-eval time.
/// Resolved to `(node_idx, clap param id)` once the plugin is loaded and its
/// params are discovered.
#[cfg(any(feature = "clap-host", feature = "lv2-host"))]
#[derive(Clone)]
struct PendingPluginBinding {
    cc: u8,
    channel: Option<u8>,
    range: (f32, f32),
    socket_id: u32,
    param_name: String,
}

#[cfg(any(feature = "clap-host", feature = "lv2-host"))]
#[derive(Clone)]
struct ResolvedPluginBinding {
    cc: u8,
    channel: Option<u8>,
    range: (f32, f32),
    node_idx: usize,
    param_id: u32,
}

/// A modulator (LFO) driving a plugin parameter, addressed by name at Lua-eval
/// time. Resolved to `(node_idx, param_id, ModState)` after the plugin loads;
/// evaluated once per processing block on the audio thread.
#[cfg(any(feature = "clap-host", feature = "lv2-host"))]
#[derive(Clone)]
struct PendingPluginMod {
    socket_id: u32,
    param_name: String,
    modulator: Modulator,
}

#[cfg(any(feature = "clap-host", feature = "lv2-host"))]
struct ResolvedPluginMod {
    node_idx: usize,
    param_id: u32,
    state: crate::param::ModState,
}

#[derive(Default)]
struct SongBuilder {
    bpm: Option<f32>,
    /// Directory of the entry song file; used to resolve relative WAV paths
    /// declared in `song:sampler(...)`.
    base_dir: std::path::PathBuf,
    sockets: Vec<SocketEntry>,
    labels: HashMap<String, u32>,
    connections: Vec<(SocketRef, SocketRef)>,
    phrases: HashMap<String, PhraseSpec>,
    tracks: Vec<TrackSpec>,
    socket_ids: HashSet<u32>,
    pending_bindings: Vec<PendingBinding>,
    #[cfg(any(feature = "clap-host", feature = "lv2-host"))]
    pending_plugin_bindings: Vec<PendingPluginBinding>,
    #[cfg(any(feature = "clap-host", feature = "lv2-host"))]
    pending_plugin_mods: Vec<PendingPluginMod>,
}

impl SongBuilder {
    fn add_socket(
        &mut self,
        id: u32,
        content: SocketContent,
        label: Option<String>,
    ) -> mlua::Result<()> {
        if !self.socket_ids.insert(id) {
            return Err(mlua::Error::RuntimeError(format!(
                "duplicate socket id: {}",
                id
            )));
        }
        if let Some(l) = label
            && self.labels.insert(l.clone(), id).is_some()
        {
            return Err(mlua::Error::RuntimeError(format!("duplicate label: {}", l)));
        }
        self.sockets.push(SocketEntry { id, content });
        Ok(())
    }
}

type SharedBuilder = Arc<Mutex<Option<SongBuilder>>>;

fn parse_wave(name: &str) -> mlua::Result<Wave> {
    match name {
        "Sine" => Ok(Wave::Sine),
        "Square" => Ok(Wave::Square),
        "Saw" => Ok(Wave::Saw),
        "Triangle" => Ok(Wave::Triangle),
        "Noise" => Ok(Wave::Noise),
        other => Err(mlua::Error::RuntimeError(format!(
            "unknown wave: {}",
            other
        ))),
    }
}

fn parse_socket_ref(v: &Value) -> mlua::Result<SocketRef> {
    match v {
        Value::Integer(i) => Ok(SocketRef::Id(*i as u32)),
        Value::Number(n) => Ok(SocketRef::Id(*n as u32)),
        Value::String(s) => Ok(SocketRef::Label(s.to_str()?.to_string())),
        _ => Err(mlua::Error::RuntimeError(
            "expected socket id (integer) or label (string)".into(),
        )),
    }
}

fn label_from_opts(opts: Option<Table>) -> mlua::Result<Option<String>> {
    match opts {
        Some(t) => t.get::<Option<String>>("label"),
        None => Ok(None),
    }
}

/// Parse a MIDI binding spec string: `"cc<N>"` or `"cc<N>:ch<M>"`.
/// Channels are 1-indexed in the spec, stored 0-indexed internally.
fn parse_midi_spec(s: &str) -> mlua::Result<(u8, Option<u8>)> {
    let s = s.trim().to_lowercase();
    let parts: Vec<&str> = s.split(':').map(str::trim).collect();
    let cc_part = parts[0];
    if !cc_part.starts_with("cc") {
        return Err(mlua::Error::RuntimeError(format!(
            "midi spec must start with 'cc': {}",
            s
        )));
    }
    let cc: u8 = cc_part[2..]
        .parse()
        .map_err(|_| mlua::Error::RuntimeError(format!("invalid cc number: {}", cc_part)))?;
    if cc > 127 {
        return Err(mlua::Error::RuntimeError(format!(
            "cc must be 0..=127: {}",
            cc
        )));
    }

    let channel = if parts.len() > 1 {
        let ch_part = parts[1];
        if !ch_part.starts_with("ch") {
            return Err(mlua::Error::RuntimeError(format!(
                "channel spec must start with 'ch': {}",
                ch_part
            )));
        }
        let ch: u8 = ch_part[2..]
            .parse()
            .map_err(|_| mlua::Error::RuntimeError(format!("invalid channel: {}", ch_part)))?;
        if !(1..=16).contains(&ch) {
            return Err(mlua::Error::RuntimeError(format!(
                "channel must be 1..=16: {}",
                ch
            )));
        }
        Some(ch - 1)
    } else {
        None
    };

    Ok((cc, channel))
}

/// How a parameter was declared in Lua. Resolved into a runtime `Param` when
/// the audio graph is built in `Song::play`.
enum ParamSpec {
    /// A plain constant (possibly live-controlled via a recorded MIDI binding).
    Const(f32),
    /// An LFO-style modulator evaluated per frame on the audio thread.
    Mod(Modulator),
}

/// Parse a modulator declaration table: `{ name="sine", freq=, low=, high= }`.
/// Recognized names: sine, triangle, ramp, square.
fn parse_modulator(name: &str, t: &Table) -> mlua::Result<Modulator> {
    let freq: f32 = t.get("freq")?;
    let low: f32 = t.get::<Option<f32>>("low")?.unwrap_or(0.0);
    let high: f32 = t.get::<Option<f32>>("high")?.unwrap_or(1.0);
    Ok(match name {
        "sine" => Modulator::Sine { freq, low, high },
        "triangle" => Modulator::Triangle { freq, low, high },
        "ramp" => Modulator::Ramp { freq, low, high },
        "square" => Modulator::Square { freq, low, high },
        other => {
            return Err(mlua::Error::RuntimeError(format!(
                "unknown modulator: '{}' (expected sine, triangle, ramp, square)",
                other
            )));
        }
    })
}

/// Parse one organ param value that may be live: a number (constant), a
/// modulator table, or a `midi_connect` handle (records a binding against
/// this socket's `param_idx`, seeded at the handle's initial value).
fn parse_organ_param(
    v: Value,
    what: &str,
    socket_id: u32,
    param_idx: u8,
    builder: &mut SongBuilder,
) -> mlua::Result<ParamSpec> {
    match v {
        Value::UserData(ud) => {
            let live = ud.borrow::<LiveParam>().map_err(|_| {
                mlua::Error::RuntimeError(format!(
                    "{} has unknown userdata type (expected LiveParam from midi_connect)",
                    what
                ))
            })?;
            builder.pending_bindings.push(PendingBinding {
                cc: live.cc,
                channel: live.channel,
                range: live.range,
                socket_id,
                param_idx,
            });
            Ok(ParamSpec::Const(live.initial))
        }
        other => parse_param_value(other, what),
    }
}

/// Parse an organ declaration table:
///   { drawbars = "88 8000 000",  -- 9 digits 0-8, separators ignored — or a
///                                -- list of 9 values, each a number 0-8, a
///                                -- modulator table, or midi_connect(...)
///     gain = 0.8,                -- swell: number, modulator, or midi_connect
///     click = 0.4,               -- key-click amount 0..1 (optional)
///     percussion = { harmonic = 3, decay = "fast", volume = "soft" },
///     vibrato = "C3",            -- V1-V3/C1-C3, or { rate=, depth=, mix= }
///     leakage = 0.3, drive = 0.4, wobble = 0.5 }   -- character, all 0..1
fn parse_organ_config(
    config: &Table,
    socket_id: u32,
    builder: &mut SongBuilder,
) -> mlua::Result<OrganSpec> {
    let drawbars: [ParamSpec; 9] = match config.get::<Value>("drawbars")? {
        Value::String(s) => {
            let s = s.to_str()?.to_owned();
            let digits: Vec<ParamSpec> = s
                .chars()
                .filter(|c| !c.is_whitespace() && *c != '-')
                .map(|c| {
                    c.to_digit(10)
                        .filter(|d| *d <= 8)
                        .map(|d| ParamSpec::Const(d as f32))
                        .ok_or_else(|| {
                            mlua::Error::RuntimeError(format!(
                                "organ drawbars must be digits 0-8, got '{}' in \"{}\"",
                                c, s
                            ))
                        })
                })
                .collect::<mlua::Result<_>>()?;
            digits.try_into().map_err(|v: Vec<ParamSpec>| {
                mlua::Error::RuntimeError(format!(
                    "organ drawbars needs exactly 9 digits (e.g. \"888000000\"), got {}",
                    v.len()
                ))
            })?
        }
        Value::Table(list) => {
            let mut bars = Vec::with_capacity(9);
            for (i, v) in list.sequence_values::<Value>().enumerate() {
                bars.push(parse_organ_param(
                    v?,
                    "organ drawbar",
                    socket_id,
                    Organ::PARAM_DRAWBAR_BASE + i as u8,
                    builder,
                )?);
            }
            bars.try_into().map_err(|v: Vec<ParamSpec>| {
                mlua::Error::RuntimeError(format!(
                    "organ drawbars list needs exactly 9 entries, got {}",
                    v.len()
                ))
            })?
        }
        _ => {
            return Err(mlua::Error::RuntimeError(
                "organ 'drawbars' must be a 9-digit string or a list of 9 values".into(),
            ));
        }
    };

    let gain = match config.get::<Value>("gain")? {
        Value::Nil => ParamSpec::Const(0.8),
        v => parse_organ_param(v, "organ gain", socket_id, Organ::PARAM_GAIN, builder)?,
    };
    let click: f32 = config.get::<Option<f32>>("click")?.unwrap_or(0.3);

    let percussion = match config.get::<Option<Table>>("percussion")? {
        Some(t) => {
            let harmonic: u8 = t.get::<Option<u8>>("harmonic")?.unwrap_or(3);
            if harmonic != 2 && harmonic != 3 {
                return Err(mlua::Error::RuntimeError(
                    "organ percussion harmonic must be 2 or 3".into(),
                ));
            }
            // The real switch is fast/slow; a number in seconds also works.
            let decay = match t.get::<Value>("decay")? {
                Value::Nil => 0.25,
                Value::Number(n) => n as f32,
                Value::Integer(i) => i as f32,
                Value::String(s) => match s.to_str()?.as_ref() {
                    "fast" => 0.25,
                    "slow" => 1.0,
                    other => {
                        return Err(mlua::Error::RuntimeError(format!(
                            "organ percussion decay must be \"fast\", \"slow\" or seconds, got \"{}\"",
                            other
                        )));
                    }
                },
                _ => {
                    return Err(mlua::Error::RuntimeError(
                        "organ percussion decay must be \"fast\", \"slow\" or seconds".into(),
                    ));
                }
            };
            // Normal/soft volume switch; an explicit `level` overrides it.
            let volume = match t.get::<Option<String>>("volume")?.as_deref() {
                None | Some("normal") => 0.7,
                Some("soft") => 0.3,
                Some(other) => {
                    return Err(mlua::Error::RuntimeError(format!(
                        "organ percussion volume must be \"normal\" or \"soft\", got \"{}\"",
                        other
                    )));
                }
            };
            let level = t.get::<Option<f32>>("level")?.unwrap_or(volume);
            Some(Percussion::new(harmonic, decay, level))
        }
        None => None,
    };

    let vibrato = match config.get::<Value>("vibrato")? {
        Value::Nil => None,
        // The six-position switch: "V1".."V3", "C1".."C3".
        Value::String(s) => {
            let mode = s.to_str()?.to_owned();
            Some(VibratoChorus::from_mode(&mode).ok_or_else(|| {
                mlua::Error::RuntimeError(format!(
                    "unknown organ vibrato mode \"{}\" (expected V1-V3 or C1-C3)",
                    mode
                ))
            })?)
        }
        // Table form: either { mode = "C3" } with optional overrides, or the
        // raw knobs { rate=, depth=, mix= } (depth in cents, mix = dry blend).
        Value::Table(t) => {
            let base = match t.get::<Option<String>>("mode")? {
                Some(mode) => VibratoChorus::from_mode(&mode).ok_or_else(|| {
                    mlua::Error::RuntimeError(format!(
                        "unknown organ vibrato mode \"{}\" (expected V1-V3 or C1-C3)",
                        mode
                    ))
                })?,
                None => VibratoChorus {
                    rate: 6.9,
                    depth_cents: 7.0,
                    chorus_mix: 0.0,
                },
            };
            Some(VibratoChorus {
                rate: t.get::<Option<f32>>("rate")?.unwrap_or(base.rate),
                depth_cents: t.get::<Option<f32>>("depth")?.unwrap_or(base.depth_cents),
                chorus_mix: t
                    .get::<Option<f32>>("mix")?
                    .unwrap_or(base.chorus_mix)
                    .clamp(0.0, 1.0),
            })
        }
        _ => {
            return Err(mlua::Error::RuntimeError(
                "organ vibrato must be a mode string (V1-V3/C1-C3) or a table".into(),
            ));
        }
    };

    let character = OrganCharacter {
        leakage: config
            .get::<Option<f32>>("leakage")?
            .unwrap_or(0.0)
            .clamp(0.0, 1.0),
        drive: config
            .get::<Option<f32>>("drive")?
            .unwrap_or(0.0)
            .clamp(0.0, 1.0),
        wobble: config
            .get::<Option<f32>>("wobble")?
            .unwrap_or(0.0)
            .clamp(0.0, 1.0),
    };

    Ok(OrganSpec {
        drawbars,
        gain,
        click,
        percussion,
        vibrato,
        character,
    })
}

/// Pull a param out of a Lua params table. A param may be:
///   - a number → constant
///   - a `midi_connect` handle → constant, with a recorded MIDI binding
///   - a modulator table `{ name="sine", ... }` → per-frame LFO
fn extract_param(
    params: &Table,
    name: &str,
    socket_id: u32,
    param_idx: u8,
    builder: &mut SongBuilder,
) -> mlua::Result<ParamSpec> {
    let v: Value = params.get(name)?;
    match v {
        Value::Number(n) => Ok(ParamSpec::Const(n as f32)),
        Value::Integer(i) => Ok(ParamSpec::Const(i as f32)),
        Value::Table(t) => {
            let mod_name: String = t.get("name").map_err(|_| {
                mlua::Error::RuntimeError(format!(
                    "'{}' table must have a 'name' field (modulator)",
                    name
                ))
            })?;
            Ok(ParamSpec::Mod(parse_modulator(&mod_name, &t)?))
        }
        Value::UserData(ud) => {
            let live = ud.borrow::<LiveParam>().map_err(|_| {
                mlua::Error::RuntimeError(format!(
                    "'{}' has unknown userdata type (expected LiveParam from midi_connect)",
                    name
                ))
            })?;
            builder.pending_bindings.push(PendingBinding {
                cc: live.cc,
                channel: live.channel,
                range: live.range,
                socket_id,
                param_idx,
            });
            Ok(ParamSpec::Const(live.initial))
        }
        Value::Nil => Err(mlua::Error::RuntimeError(format!(
            "missing param: '{}'",
            name
        ))),
        _ => Err(mlua::Error::RuntimeError(format!(
            "'{}' must be a number, a modulator table, or a midi_connect handle",
            name
        ))),
    }
}

/// Parse a param value that supports constants and modulators but not MIDI
/// binding (e.g. synth oscillator gain, which has no graph `set_param` route).
fn parse_param_value(v: Value, what: &str) -> mlua::Result<ParamSpec> {
    match v {
        Value::Number(n) => Ok(ParamSpec::Const(n as f32)),
        Value::Integer(i) => Ok(ParamSpec::Const(i as f32)),
        Value::Table(t) => {
            let mod_name: String = t.get("name").map_err(|_| {
                mlua::Error::RuntimeError(format!(
                    "{} table must have a 'name' field (modulator)",
                    what
                ))
            })?;
            Ok(ParamSpec::Mod(parse_modulator(&mod_name, &t)?))
        }
        _ => Err(mlua::Error::RuntimeError(format!(
            "{} must be a number or a modulator table",
            what
        ))),
    }
}

/// Parse a plugin `params` subtable, shared by the CLAP and LV2 branches of
/// `instrument()`/`effect()`. Each entry is one of:
///   - a number → initial value
///   - a `midi_connect` handle → recorded as a pending plugin binding
///   - a modulator table `{ name="sine", ... }` → recorded as a pending mod
///
/// `kind` is "clap" or "lv2", used only for error messages. Returns the list
/// of `(name, initial_value)` pairs to seed the plugin's params.
#[cfg(any(feature = "clap-host", feature = "lv2-host"))]
fn parse_plugin_params(
    builder: &mut SongBuilder,
    socket_id: u32,
    config: &Table,
    kind: &str,
) -> mlua::Result<Vec<(String, f32)>> {
    let mut params: Vec<(String, f32)> = Vec::new();
    if let Some(params_t) = config.get::<Option<Table>>("params")? {
        for pair in params_t.pairs::<String, Value>() {
            let (name, value) = pair?;
            match value {
                Value::Number(n) => params.push((name, n as f32)),
                Value::Integer(i) => params.push((name, i as f32)),
                Value::UserData(ud) => {
                    let live = ud.borrow::<LiveParam>().map_err(|_| {
                        mlua::Error::RuntimeError(format!(
                            "{} param '{}' has unknown userdata (expected midi_connect handle)",
                            kind, name
                        ))
                    })?;
                    builder.pending_plugin_bindings.push(PendingPluginBinding {
                        cc: live.cc,
                        channel: live.channel,
                        range: live.range,
                        socket_id,
                        param_name: name.clone(),
                    });
                    params.push((name, live.initial));
                }
                Value::Table(t) => {
                    let mod_name: String = t.get("name")?;
                    let modulator = parse_modulator(&mod_name, &t)?;
                    builder.pending_plugin_mods.push(PendingPluginMod {
                        socket_id,
                        param_name: name.clone(),
                        modulator,
                    });
                    params.push((name, modulator_initial_value(&modulator)));
                }
                _ => {
                    return Err(mlua::Error::RuntimeError(format!(
                        "{} param '{}' must be a number, modulator table, or midi_connect handle",
                        kind, name
                    )));
                }
            }
        }
    }
    Ok(params)
}

/// If `config` carries a `clap` or `lv2` key, add the corresponding plugin
/// socket and return `true`. Otherwise return `false` so the caller falls back
/// to parsing a built-in instrument/effect. `is_instrument` distinguishes the
/// `instrument()` and `effect()` call sites.
///
/// Plugin paths are resolved relative to the song file's directory. The label
/// is read from the config table's `label` key (handled by the caller).
#[cfg(any(feature = "clap-host", feature = "lv2-host"))]
fn try_add_plugin_socket(
    builder: &mut SongBuilder,
    socket_id: u32,
    config: &Table,
    is_instrument: bool,
    label: Option<String>,
) -> mlua::Result<bool> {
    let resolve = |path: &str, builder: &SongBuilder| -> String {
        let p = std::path::Path::new(path);
        let resolved = if p.is_absolute() {
            p.to_path_buf()
        } else {
            builder.base_dir.join(p)
        };
        resolved.to_string_lossy().into_owned()
    };

    #[cfg(feature = "clap-host")]
    if let Some(path) = config.get::<Option<String>>("clap")? {
        let plugin_id: String = config.get("id")?;
        let params = parse_plugin_params(builder, socket_id, config, "clap")?;
        let resolved = resolve(&path, builder);
        builder.add_socket(
            socket_id,
            SocketContent::Plugin {
                path: resolved,
                plugin_id,
                is_instrument,
                params,
            },
            label,
        )?;
        return Ok(true);
    }
    #[cfg(not(feature = "clap-host"))]
    if config.contains_key("clap")? {
        return Err(mlua::Error::RuntimeError(
            "this build has no CLAP support (rebuild with --features clap-host)".into(),
        ));
    }

    #[cfg(feature = "lv2-host")]
    if let Some(path) = config.get::<Option<String>>("lv2")? {
        // `lv2` is the .lv2 bundle dir; `uri` selects the plugin within it.
        let plugin_uri: String = config.get("uri")?;
        let params = parse_plugin_params(builder, socket_id, config, "lv2")?;
        let resolved = resolve(&path, builder);
        builder.add_socket(
            socket_id,
            SocketContent::Lv2 {
                path: resolved,
                plugin_uri,
                is_instrument,
                params,
            },
            label,
        )?;
        return Ok(true);
    }
    #[cfg(not(feature = "lv2-host"))]
    if config.contains_key("lv2")? {
        return Err(mlua::Error::RuntimeError(
            "this build has no LV2 support (rebuild with --features lv2-host)".into(),
        ));
    }

    Ok(false)
}

/// Stub for builds with no plugin support: detect a plugin key and error with a
/// hint, otherwise report "not a plugin" so the caller parses a built-in.
#[cfg(not(any(feature = "clap-host", feature = "lv2-host")))]
fn try_add_plugin_socket(
    _builder: &mut SongBuilder,
    _socket_id: u32,
    config: &Table,
    _is_instrument: bool,
    _label: Option<String>,
) -> mlua::Result<bool> {
    if config.contains_key("clap")? || config.contains_key("lv2")? {
        return Err(mlua::Error::RuntimeError(
            "this build has no plugin support (rebuild with --features clap-host,lv2-host)".into(),
        ));
    }
    Ok(false)
}

impl ParamSpec {
    /// Build the runtime `Param`.
    fn build(self, sample_rate: f32) -> Param {
        match self {
            ParamSpec::Const(v) => Param::constant(v, sample_rate),
            ParamSpec::Mod(m) => Param::modulator(m),
        }
    }
}

/// A delay-time value as declared in Lua. Times resolve to milliseconds; note
/// durations need BPM, so they're kept as beats until the graph is built.
enum DelayTimeSpec {
    /// Already in milliseconds.
    Ms(f32),
    /// A note duration in beats (e.g. "4." → 1.5), converted to ms via BPM.
    Beats(f32),
    /// A modulator whose low/high are in milliseconds.
    Mod(Modulator),
}

/// Parse an RTTTL-style note-duration string into beats: the number is the
/// note value (4 = quarter = 1 beat, 8 = eighth = 0.5 beat), and a trailing
/// '.' dots it (×1.5).
fn parse_note_duration(s: &str) -> mlua::Result<f32> {
    let dotted = s.ends_with('.');
    let digits = s.trim_end_matches('.');
    let value: f32 = digits.parse().map_err(|_| {
        mlua::Error::RuntimeError(format!(
            "invalid note duration '{}' (expected e.g. \"4\", \"8.\")",
            s
        ))
    })?;
    if value <= 0.0 {
        return Err(mlua::Error::RuntimeError(format!(
            "note duration must be positive: '{}'",
            s
        )));
    }
    let beats = 4.0 / value;
    Ok(if dotted { beats * 1.5 } else { beats })
}

/// Parse one delay-time value: a number (ms), a note-duration string, a
/// modulator table (ms), or a `midi_connect` handle (records a binding on
/// `param_idx` and seeds the initial ms value).
fn parse_delay_time(
    v: Value,
    socket_id: u32,
    param_idx: u8,
    builder: &mut SongBuilder,
) -> mlua::Result<DelayTimeSpec> {
    match v {
        Value::Number(n) => Ok(DelayTimeSpec::Ms(n as f32)),
        Value::Integer(i) => Ok(DelayTimeSpec::Ms(i as f32)),
        Value::String(s) => Ok(DelayTimeSpec::Beats(parse_note_duration(&s.to_str()?)?)),
        Value::UserData(ud) => {
            let live = ud.borrow::<LiveParam>().map_err(|_| {
                mlua::Error::RuntimeError(
                    "delay time userdata must be a midi_connect handle".into(),
                )
            })?;
            builder.pending_bindings.push(PendingBinding {
                cc: live.cc,
                channel: live.channel,
                range: live.range,
                socket_id,
                param_idx,
            });
            // The MIDI thread writes ms directly into the param's target; start
            // at the handle's initial value (also ms).
            Ok(DelayTimeSpec::Ms(live.initial))
        }
        Value::Table(t) => {
            let name: String = t.get("name").map_err(|_| {
                mlua::Error::RuntimeError(
                    "delay time table must be a modulator with a 'name' field".into(),
                )
            })?;
            Ok(DelayTimeSpec::Mod(parse_modulator(&name, &t)?))
        }
        _ => Err(mlua::Error::RuntimeError(
            "delay time must be a number (ms), a note string like \"4.\", a modulator, or midi_connect".into(),
        )),
    }
}

/// Parse the `time` field, which is either a single value (mono → both
/// channels) or a `{left, right}` pair. Returns (left, right). A single
/// `midi_connect` handle binds *both* taps so one knob moves them together.
fn parse_delay_times(
    params: &Table,
    socket_id: u32,
    builder: &mut SongBuilder,
) -> mlua::Result<(DelayTimeSpec, DelayTimeSpec)> {
    let v: Value = params.get("time")?;
    match v {
        // A 2-element array (no `name` key) is an L/R pair; a table *with*
        // `name` is a single modulator value.
        Value::Table(ref t) if t.get::<Option<Value>>("name")?.is_none() && t.raw_len() == 2 => {
            let left = parse_delay_time(t.get(1)?, socket_id, Delay::PARAM_TIME_L, builder)?;
            let right = parse_delay_time(t.get(2)?, socket_id, Delay::PARAM_TIME_R, builder)?;
            Ok((left, right))
        }
        Value::Nil => Err(mlua::Error::RuntimeError("delay missing 'time'".into())),
        other => {
            // Single value applies to both channels. Re-parse for each side so
            // each owns its own spec and (for a midi_connect handle) records a
            // binding on both PARAM_TIME_L and PARAM_TIME_R.
            let left = parse_delay_time(other.clone(), socket_id, Delay::PARAM_TIME_L, builder)?;
            let right = parse_delay_time(other, socket_id, Delay::PARAM_TIME_R, builder)?;
            Ok((left, right))
        }
    }
}

impl DelayTimeSpec {
    /// Resolve to a runtime `Param` (ms), converting beats via BPM. Also
    /// returns the initial delay in samples so the line can start in time.
    fn build(self, bpm: f32, sample_rate: f32) -> (Param, f32) {
        let ms = match self {
            DelayTimeSpec::Ms(ms) => ms,
            DelayTimeSpec::Beats(beats) => beats * (60_000.0 / bpm),
            DelayTimeSpec::Mod(m) => {
                // For a modulator the value moves on its own; use the param's
                // first value as the initial sample count.
                let param = Param::modulator(m);
                let initial_samples = modulator_initial_value(&m) * 0.001 * sample_rate;
                return (param, initial_samples);
            }
        };
        let param = Param::constant(ms, sample_rate);
        (param, ms * 0.001 * sample_rate)
    }
}

/// The value a modulator produces at phase 0 (its starting point). Used to seed
/// a delay line's initial read position and a plugin param's initial value.
fn modulator_initial_value(m: &Modulator) -> f32 {
    match *m {
        // All shapes start at `low` at phase 0 in our implementation.
        Modulator::Sine { low, .. }
        | Modulator::Triangle { low, .. }
        | Modulator::Ramp { low, .. }
        | Modulator::Square { low, .. } => low,
    }
}

fn register_methods(lua: &Lua, song: &Table, builder: &SharedBuilder) -> mlua::Result<()> {
    {
        let b = builder.clone();
        song.set(
            "bpm",
            lua.create_function(move |_, (_self, bpm): (Value, f32)| {
                b.lock().unwrap().as_mut().unwrap().bpm = Some(bpm);
                Ok(())
            })?,
        )?;
    }

    {
        let b = builder.clone();
        song.set(
            "instrument",
            lua.create_function(
                move |_, (_self, id, config, opts): (Value, u32, Table, Option<Table>)| {
                    // A `clap`/`lv2` key turns this into a plugin instrument.
                    // Label may live in the trailing opts or in the config table.
                    let label = match label_from_opts(opts)? {
                        Some(l) => Some(l),
                        None => config.get::<Option<String>>("label")?,
                    };
                    {
                        let mut guard = b.lock().unwrap();
                        let builder = guard.as_mut().unwrap();
                        if try_add_plugin_socket(builder, id, &config, true, label.clone())? {
                            return Ok(());
                        }
                    }

                    // A `drawbars` key makes this a built-in tonewheel organ.
                    if config.contains_key("drawbars")? {
                        let mut guard = b.lock().unwrap();
                        let builder = guard.as_mut().unwrap();
                        let spec = parse_organ_config(&config, id, builder)?;
                        builder.add_socket(id, SocketContent::Organ(spec), label)?;
                        return Ok(());
                    }

                    let adsr_t: Table = config.get("adsr")?;
                    let adsr = ADSR {
                        a: adsr_t.get(1)?,
                        d: adsr_t.get(2)?,
                        s: adsr_t.get(3)?,
                        r: adsr_t.get(4)?,
                    };
                    let osc_t: Table = config.get("oscillators")?;
                    let mut oscillators = Vec::new();
                    for v in osc_t.sequence_values::<Table>() {
                        let osc = v?;
                        let wave_str: String = osc.get("wave")?;
                        let wave = parse_wave(&wave_str)?;
                        let gain_value: Value = osc.get("gain")?;
                        let gain = parse_param_value(gain_value, "oscillator gain")?;
                        oscillators.push((wave, gain));
                    }
                    b.lock().unwrap().as_mut().unwrap().add_socket(
                        id,
                        SocketContent::Instrument { adsr, oscillators },
                        label,
                    )?;
                    Ok(())
                },
            )?,
        )?;
    }

    {
        let b = builder.clone();
        song.set(
            "sampler",
            lua.create_function(
                move |_, (_self, id, config, opts): (Value, u32, Table, Option<Table>)| {
                    let mut guard = b.lock().unwrap();
                    let builder = guard.as_mut().unwrap();
                    let base_dir = builder.base_dir.clone();

                    // Parse one zone from a Lua table. `low`/`high` default to
                    // `root` (a single-key mapping); `root` defaults to c3.
                    let parse_zone = |t: &Table| -> mlua::Result<SamplerZoneSpec> {
                        let file: String = t.get("file")?;
                        let root = match t.get::<Value>("root")? {
                            Value::Nil => note_name_to_midi("c3")
                                .map_err(mlua::Error::RuntimeError)?,
                            v => parse_note_value(&v)?,
                        };
                        let low = match t.get::<Value>("low")? {
                            Value::Nil => root,
                            v => parse_note_value(&v)?,
                        };
                        let high = match t.get::<Value>("high")? {
                            Value::Nil => root,
                            v => parse_note_value(&v)?,
                        };
                        if low > high {
                            return Err(mlua::Error::RuntimeError(format!(
                                "sampler zone for '{}' has low > high",
                                file
                            )));
                        }
                        // Resolve relative WAV paths against the song's dir.
                        let path = base_dir.join(&file);
                        Ok(SamplerZoneSpec {
                            file: path.to_string_lossy().into_owned(),
                            root,
                            low,
                            high,
                        })
                    };

                    // Two forms: a single `file=...` (basic), or a `samples`
                    // list of zone tables (advanced).
                    let zones = match config.get::<Value>("samples")? {
                        Value::Table(list) => {
                            let mut zones = Vec::new();
                            for v in list.sequence_values::<Table>() {
                                zones.push(parse_zone(&v?)?);
                            }
                            if zones.is_empty() {
                                return Err(mlua::Error::RuntimeError(
                                    "sampler 'samples' list is empty".into(),
                                ));
                            }
                            zones
                        }
                        Value::Nil => {
                            // Basic single-sample form: map the whole keyboard.
                            let mut zone = parse_zone(&config)?;
                            zone.low = 0;
                            zone.high = 127;
                            vec![zone]
                        }
                        _ => {
                            return Err(mlua::Error::RuntimeError(
                                "sampler 'samples' must be a list of zone tables".into(),
                            ));
                        }
                    };

                    let label = label_from_opts(opts)?;
                    builder.add_socket(id, SocketContent::Sampler { zones }, label)?;
                    Ok(())
                },
            )?,
        )?;
    }

    {
        let b = builder.clone();
        song.set(
            "effect",
            lua.create_function(
                move |inner_lua, (_self, id, config, opts): (Value, u32, Table, Option<Table>)| {
                    // Label may live in the trailing opts or in the config table.
                    let label = match label_from_opts(opts)? {
                        Some(l) => Some(l),
                        None => config.get::<Option<String>>("label")?,
                    };

                    let mut guard = b.lock().unwrap();
                    let builder = guard.as_mut().unwrap();

                    // A `clap`/`lv2` key makes this a plugin effect.
                    if try_add_plugin_socket(builder, id, &config, false, label.clone())? {
                        return Ok(());
                    }

                    // Built-in effect: `kind` names it, `params` holds its values.
                    // An omitted `params` becomes an empty table so effects whose
                    // fields all default still work.
                    let kind: String = config.get("kind").map_err(|_| {
                        mlua::Error::RuntimeError(
                            "effect table must have a 'kind' field (or a 'clap'/'lv2' plugin key)"
                                .into(),
                        )
                    })?;
                    let params: Table = match config.get::<Option<Table>>("params")? {
                        Some(t) => t,
                        None => inner_lua.create_table()?,
                    };

                    let spec = match kind.as_str() {
                        "Gain" => {
                            let level =
                                extract_param(&params, "level", id, Gain::PARAM_LEVEL, builder)?;
                            EffectSpec::Gain { level }
                        }
                        "Reverb" => EffectSpec::Reverb {
                            delay: extract_param(
                                &params,
                                "delay",
                                id,
                                Reverb::PARAM_DELAY,
                                builder,
                            )?,
                            feedback: extract_param(
                                &params,
                                "feedback",
                                id,
                                Reverb::PARAM_FEEDBACK,
                                builder,
                            )?,
                            mix: extract_param(&params, "mix", id, Reverb::PARAM_MIX, builder)?,
                        },
                        "Lowpass" => EffectSpec::Lowpass {
                            cutoff: extract_param(
                                &params,
                                "cutoff",
                                id,
                                Lowpass::PARAM_CUTOFF,
                                builder,
                            )?,
                        },
                        "Highpass" => EffectSpec::Highpass {
                            cutoff: extract_param(
                                &params,
                                "cutoff",
                                id,
                                Highpass::PARAM_CUTOFF,
                                builder,
                            )?,
                        },
                        "Distortion" => {
                            let drive = extract_param(
                                &params,
                                "drive",
                                id,
                                Distortion::PARAM_DRIVE,
                                builder,
                            )?;
                            let mix = extract_param(
                                &params,
                                "mix",
                                id,
                                Distortion::PARAM_MIX,
                                builder,
                            )?;
                            EffectSpec::Distortion { drive, mix }
                        }
                        "Tremolo" => EffectSpec::Tremolo {
                            rate: extract_param(&params, "rate", id, Tremolo::PARAM_RATE, builder)?,
                            depth: extract_param(
                                &params,
                                "depth",
                                id,
                                Tremolo::PARAM_DEPTH,
                                builder,
                            )?,
                        },
                        "Bitcrusher" => EffectSpec::Bitcrusher {
                            bits: extract_param(
                                &params,
                                "bits",
                                id,
                                Bitcrusher::PARAM_BITS,
                                builder,
                            )?,
                        },
                        "Pan" => {
                            let amount =
                                extract_param(&params, "amount", id, Pan::PARAM_AMOUNT, builder)?;
                            EffectSpec::Pan { amount }
                        }
                        "Delay" => {
                            let (time_l, time_r) = parse_delay_times(&params, id, builder)?;
                            // feedback/mix default to sensible values if omitted.
                            let feedback = if params.contains_key("feedback")? {
                                extract_param(
                                    &params,
                                    "feedback",
                                    id,
                                    Delay::PARAM_FEEDBACK,
                                    builder,
                                )?
                            } else {
                                ParamSpec::Const(0.3)
                            };
                            let mix = if params.contains_key("mix")? {
                                extract_param(&params, "mix", id, Delay::PARAM_MIX, builder)?
                            } else {
                                ParamSpec::Const(0.5)
                            };
                            let ping_pong =
                                params.get::<Option<bool>>("ping_pong")?.unwrap_or(false);
                            // Glide time constant in seconds; 0 (default) = no
                            // glide, no Doppler.
                            let glide = params.get::<Option<f32>>("glide")?.unwrap_or(0.0);
                            EffectSpec::Delay {
                                time_l,
                                time_r,
                                feedback,
                                mix,
                                ping_pong,
                                glide,
                            }
                        }
                        other => {
                            return Err(mlua::Error::RuntimeError(format!(
                                "unknown effect: {}",
                                other
                            )));
                        }
                    };
                    builder.add_socket(id, SocketContent::Effect(spec), label)?;
                    Ok(())
                },
            )?,
        )?;
    }

    {
        let b = builder.clone();
        song.set(
            "output",
            lua.create_function(move |_, (_self, id, opts): (Value, u32, Option<Table>)| {
                let label = label_from_opts(opts)?;
                b.lock()
                    .unwrap()
                    .as_mut()
                    .unwrap()
                    .add_socket(id, SocketContent::Output, label)?;
                Ok(())
            })?,
        )?;
    }

    {
        let b = builder.clone();
        song.set(
            "connection",
            lua.create_function(move |_, (_self, from, to): (Value, Value, Value)| {
                let from_ref = parse_socket_ref(&from)?;
                let to_ref = parse_socket_ref(&to)?;
                b.lock()
                    .unwrap()
                    .as_mut()
                    .unwrap()
                    .connections
                    .push((from_ref, to_ref));
                Ok(())
            })?,
        )?;
    }

    {
        let b = builder.clone();
        song.set(
            "phrase",
            lua.create_function(
                move |inner_lua, (_self, name, opts): (Value, String, Table)| {
                    let rtttl_value: Value = opts.get("rtttl").unwrap_or(Value::Nil);
                    let notes_value: Value = opts.get("notes").unwrap_or(Value::Nil);

                    let (notes_source, duration_beats) = if let Value::String(s) = rtttl_value {
                        let rtttl_str = s.to_str()?.to_string();
                        let (duration, notes) = parse_rtttl(&rtttl_str);
                        (NotesSourceSpec::Static(notes), duration)
                    } else {
                        let duration: f32 = opts.get("duration")?;
                        match notes_value {
                            Value::Table(t) => {
                                let notes = notes_from_table(&t)?;
                                (NotesSourceSpec::Static(notes), duration)
                            }
                            Value::Function(f) => {
                                let key = inner_lua.create_registry_value(f)?;
                                (NotesSourceSpec::Dynamic(key), duration)
                            }
                            _ => {
                                return Err(mlua::Error::RuntimeError(format!(
                                    "phrase '{}' must have notes (table or function) or rtttl",
                                    name
                                )));
                            }
                        }
                    };

                    let on_enter = opts
                        .get::<Option<Function>>("on_enter")?
                        .map(|f| inner_lua.create_registry_value(f))
                        .transpose()?;
                    let on_loop = opts
                        .get::<Option<Function>>("on_loop")?
                        .map(|f| inner_lua.create_registry_value(f))
                        .transpose()?;
                    let on_exit = opts
                        .get::<Option<Function>>("on_exit")?
                        .map(|f| inner_lua.create_registry_value(f))
                        .transpose()?;

                    let mut bg = b.lock().unwrap();
                    let bs = bg.as_mut().unwrap();
                    if bs.phrases.contains_key(&name) {
                        return Err(mlua::Error::RuntimeError(format!(
                            "duplicate phrase name: {}",
                            name
                        )));
                    }
                    bs.phrases.insert(
                        name,
                        PhraseSpec {
                            duration_beats,
                            notes_source,
                            on_enter,
                            on_loop,
                            on_exit,
                        },
                    );
                    Ok(())
                },
            )?,
        )?;
    }

    {
        let b = builder.clone();
        song.set(
            "track",
            lua.create_function(move |inner_lua, (_self, opts): (Value, Table)| {
                let targets_t: Table = opts.get("targets")?;
                let mut targets = Vec::new();
                for v in targets_t.sequence_values::<Value>() {
                    targets.push(parse_socket_ref(&v?)?);
                }

                let initial: String = opts.get("initial")?;

                let transitions_t: Option<Table> = opts.get("transitions")?;
                let mut transitions = Vec::new();
                if let Some(tt) = transitions_t {
                    for v in tt.sequence_values::<Table>() {
                        let t = v?;
                        let from: String = t.get("from")?;
                        let to: String = t.get("to")?;
                        let condition_fn: Option<Function> = t.get("condition")?;
                        let condition = condition_fn
                            .map(|f| inner_lua.create_registry_value(f))
                            .transpose()?;
                        transitions.push(TransitionSpec {
                            from,
                            to,
                            condition,
                        });
                    }
                }

                b.lock().unwrap().as_mut().unwrap().tracks.push(TrackSpec {
                    targets,
                    initial,
                    transitions,
                });
                Ok(())
            })?,
        )?;
    }

    Ok(())
}

impl Song {
    pub fn load<P: AsRef<Path>>(
        path: P,
        lua: &LuaRuntime,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let parent = path
            .as_ref()
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));

        let builder: SharedBuilder = Arc::new(Mutex::new(Some(SongBuilder {
            base_dir: parent.clone(),
            ..SongBuilder::default()
        })));
        let song_table = lua.lua().create_table()?;
        register_methods(lua.lua(), &song_table, &builder)?;
        lua.lua().globals().set("song", song_table)?;

        // Global helper: midi_connect("cc<N>") | midi_connect{ cc=N, ... }
        // Returns a LiveParam userdata that effect param parsers recognize.
        let midi_connect = lua.lua().create_function(|_, arg: Value| match arg {
            Value::String(s) => {
                let (cc, channel) = parse_midi_spec(&s.to_str()?)?;
                Ok(LiveParam {
                    cc,
                    channel,
                    initial: 0.0,
                    range: (0.0, 1.0),
                })
            }
            Value::Table(t) => {
                let cc: u8 = t.get("cc")?;
                if cc > 127 {
                    return Err(mlua::Error::RuntimeError(format!(
                        "cc must be 0..=127: {}",
                        cc
                    )));
                }
                let channel = t.get::<Option<u8>>("channel")?;
                let channel = match channel {
                    None => None,
                    Some(c) if (1..=16).contains(&c) => Some(c - 1),
                    Some(c) => {
                        return Err(mlua::Error::RuntimeError(format!(
                            "channel must be 1..=16: {}",
                            c
                        )));
                    }
                };
                let initial: f32 = t.get::<Option<f32>>("initial")?.unwrap_or(0.0);
                let range = match t.get::<Option<Table>>("range")? {
                    Some(rt) => (rt.get(1)?, rt.get(2)?),
                    None => (0.0, 1.0),
                };
                Ok(LiveParam {
                    cc,
                    channel,
                    initial,
                    range,
                })
            }
            _ => Err(mlua::Error::RuntimeError(
                "midi_connect expects a string spec or a table".into(),
            )),
        })?;
        lua.lua().globals().set("midi_connect", midi_connect)?;

        let code = std::fs::read_to_string(path.as_ref())?;
        let chunk_name = path.as_ref().display().to_string();

        // Make `require` resolve modules relative to the entry file's directory.
        let parent_str = parent.display().to_string();
        let package: Table = lua.lua().globals().get("package")?;
        let current_path: String = package.get("path")?;
        package.set(
            "path",
            format!("{0}/?.lua;{0}/?/init.lua;{1}", parent_str, current_path),
        )?;

        lua.lua().load(&code).set_name(chunk_name).exec()?;

        let state = builder
            .lock()
            .unwrap()
            .take()
            .ok_or("song builder was already taken")?;

        Ok(Song {
            bpm: state.bpm.unwrap_or(120.0),
            sockets: state.sockets,
            labels: state.labels,
            connections: state.connections,
            phrases: state.phrases,
            tracks: state.tracks,
            pending_bindings: state.pending_bindings,
            #[cfg(any(feature = "clap-host", feature = "lv2-host"))]
            pending_plugin_bindings: state.pending_plugin_bindings,
            #[cfg(any(feature = "clap-host", feature = "lv2-host"))]
            pending_plugin_mods: state.pending_plugin_mods,
        })
    }

    pub fn play(self, lua: LuaRuntime) -> ! {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .expect("No audio output device found");
        let stream_config = device.default_output_config().unwrap();
        let sample_rate = stream_config.sample_rate() as f32;
        let channels = stream_config.channels() as usize;

        println!("Audio: {} Hz, {} channels", sample_rate, channels);
        println!("Playing at {} BPM...", self.bpm);

        let Song {
            bpm,
            sockets,
            labels,
            connections,
            phrases,
            tracks,
            pending_bindings,
            #[cfg(any(feature = "clap-host", feature = "lv2-host"))]
            pending_plugin_bindings,
            #[cfg(any(feature = "clap-host", feature = "lv2-host"))]
            pending_plugin_mods,
        } = self;

        let mut audio_nodes: Vec<AudioNode> = Vec::new();
        let mut socket_to_node: HashMap<u32, usize> = HashMap::new();
        // socket id → (lowercased param name → CLAP param id), captured while
        // building plugin nodes so name-based MIDI bindings can be resolved.
        #[cfg(any(feature = "clap-host", feature = "lv2-host"))]
        let mut plugin_param_ids: HashMap<u32, HashMap<String, u32>> = HashMap::new();

        for socket in sockets {
            let node = match socket.content {
                SocketContent::Instrument { adsr, oscillators } => {
                    let oscillators: Vec<(Oscillator, Param)> = oscillators
                        .into_iter()
                        .map(|(wave, gain)| (Oscillator::new(wave), gain.build(sample_rate)))
                        .collect();
                    let synth = Synth::new(socket.id as usize, adsr, oscillators);
                    AudioNode::Source(Box::new(synth))
                }
                SocketContent::Organ(spec) => {
                    let organ = Organ::new(
                        socket.id as usize,
                        spec.drawbars.map(|d| d.build(sample_rate)),
                        spec.gain.build(sample_rate),
                        spec.click,
                        spec.percussion,
                        spec.vibrato,
                        spec.character,
                    );
                    AudioNode::Source(Box::new(organ))
                }
                SocketContent::Sampler { zones } => {
                    // Load every WAV up front (setup time, never the audio
                    // callback). A failed load is fatal — better to know now.
                    let zones: Vec<Zone> = zones
                        .into_iter()
                        .map(|z| {
                            let sample = Sample::load(&z.file).unwrap_or_else(|e| {
                                panic!("sampler socket {}: {}", socket.id, e)
                            });
                            Zone {
                                sample,
                                root: z.root,
                                low: z.low,
                                high: z.high,
                            }
                        })
                        .collect();
                    let sampler = Sampler::new(socket.id as usize, zones);
                    AudioNode::Source(Box::new(sampler))
                }
                SocketContent::Effect(EffectSpec::Gain { level }) => {
                    AudioNode::Processor(Box::new(Gain::new(level.build(sample_rate), sample_rate)))
                }
                SocketContent::Effect(EffectSpec::Reverb {
                    delay,
                    feedback,
                    mix,
                }) => AudioNode::Processor(Box::new(Reverb::new(
                    delay.build(sample_rate),
                    feedback.build(sample_rate),
                    mix.build(sample_rate),
                    sample_rate,
                ))),
                SocketContent::Effect(EffectSpec::Lowpass { cutoff }) => {
                    AudioNode::Processor(Box::new(Lowpass::new(
                        cutoff.build(sample_rate),
                        sample_rate,
                    )))
                }
                SocketContent::Effect(EffectSpec::Highpass { cutoff }) => {
                    AudioNode::Processor(Box::new(Highpass::new(
                        cutoff.build(sample_rate),
                        sample_rate,
                    )))
                }
                SocketContent::Effect(EffectSpec::Distortion { drive, mix }) => {
                    AudioNode::Processor(Box::new(Distortion::new(
                        drive.build(sample_rate),
                        mix.build(sample_rate),
                        sample_rate,
                    )))
                }
                SocketContent::Effect(EffectSpec::Tremolo { rate, depth }) => {
                    AudioNode::Processor(Box::new(Tremolo::new(
                        rate.build(sample_rate),
                        depth.build(sample_rate),
                        sample_rate,
                    )))
                }
                SocketContent::Effect(EffectSpec::Bitcrusher { bits }) => {
                    AudioNode::Processor(Box::new(Bitcrusher::new(
                        bits.build(sample_rate),
                        sample_rate,
                    )))
                }
                SocketContent::Effect(EffectSpec::Pan { amount }) => {
                    AudioNode::Processor(Box::new(Pan::new(amount.build(sample_rate), sample_rate)))
                }
                SocketContent::Effect(EffectSpec::Delay {
                    time_l,
                    time_r,
                    feedback,
                    mix,
                    ping_pong,
                    glide,
                }) => {
                    let (time_l, init_l) = time_l.build(bpm, sample_rate);
                    let (time_r, init_r) = time_r.build(bpm, sample_rate);
                    AudioNode::Processor(Box::new(Delay::new(
                        time_l,
                        time_r,
                        feedback.build(sample_rate),
                        mix.build(sample_rate),
                        ping_pong,
                        glide,
                        init_l,
                        init_r,
                        sample_rate,
                    )))
                }
                #[cfg(feature = "clap-host")]
                SocketContent::Plugin {
                    path,
                    plugin_id,
                    is_instrument,
                    params,
                } => {
                    match crate::clap::PluginNode::load(
                        socket.id as usize,
                        std::path::Path::new(&path),
                        &plugin_id,
                        sample_rate,
                    ) {
                        Ok(mut node) => {
                            // Record the param name→id map for binding resolution.
                            let mut name_to_id = HashMap::new();
                            for p in node.params() {
                                name_to_id.insert(p.name.to_lowercase(), p.id);
                            }
                            plugin_param_ids.insert(socket.id, name_to_id);

                            // `AURIO_DUMP_PARAMS=1` lists every param the plugin
                            // exposes, so bindings can be matched to real names.
                            if std::env::var("AURIO_DUMP_PARAMS").is_ok() {
                                println!(
                                    "[plugin] socket {}: '{}' exposes {} params:",
                                    socket.id,
                                    node.name(),
                                    node.params().len()
                                );
                                for p in node.params() {
                                    println!(
                                        "    \"{}\"  [{:.3} .. {:.3}, default {:.3}]",
                                        p.name, p.min, p.max, p.default
                                    );
                                }
                            }

                            // Apply declared initial param values by name.
                            for (name, value) in &params {
                                if let Some(pid) = node.param_id_by_name(name) {
                                    if is_instrument {
                                        crate::instrument::Instrument::set_param_by_id(
                                            &mut node, pid, *value,
                                        );
                                    } else {
                                        crate::effect::Effect::set_param_by_id(
                                            &mut node, pid, *value,
                                        );
                                    }
                                } else {
                                    eprintln!(
                                        "[plugin] socket {}: '{}' has no param named '{}'",
                                        socket.id,
                                        node.name(),
                                        name
                                    );
                                }
                            }

                            println!("[plugin] loaded '{}' for socket {}", node.name(), socket.id);
                            if is_instrument {
                                AudioNode::Source(Box::new(node))
                            } else {
                                AudioNode::Processor(Box::new(node))
                            }
                        }
                        Err(e) => {
                            // Skip-and-warn: keep the rest of the song playing.
                            // Instruments become silence; effects become a
                            // transparent pass-through.
                            eprintln!(
                                "[plugin] socket {}: failed to load '{}' ({}): {}",
                                socket.id, path, plugin_id, e
                            );
                            if is_instrument {
                                AudioNode::Source(Box::new(SilentSource::new(socket.id as usize)))
                            } else {
                                AudioNode::Processor(Box::new(Passthrough))
                            }
                        }
                    }
                }
                #[cfg(feature = "lv2-host")]
                SocketContent::Lv2 {
                    path,
                    plugin_uri,
                    is_instrument,
                    params,
                } => {
                    match crate::lv2::Lv2Node::load(
                        socket.id as usize,
                        std::path::Path::new(&path),
                        &plugin_uri,
                        sample_rate,
                    ) {
                        Ok(mut node) => {
                            // Record param name/symbol → id map for bindings.
                            let mut name_to_id = HashMap::new();
                            for p in node.params() {
                                name_to_id.insert(p.name.to_lowercase(), p.id);
                                name_to_id.insert(p.symbol.to_lowercase(), p.id);
                            }
                            plugin_param_ids.insert(socket.id, name_to_id);

                            if std::env::var("AURIO_DUMP_PARAMS").is_ok() {
                                let ps = node.params();
                                println!(
                                    "[lv2] socket {}: '{}' exposes {} control params:",
                                    socket.id, plugin_uri, ps.len()
                                );
                                for p in &ps {
                                    println!(
                                        "    \"{}\" (symbol \"{}\")  [{:.3} .. {:.3}, default {:.3}]",
                                        p.name, p.symbol, p.min, p.max, p.default
                                    );
                                }
                            }

                            // Apply declared initial values by name or symbol.
                            for (name, value) in &params {
                                if let Some(pid) = node.param_id_by_name(name) {
                                    if is_instrument {
                                        crate::instrument::Instrument::set_param_by_id(
                                            &mut node, pid, *value,
                                        );
                                    } else {
                                        crate::effect::Effect::set_param_by_id(
                                            &mut node, pid, *value,
                                        );
                                    }
                                } else {
                                    eprintln!(
                                        "[lv2] socket {}: no control param named '{}'",
                                        socket.id, name
                                    );
                                }
                            }

                            println!("[lv2] loaded '{}' for socket {}", plugin_uri, socket.id);
                            if is_instrument {
                                AudioNode::Source(Box::new(node))
                            } else {
                                AudioNode::Processor(Box::new(node))
                            }
                        }
                        Err(e) => {
                            eprintln!(
                                "[lv2] socket {}: failed to load '{}' ({}): {}",
                                socket.id, path, plugin_uri, e
                            );
                            if is_instrument {
                                AudioNode::Source(Box::new(SilentSource::new(socket.id as usize)))
                            } else {
                                AudioNode::Processor(Box::new(Passthrough))
                            }
                        }
                    }
                }
                SocketContent::Output => AudioNode::Output,
            };
            let idx = audio_nodes.len();
            audio_nodes.push(node);
            socket_to_node.insert(socket.id, idx);
        }

        let resolve_socket_id = |r: &SocketRef| -> u32 {
            match r {
                SocketRef::Id(i) => *i,
                SocketRef::Label(name) => *labels
                    .get(name)
                    .unwrap_or_else(|| panic!("unknown label: {}", name)),
            }
        };

        let resolve_node = |r: &SocketRef| -> usize {
            let id = resolve_socket_id(r);
            *socket_to_node
                .get(&id)
                .unwrap_or_else(|| panic!("unknown socket id: {}", id))
        };

        let audio_edges: Vec<(usize, usize)> = connections
            .iter()
            .map(|(f, t)| (resolve_node(f), resolve_node(t)))
            .collect();

        let mut audio_graph =
            AudioGraph::new(audio_nodes, audio_edges).expect("Failed to build audio graph");

        let mut state_graphs: Vec<StateGraph> = Vec::with_capacity(tracks.len());
        for track in tracks {
            let track_targets: Vec<usize> = track
                .targets
                .iter()
                .map(|r| resolve_socket_id(r) as usize)
                .collect();

            let mut phrase_indices: HashMap<String, usize> = HashMap::new();
            let mut phrase_entries: Vec<PhraseEntry> = Vec::new();

            let mut ensure = |name: &str| -> Result<usize, String> {
                if let Some(&idx) = phrase_indices.get(name) {
                    return Ok(idx);
                }
                let spec = phrases
                    .get(name)
                    .ok_or_else(|| format!("unknown phrase: {}", name))?;
                let entry = build_phrase_entry(name, spec, lua.lua())
                    .map_err(|e| format!("building phrase '{}': {}", name, e))?;
                let idx = phrase_entries.len();
                phrase_entries.push(entry);
                phrase_indices.insert(name.to_string(), idx);
                Ok(idx)
            };

            let initial_idx = ensure(&track.initial).expect("track initial phrase");

            let mut runtime_transitions = Vec::new();
            for ts in track.transitions {
                let from_idx = ensure(&ts.from).expect("transition from phrase");
                let to_idx = ensure(&ts.to).expect("transition to phrase");
                runtime_transitions.push(Transition::new(from_idx, to_idx, ts.condition));
            }

            state_graphs.push(StateGraph::new(
                track_targets,
                phrase_entries,
                runtime_transitions,
                initial_idx,
            ));
        }

        // Resolve every pending MIDI binding now that node indices are known.
        let resolved_bindings: Vec<ResolvedBinding> = pending_bindings
            .into_iter()
            .filter_map(|b| {
                socket_to_node
                    .get(&b.socket_id)
                    .map(|&node_idx| ResolvedBinding {
                        cc: b.cc,
                        channel: b.channel,
                        range: b.range,
                        node_idx,
                        param_idx: b.param_idx,
                    })
            })
            .collect();

        // Resolve plugin param bindings: map each param *name* to its CLAP id
        // using the map captured at load. A name that doesn't match a real param
        // (or a plugin that failed to load) is warned about and skipped.
        #[cfg(any(feature = "clap-host", feature = "lv2-host"))]
        let resolved_plugin_bindings: Vec<ResolvedPluginBinding> = pending_plugin_bindings
            .into_iter()
            .filter_map(|b| {
                let &node_idx = socket_to_node.get(&b.socket_id)?;
                let param_id = plugin_param_ids
                    .get(&b.socket_id)
                    .and_then(|m| m.get(&b.param_name.to_lowercase()).copied());
                match param_id {
                    Some(param_id) => Some(ResolvedPluginBinding {
                        cc: b.cc,
                        channel: b.channel,
                        range: b.range,
                        node_idx,
                        param_id,
                    }),
                    None => {
                        eprintln!(
                            "[plugin] socket {}: midi binding references unknown param '{}'",
                            b.socket_id, b.param_name
                        );
                        None
                    }
                }
            })
            .collect();

        // Resolve plugin param modulators the same way (name -> id), wrapping a
        // fresh ModState. These are evaluated per block in the audio callback.
        #[cfg(any(feature = "clap-host", feature = "lv2-host"))]
        let mut resolved_plugin_mods: Vec<ResolvedPluginMod> = pending_plugin_mods
            .into_iter()
            .filter_map(|m| {
                let &node_idx = socket_to_node.get(&m.socket_id)?;
                let param_id = plugin_param_ids
                    .get(&m.socket_id)
                    .and_then(|map| map.get(&m.param_name.to_lowercase()).copied());
                match param_id {
                    Some(param_id) => Some(ResolvedPluginMod {
                        node_idx,
                        param_id,
                        state: crate::param::ModState::new(m.modulator),
                    }),
                    None => {
                        eprintln!(
                            "[plugin] socket {}: modulator references unknown param '{}'",
                            m.socket_id, m.param_name
                        );
                        None
                    }
                }
            })
            .collect();

        // Two ring buffers feed the audio thread:
        //   notes_rb  — produced by the timing thread
        //   params_rb — produced by the MIDI input thread (if any bindings exist)
        // Both are SPSC; consumed in order by the audio callback per frame.
        let notes_rb = HeapRb::<ScheduledEvent>::new(4096);
        let (notes_producer, mut notes_consumer) = notes_rb.split();

        let params_rb = HeapRb::<ScheduledEvent>::new(256);
        let (params_producer, mut params_consumer) = params_rb.split();

        let sample_counter = Arc::new(AtomicU64::new(0));
        let audio_counter = sample_counter.clone();
        let timing_counter = sample_counter.clone();

        std::thread::spawn(move || {
            run_timing_thread(
                &lua,
                state_graphs,
                notes_producer,
                timing_counter,
                bpm,
                sample_rate,
            );
        });

        #[cfg(not(any(feature = "clap-host", feature = "lv2-host")))]
        let has_plugin_bindings = false;
        #[cfg(any(feature = "clap-host", feature = "lv2-host"))]
        let has_plugin_bindings = !resolved_plugin_bindings.is_empty();

        if !resolved_bindings.is_empty() || has_plugin_bindings {
            spawn_midi_thread(
                resolved_bindings,
                #[cfg(any(feature = "clap-host", feature = "lv2-host"))]
                resolved_plugin_bindings,
                params_producer,
                sample_counter.clone(),
            );
        }

        let cpal_config: cpal::StreamConfig = stream_config.into();

        // Stereo scratch the graph renders into, one slot per device frame.
        // Pre-allocated to MAX_BLOCK so the callback never allocates; longer
        // device buffers are processed in MAX_BLOCK-sized chunks.
        let mut block_scratch = vec![Frame::SILENCE; MAX_BLOCK];

        let stream = device
            .build_output_stream(
                &cpal_config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let current = audio_counter.load(Ordering::Relaxed);
                    let num_frames = data.len() / channels;

                    // `pos` is the frame offset within this callback we've
                    // rendered up to. We process variable-length segments, each
                    // bounded by the next pending event and by MAX_BLOCK, so
                    // events land sample-accurately at a block boundary.
                    let mut pos = 0usize;
                    while pos < num_frames {
                        let seg_start_sample = current + pos as u64;

                        // Apply every event due at or before this segment's first
                        // sample. Draining here (rather than mid-segment) is what
                        // pins each event to a block boundary.
                        while notes_consumer
                            .first()
                            .is_some_and(|e| e.sample <= seg_start_sample)
                        {
                            let event = notes_consumer.try_pop().unwrap();
                            audio_graph.process_event(&event);
                        }
                        while params_consumer
                            .first()
                            .is_some_and(|e| e.sample <= seg_start_sample)
                        {
                            let event = params_consumer.try_pop().unwrap();
                            audio_graph.process_event(&event);
                        }

                        // Segment length: up to the next event, capped at the
                        // remaining frames and MAX_BLOCK.
                        let remaining = num_frames - pos;
                        let until_event = next_event_sample(&notes_consumer, &params_consumer)
                            .map(|s| s.saturating_sub(seg_start_sample) as usize)
                            .filter(|&d| d > 0)
                            .unwrap_or(remaining);
                        // `mut` is used only when a plugin-host feature caps the
                        // block below for smooth modulation.
                        #[cfg_attr(
                            not(any(feature = "clap-host", feature = "lv2-host")),
                            allow(unused_mut)
                        )]
                        let mut seg_len = until_event.min(remaining).min(MAX_BLOCK);

                        // Plugin params can only change once per block (the plugin
                        // reads them once per run()). To keep modulator-driven
                        // params smooth even on large device buffers, cap the
                        // block so the LFO is re-evaluated often enough.
                        #[cfg(any(feature = "clap-host", feature = "lv2-host"))]
                        if !resolved_plugin_mods.is_empty() {
                            const MOD_MAX_BLOCK: usize = 256; // ~188 Hz at 48 kHz
                            seg_len = seg_len.min(MOD_MAX_BLOCK);
                        }

                        // Evaluate each plugin modulator once for this block and
                        // push the value to its node before rendering.
                        #[cfg(any(feature = "clap-host", feature = "lv2-host"))]
                        for m in resolved_plugin_mods.iter_mut() {
                            let value = m.state.next_block(sample_rate, seg_len);
                            audio_graph.set_plugin_param(m.node_idx, m.param_id, value);
                        }

                        let seg = &mut block_scratch[..seg_len];
                        audio_graph.process_block(seg, sample_rate);

                        // Interleave the segment onto the device's channels:
                        // mono device → downmix; otherwise even channels get L,
                        // odd channels get R.
                        for (i, out) in seg.iter().enumerate() {
                            let base = (pos + i) * channels;
                            if channels == 1 {
                                data[base] = (out.l + out.r) * 0.5;
                            } else {
                                for ch in 0..channels {
                                    data[base + ch] = if ch % 2 == 0 { out.l } else { out.r };
                                }
                            }
                        }

                        pos += seg_len;
                    }

                    audio_counter.fetch_add(num_frames as u64, Ordering::Relaxed);
                },
                |err| eprintln!("Audio error: {}", err),
                None,
            )
            .unwrap();

        stream.play().unwrap();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

/// Fallback nodes used when a CLAP plugin fails to load, so the rest of the
/// song still plays: an instrument becomes silence, an effect becomes a
/// transparent pass-through.
#[cfg(any(feature = "clap-host", feature = "lv2-host"))]
struct SilentSource {
    id: usize,
}
#[cfg(any(feature = "clap-host", feature = "lv2-host"))]
impl SilentSource {
    fn new(id: usize) -> Self {
        Self { id }
    }
}
#[cfg(any(feature = "clap-host", feature = "lv2-host"))]
impl crate::instrument::Instrument for SilentSource {
    fn id(&self) -> usize {
        self.id
    }
    fn process_event(&mut self, _event: &crate::midi::NoteEvent) {}
    fn next_sample(&mut self, _sample_rate: f32) -> Frame {
        Frame::SILENCE
    }
}

#[cfg(any(feature = "clap-host", feature = "lv2-host"))]
struct Passthrough;
#[cfg(any(feature = "clap-host", feature = "lv2-host"))]
impl crate::effect::Effect for Passthrough {
    fn process(&mut self, frame: Frame) -> Frame {
        frame
    }
}

/// Earliest `sample` among the front events of the notes and params consumers,
/// without popping either. Used to bound the next render segment so an event
/// never fires mid-block. `None` when both queues are empty.
fn next_event_sample<N, P>(notes: &N, params: &P) -> Option<u64>
where
    N: Consumer<Item = ScheduledEvent>,
    P: Consumer<Item = ScheduledEvent>,
{
    let n = notes.first().map(|e| e.sample);
    let p = params.first().map(|e| e.sample);
    match (n, p) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// Spawn a thread that opens the first available MIDI input port and pushes
/// `SetParam` events into the audio thread's params ringbuf whenever a
/// matching CC message arrives. The thread parks forever to keep the
/// `MidiInputConnection` (and its callback closure) alive.
fn spawn_midi_thread(
    bindings: Vec<ResolvedBinding>,
    #[cfg(any(feature = "clap-host", feature = "lv2-host"))] plugin_bindings: Vec<ResolvedPluginBinding>,
    mut producer: HeapProd<ScheduledEvent>,
    audio_counter: Arc<AtomicU64>,
) {
    std::thread::spawn(move || {
        let midi_in = match midir::MidiInput::new("aurio") {
            Ok(m) => m,
            Err(e) => {
                eprintln!("[midi] init failed: {}", e);
                return;
            }
        };

        let ports = midi_in.ports();
        if ports.is_empty() {
            eprintln!("[midi] no input ports available");
            return;
        }

        let port = ports[0].clone();
        let port_name = midi_in
            .port_name(&port)
            .unwrap_or_else(|_| "?".to_string());
        println!("[midi] listening on: {}", port_name);

        let _conn = match midi_in.connect(
            &port,
            "aurio-input",
            move |_ts, msg, _| {
                // Control Change: status byte 0xBn where n is the channel
                if msg.len() >= 3 && (msg[0] & 0xF0) == 0xB0 {
                    let channel = msg[0] & 0x0F;
                    let cc = msg[1];
                    let raw = msg[2] as f32 / 127.0;

                    for b in &bindings {
                        if b.cc == cc && b.channel.is_none_or(|c| c == channel) {
                            let value = b.range.0 + raw * (b.range.1 - b.range.0);
                            let event = ScheduledEvent::set_param(
                                audio_counter.load(Ordering::Relaxed),
                                b.node_idx,
                                b.param_idx,
                                value,
                            );
                            // Drop on overflow — knob noise is fine to lose.
                            let _ = producer.try_push(event);
                        }
                    }

                    #[cfg(any(feature = "clap-host", feature = "lv2-host"))]
                    for b in &plugin_bindings {
                        if b.cc == cc && b.channel.is_none_or(|c| c == channel) {
                            let value = b.range.0 + raw * (b.range.1 - b.range.0);
                            let event = ScheduledEvent::set_plugin_param(
                                audio_counter.load(Ordering::Relaxed),
                                b.node_idx,
                                b.param_id,
                                value,
                            );
                            let _ = producer.try_push(event);
                        }
                    }
                }
            },
            (),
        ) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[midi] connect failed: {}", e);
                return;
            }
        };

        // Park forever to keep the connection (and its callback) alive.
        std::thread::park();
    });
}

fn build_phrase_entry(
    name: &str,
    spec: &PhraseSpec,
    lua: &Lua,
) -> Result<PhraseEntry, mlua::Error> {
    let phrase: Box<dyn Phrase + Send> = match &spec.notes_source {
        NotesSourceSpec::Static(notes) => Box::new(StaticPhrase::new(
            name.to_string(),
            notes.clone(),
            spec.duration_beats,
        )),
        NotesSourceSpec::Dynamic(key) => {
            let new_key = clone_registry_key(lua, key)?;
            Box::new(PhraseScript::new(
                name.to_string(),
                spec.duration_beats,
                new_key,
            ))
        }
    };

    let hooks = PhraseHooks {
        on_enter: clone_optional_key(lua, &spec.on_enter)?,
        on_loop: clone_optional_key(lua, &spec.on_loop)?,
        on_exit: clone_optional_key(lua, &spec.on_exit)?,
    };

    Ok(PhraseEntry { phrase, hooks })
}

fn clone_registry_key(lua: &Lua, key: &RegistryKey) -> Result<RegistryKey, mlua::Error> {
    let f: Function = lua.registry_value(key)?;
    lua.create_registry_value(f)
}

fn clone_optional_key(
    lua: &Lua,
    key: &Option<RegistryKey>,
) -> Result<Option<RegistryKey>, mlua::Error> {
    match key {
        Some(k) => Ok(Some(clone_registry_key(lua, k)?)),
        None => Ok(None),
    }
}
