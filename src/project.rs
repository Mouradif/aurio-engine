use crate::audio_graph::{AudioGraph, AudioNode};
use crate::effect::{Effect, Gain, Reverb};
use crate::midi::{MidiNote, ScheduledEvent};
use crate::oscillator::{Oscillator, Wave};
use crate::phrase::{PhraseEntry, PhraseHooks};
use crate::phrase_script::PhraseScript;
use crate::scripting::LuaRuntime;
use crate::state_graph::{StateGraph, run_timing_thread};
use crate::static_phrase::StaticPhrase;
use crate::synth::{ADSR, Synth};
use crate::transition::Transition;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Split};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Serialize, Deserialize)]
pub struct Song {
    pub bpm: f32,
    pub instruments: Vec<InstrumentConfig>,
    #[serde(default)]
    pub effects: Vec<EffectEntry>,
    #[serde(default)]
    pub outputs: Vec<OutputEntry>,
    #[serde(default)]
    pub connections: Vec<Connection>,
    pub tracks: Vec<TrackConfig>,
}

#[derive(Serialize, Deserialize)]
pub struct OutputEntry {
    pub name: String,
}

#[derive(Serialize, Deserialize)]
pub struct InstrumentConfig {
    pub id: usize,
    pub adsr: ADSR,
    pub oscillators: Vec<OscillatorConfig>,
}

#[derive(Serialize, Deserialize)]
pub struct OscillatorConfig {
    pub wave: Wave,
    pub gain: f32,
}

#[derive(Serialize, Deserialize)]
pub struct EffectEntry {
    pub name: String,
    pub kind: EffectConfig,
}

#[derive(Serialize, Deserialize)]
pub enum EffectConfig {
    Gain {
        level: f32,
    },
    Reverb {
        delay_seconds: f32,
        feedback: f32,
        mix: f32,
    },
}

#[derive(Serialize, Deserialize)]
pub enum NodeRef {
    Instrument(usize),
    Effect(String),
    Output(String),
}

#[derive(Serialize, Deserialize)]
pub struct Connection {
    pub from: NodeRef,
    pub to: NodeRef,
}

#[derive(Serialize, Deserialize)]
pub struct TrackConfig {
    pub targets: Vec<usize>,
    #[serde(default)]
    pub initial: usize,
    pub patterns: Vec<PatternConfig>,
    #[serde(default)]
    pub transitions: Vec<TransitionConfig>,
}

#[derive(Serialize, Deserialize)]
pub enum PatternConfig {
    Notes {
        name: String,
        duration_beats: f32,
        notes: Vec<MidiNoteConfig>,
        #[serde(default)]
        on_enter: String,
        #[serde(default)]
        on_loop: String,
        #[serde(default)]
        on_exit: String,
    },
    Rtttl {
        name: String,
        source: String,
        #[serde(default)]
        on_enter: String,
        #[serde(default)]
        on_loop: String,
        #[serde(default)]
        on_exit: String,
    },
    LuaOnce {
        name: String,
        duration_beats: f32,
        code: String,
        #[serde(default)]
        on_enter: String,
        #[serde(default)]
        on_loop: String,
        #[serde(default)]
        on_exit: String,
    },
    Lua {
        name: String,
        duration_beats: f32,
        code: String,
        #[serde(default)]
        on_enter: String,
        #[serde(default)]
        on_loop: String,
        #[serde(default)]
        on_exit: String,
    },
}

#[derive(Serialize, Deserialize)]
pub struct MidiNoteConfig {
    pub pitch: u8,
    pub velocity: u8,
    pub start_beat: f32,
    pub duration_beats: f32,
}

impl From<MidiNoteConfig> for MidiNote {
    fn from(n: MidiNoteConfig) -> Self {
        Self {
            pitch: n.pitch,
            velocity: n.velocity,
            start_beat: n.start_beat,
            duration_beats: n.duration_beats,
        }
    }
}

fn default_condition() -> String {
    "true".to_string()
}

#[derive(Serialize, Deserialize)]
pub struct TransitionConfig {
    pub from: usize,
    pub to: usize,
    #[serde(default = "default_condition")]
    pub condition: String,
}

impl Song {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let contents = fs::read_to_string(path)?;
        let song: Song = ron::from_str(&contents)?;
        Ok(song)
    }

    pub fn play(self) -> ! {
        let lua = LuaRuntime::new();

        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .expect("No audio output device found");
        let stream_config = device.default_output_config().unwrap();
        let sample_rate = stream_config.sample_rate() as f32;
        let channels = stream_config.channels() as usize;

        println!("Audio: {} Hz, {} channels", sample_rate, channels);
        println!("Playing at {} BPM...", self.bpm);

        let mut audio_nodes: Vec<AudioNode> = Vec::new();
        let mut instrument_idx: HashMap<usize, usize> = HashMap::new();
        let mut effect_idx: HashMap<String, usize> = HashMap::new();

        for instrument in self.instruments {
            let oscillators: Vec<(Oscillator, f32)> = instrument
                .oscillators
                .into_iter()
                .map(|o| (Oscillator::new(o.wave), o.gain))
                .collect();
            let synth = Synth::new(instrument.id, instrument.adsr, oscillators);
            let idx = audio_nodes.len();
            audio_nodes.push(AudioNode::Source(Box::new(synth)));
            if instrument_idx.insert(instrument.id, idx).is_some() {
                panic!("Duplicate instrument id: {}", instrument.id);
            }
        }

        for entry in self.effects {
            let effect: Box<dyn Effect + Send> = match entry.kind {
                EffectConfig::Gain { level } => Box::new(Gain::new(level)),
                EffectConfig::Reverb {
                    delay_seconds,
                    feedback,
                    mix,
                } => Box::new(Reverb::new(delay_seconds, feedback, mix, sample_rate)),
            };
            let idx = audio_nodes.len();
            audio_nodes.push(AudioNode::Processor(effect));
            if effect_idx.insert(entry.name.clone(), idx).is_some() {
                panic!("Duplicate effect name: {}", entry.name);
            }
        }

        let mut output_idx: HashMap<String, usize> = HashMap::new();
        for entry in self.outputs {
            let idx = audio_nodes.len();
            audio_nodes.push(AudioNode::Output);
            if output_idx.insert(entry.name.clone(), idx).is_some() {
                panic!("Duplicate output name: {}", entry.name);
            }
        }

        let resolve = |r: &NodeRef| -> usize {
            match r {
                NodeRef::Instrument(id) => *instrument_idx
                    .get(id)
                    .unwrap_or_else(|| panic!("Unknown instrument id in routing: {}", id)),
                NodeRef::Effect(name) => *effect_idx
                    .get(name)
                    .unwrap_or_else(|| panic!("Unknown effect name in routing: {}", name)),
                NodeRef::Output(name) => *output_idx
                    .get(name)
                    .unwrap_or_else(|| panic!("Unknown output name in routing: {}", name)),
            }
        };

        let audio_edges: Vec<(usize, usize)> = self
            .connections
            .iter()
            .map(|c| (resolve(&c.from), resolve(&c.to)))
            .collect();

        let mut audio_graph = AudioGraph::new(audio_nodes, audio_edges)
            .expect("Failed to build audio graph");

        let mut state_graphs: Vec<StateGraph> = Vec::with_capacity(self.tracks.len());
        for track in self.tracks {
            let phrases: Vec<PhraseEntry> = track
                .patterns
                .into_iter()
                .map(|p| build_phrase_entry(p, &lua))
                .collect();
            let transitions: Vec<Transition> = track
                .transitions
                .into_iter()
                .map(|t| Transition::new(t.from, t.to, t.condition))
                .collect();
            state_graphs.push(StateGraph::new(
                track.targets,
                phrases,
                transitions,
                track.initial,
            ));
        }

        let ring_buffer = HeapRb::<ScheduledEvent>::new(4096);
        let (producer, mut consumer) = ring_buffer.split();

        let sample_counter = Arc::new(AtomicU64::new(0));
        let audio_counter = sample_counter.clone();
        let timing_counter = sample_counter.clone();
        let bpm = self.bpm;

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

                        while consumer.first().map_or(false, |e| e.sample <= frame_sample) {
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

fn build_phrase_entry(p: PatternConfig, lua: &LuaRuntime) -> PhraseEntry {
    match p {
        PatternConfig::Notes {
            name,
            duration_beats,
            notes,
            on_enter,
            on_loop,
            on_exit,
        } => {
            let notes: Vec<MidiNote> = notes.into_iter().map(Into::into).collect();
            PhraseEntry {
                phrase: Box::new(StaticPhrase::new(name, notes, duration_beats)),
                hooks: PhraseHooks {
                    on_enter,
                    on_loop,
                    on_exit,
                },
            }
        }
        PatternConfig::Rtttl {
            name,
            source,
            on_enter,
            on_loop,
            on_exit,
        } => PhraseEntry {
            phrase: Box::new(StaticPhrase::from_rtttl(name, &source)),
            hooks: PhraseHooks {
                on_enter,
                on_loop,
                on_exit,
            },
        },
        PatternConfig::LuaOnce {
            name,
            duration_beats,
            code,
            on_enter,
            on_loop,
            on_exit,
        } => PhraseEntry {
            phrase: Box::new(StaticPhrase::from_lua(lua, name, code, duration_beats)),
            hooks: PhraseHooks {
                on_enter,
                on_loop,
                on_exit,
            },
        },
        PatternConfig::Lua {
            name,
            duration_beats,
            code,
            on_enter,
            on_loop,
            on_exit,
        } => PhraseEntry {
            phrase: Box::new(PhraseScript::new(name, code, duration_beats)),
            hooks: PhraseHooks {
                on_enter,
                on_loop,
                on_exit,
            },
        },
    }
}
