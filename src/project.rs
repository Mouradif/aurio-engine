use crate::audio_graph::{AudioGraph, AudioNode};
use crate::effect::{Bitcrusher, Distortion, Gain, Highpass, Lowpass, Reverb, Tremolo};
use crate::midi::{MidiNote, ScheduledEvent};
use crate::oscillator::{Oscillator, Wave};
use crate::phrase::{Phrase, PhraseEntry, PhraseHooks};
use crate::phrase_script::PhraseScript;
use crate::rtttl::parse_rtttl;
use crate::scripting::{LuaRuntime, notes_from_table};
use crate::state_graph::{StateGraph, run_timing_thread};
use crate::static_phrase::StaticPhrase;
use crate::synth::{ADSR, Synth};
use crate::transition::Transition;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use mlua::{Function, Lua, RegistryKey, Table, Value};
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Split};
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
    Effect(EffectSpec),
    Output,
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

#[derive(Default)]
struct SongBuilder {
    bpm: Option<f32>,
    sockets: Vec<SocketEntry>,
    labels: HashMap<String, u32>,
    connections: Vec<(SocketRef, SocketRef)>,
    phrases: HashMap<String, PhraseSpec>,
    tracks: Vec<TrackSpec>,
    socket_ids: HashSet<u32>,
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
                    let spec = match kind.as_str() {
                        "Gain" => EffectSpec::Gain {
                            level: params.get("level")?,
                        },
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
                        "Distortion" => EffectSpec::Distortion {
                            drive: params.get("drive")?,
                            mix: params.get("mix")?,
                        },
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
                    let label = label_from_opts(opts)?;
                    b.lock().unwrap().as_mut().unwrap().add_socket(
                        id,
                        SocketContent::Effect(spec),
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
        let builder: SharedBuilder = Arc::new(Mutex::new(Some(SongBuilder::default())));
        let song_table = lua.lua().create_table()?;
        register_methods(lua.lua(), &song_table, &builder)?;
        lua.lua().globals().set("song", song_table)?;

        let code = std::fs::read_to_string(path.as_ref())?;
        let chunk_name = path.as_ref().display().to_string();

        // Make `require` resolve modules relative to the entry file's directory.
        let parent = path
            .as_ref()
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
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
                SocketContent::Effect(EffectSpec::Gain { level }) => {
                    AudioNode::Processor(Box::new(Gain::new(level)))
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
                    AudioNode::Processor(Box::new(Distortion::new(drive, mix)))
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

        let ring_buffer = HeapRb::<ScheduledEvent>::new(4096);
        let (producer, mut consumer) = ring_buffer.split();

        let sample_counter = Arc::new(AtomicU64::new(0));
        let audio_counter = sample_counter.clone();
        let timing_counter = sample_counter.clone();

        std::thread::spawn(move || {
            run_timing_thread(
                &lua,
                state_graphs,
                producer,
                timing_counter,
                bpm,
                sample_rate,
            );
        });

        let cpal_config: cpal::StreamConfig = stream_config.into();

        let stream = device
            .build_output_stream(
                &cpal_config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let current = audio_counter.load(Ordering::Relaxed);
                    let num_frames = data.len() / channels;

                    for frame in 0..num_frames {
                        let frame_sample = current + frame as u64;

                        while consumer.first().is_some_and(|e| e.sample <= frame_sample) {
                            let event = consumer.try_pop().unwrap();
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
