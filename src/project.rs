use crate::audio_graph::{AudioGraph, AudioNode};
use crate::effect::{Effect, Gain, Reverb};
use crate::midi::{MidiNote, ScheduledEvent};
use crate::oscillator::{Oscillator, Wave};
use crate::phrase::Phrase;
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
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Serialize, Deserialize)]
pub struct Song {
    pub bpm: f32,
    pub instruments: Vec<InstrumentConfig>,
    pub tracks: Vec<TrackConfig>,
}

#[derive(Serialize, Deserialize)]
pub struct InstrumentConfig {
    pub id: usize,
    pub adsr: ADSR,
    pub oscillators: Vec<OscillatorConfig>,
    #[serde(default)]
    pub effects: Vec<EffectConfig>,
}

#[derive(Serialize, Deserialize)]
pub struct OscillatorConfig {
    pub wave: Wave,
    pub gain: f32,
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
    },
    Rtttl {
        name: String,
        source: String,
    },
    LuaOnce {
        name: String,
        duration_beats: f32,
        code: String,
    },
    Lua {
        name: String,
        duration_beats: f32,
        code: String,
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

        let mut audio_nodes: Vec<AudioNode> = vec![AudioNode::Output];
        let output_idx = 0usize;
        let mut audio_edges: Vec<(usize, usize)> = Vec::new();

        for instrument in self.instruments {
            let oscillators: Vec<(Oscillator, f32)> = instrument
                .oscillators
                .into_iter()
                .map(|o| (Oscillator::new(o.wave), o.gain))
                .collect();
            let synth = Synth::new(instrument.id, instrument.adsr, oscillators);
            let synth_idx = audio_nodes.len();
            audio_nodes.push(AudioNode::Source(Box::new(synth)));

            let mut prev = synth_idx;
            for effect_cfg in instrument.effects {
                let effect: Box<dyn Effect + Send> = match effect_cfg {
                    EffectConfig::Gain { level } => Box::new(Gain::new(level)),
                    EffectConfig::Reverb {
                        delay_seconds,
                        feedback,
                        mix,
                    } => Box::new(Reverb::new(delay_seconds, feedback, mix, sample_rate)),
                };
                let eff_idx = audio_nodes.len();
                audio_nodes.push(AudioNode::Processor(effect));
                audio_edges.push((prev, eff_idx));
                prev = eff_idx;
            }
            audio_edges.push((prev, output_idx));
        }

        let mut audio_graph = AudioGraph::new(audio_nodes, audio_edges)
            .expect("Failed to build audio graph");

        let mut state_graphs: Vec<StateGraph> = Vec::with_capacity(self.tracks.len());
        for track in self.tracks {
            let phrases: Vec<Box<dyn Phrase + Send>> = track
                .patterns
                .into_iter()
                .map(|p| build_phrase(p, &lua))
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

fn build_phrase(p: PatternConfig, lua: &LuaRuntime) -> Box<dyn Phrase + Send> {
    match p {
        PatternConfig::Notes {
            name,
            duration_beats,
            notes,
        } => {
            let notes: Vec<MidiNote> = notes.into_iter().map(Into::into).collect();
            Box::new(StaticPhrase::new(name, notes, duration_beats))
        }
        PatternConfig::Rtttl { name, source } => {
            Box::new(StaticPhrase::from_rtttl(name, &source))
        }
        PatternConfig::LuaOnce {
            name,
            duration_beats,
            code,
        } => Box::new(StaticPhrase::from_lua(lua, name, code, duration_beats)),
        PatternConfig::Lua {
            name,
            duration_beats,
            code,
        } => Box::new(PhraseScript::new(name, code, duration_beats)),
    }
}
