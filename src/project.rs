use crate::audio_graph::{AudioGraph, AudioNode};
use crate::effect::{Bitcrusher, Distortion, Gain, Highpass, Lowpass, Reverb, Tremolo};
use crate::midi::{MidiNote, ScheduledEvent};
use crate::oscillator::{Oscillator, Wave};
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
}

struct SocketEntry {
    id: u32,
    content: SocketContent,
}

enum SocketContent {
    Instrument {
        adsr: ADSR,
        oscillators: Vec<(Wave, f32)>,
    },
    Sampler {
        zones: Vec<SamplerZoneSpec>,
    },
    Effect(EffectSpec),
    Output,
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
    Gain { level: f32 },
    Reverb { delay: f32, feedback: f32, mix: f32 },
    Lowpass { cutoff: f32 },
    Highpass { cutoff: f32 },
    Distortion { drive: f32, mix: f32 },
    Tremolo { rate: f32, depth: f32 },
    Bitcrusher { bits: f32 },
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

/// Pull a numeric param out of a Lua params table, or — if it's a
/// `LiveParam` userdata — record a pending MIDI binding and return its
/// initial value. The binding is later resolved to a node index in
/// `Song::play`.
fn extract_param(
    params: &Table,
    name: &str,
    socket_id: u32,
    param_idx: u8,
    builder: &mut SongBuilder,
) -> mlua::Result<f32> {
    let v: Value = params.get(name)?;
    match v {
        Value::Number(n) => Ok(n as f32),
        Value::Integer(i) => Ok(i as f32),
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
            Ok(live.initial)
        }
        Value::Nil => Err(mlua::Error::RuntimeError(format!(
            "missing param: '{}'",
            name
        ))),
        _ => Err(mlua::Error::RuntimeError(format!(
            "'{}' must be a number or a midi_connect handle",
            name
        ))),
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
                        let gain: f32 = osc.get("gain")?;
                        oscillators.push((wave, gain));
                    }
                    let label = label_from_opts(opts)?;
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
                move |_,
                      (_self, id, kind, params, opts): (
                    Value,
                    u32,
                    String,
                    Table,
                    Option<Table>,
                )| {
                    let label = label_from_opts(opts)?;
                    let mut guard = b.lock().unwrap();
                    let builder = guard.as_mut().unwrap();

                    let spec = match kind.as_str() {
                        "Gain" => {
                            let level =
                                extract_param(&params, "level", id, Gain::PARAM_LEVEL, builder)?;
                            EffectSpec::Gain { level }
                        }
                        "Reverb" => EffectSpec::Reverb {
                            delay: params.get("delay")?,
                            feedback: params.get("feedback")?,
                            mix: params.get("mix")?,
                        },
                        "Lowpass" => EffectSpec::Lowpass {
                            cutoff: params.get("cutoff")?,
                        },
                        "Highpass" => EffectSpec::Highpass {
                            cutoff: params.get("cutoff")?,
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
                            rate: params.get("rate")?,
                            depth: params.get("depth")?,
                        },
                        "Bitcrusher" => EffectSpec::Bitcrusher {
                            bits: params.get("bits")?,
                        },
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
        } = self;

        let mut audio_nodes: Vec<AudioNode> = Vec::new();
        let mut socket_to_node: HashMap<u32, usize> = HashMap::new();

        for socket in sockets {
            let node = match socket.content {
                SocketContent::Instrument { adsr, oscillators } => {
                    let oscillators: Vec<(Oscillator, f32)> = oscillators
                        .into_iter()
                        .map(|(wave, gain)| (Oscillator::new(wave), gain))
                        .collect();
                    let synth = Synth::new(socket.id as usize, adsr, oscillators);
                    AudioNode::Source(Box::new(synth))
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
                    AudioNode::Processor(Box::new(Gain::new(level, sample_rate)))
                }
                SocketContent::Effect(EffectSpec::Reverb {
                    delay,
                    feedback,
                    mix,
                }) => {
                    AudioNode::Processor(Box::new(Reverb::new(delay, feedback, mix, sample_rate)))
                }
                SocketContent::Effect(EffectSpec::Lowpass { cutoff }) => {
                    AudioNode::Processor(Box::new(Lowpass::new(cutoff, sample_rate)))
                }
                SocketContent::Effect(EffectSpec::Highpass { cutoff }) => {
                    AudioNode::Processor(Box::new(Highpass::new(cutoff, sample_rate)))
                }
                SocketContent::Effect(EffectSpec::Distortion { drive, mix }) => {
                    AudioNode::Processor(Box::new(Distortion::new(drive, mix, sample_rate)))
                }
                SocketContent::Effect(EffectSpec::Tremolo { rate, depth }) => {
                    AudioNode::Processor(Box::new(Tremolo::new(rate, depth, sample_rate)))
                }
                SocketContent::Effect(EffectSpec::Bitcrusher { bits }) => {
                    AudioNode::Processor(Box::new(Bitcrusher::new(bits)))
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

        if !resolved_bindings.is_empty() {
            spawn_midi_thread(resolved_bindings, params_producer, sample_counter.clone());
        }

        let cpal_config: cpal::StreamConfig = stream_config.into();

        let stream = device
            .build_output_stream(
                &cpal_config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let current = audio_counter.load(Ordering::Relaxed);
                    let num_frames = data.len() / channels;

                    for frame in 0..num_frames {
                        let frame_sample = current + frame as u64;

                        while notes_consumer
                            .first()
                            .is_some_and(|e| e.sample <= frame_sample)
                        {
                            let event = notes_consumer.try_pop().unwrap();
                            audio_graph.process_event(&event);
                        }
                        while params_consumer
                            .first()
                            .is_some_and(|e| e.sample <= frame_sample)
                        {
                            let event = params_consumer.try_pop().unwrap();
                            audio_graph.process_event(&event);
                        }

                        let sample = audio_graph.next_sample(sample_rate);

                        for ch in 0..channels {
                            data[frame * channels + ch] = sample;
                        }
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

/// Spawn a thread that opens the first available MIDI input port and pushes
/// `SetParam` events into the audio thread's params ringbuf whenever a
/// matching CC message arrives. The thread parks forever to keep the
/// `MidiInputConnection` (and its callback closure) alive.
fn spawn_midi_thread(
    bindings: Vec<ResolvedBinding>,
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
