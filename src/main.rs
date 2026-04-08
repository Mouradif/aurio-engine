use aurio::audio_graph::{AudioGraph, AudioNode};
use aurio::effect::{Gain, Reverb};
use aurio::midi::{MidiNote, ScheduledEvent};
use aurio::oscillator::{Oscillator, Wave};
use aurio::phrase::Phrase;
use aurio::phrase_script::PhraseScript;
use aurio::scripting::LuaRuntime;
use aurio::state_graph::{StateGraph, run_timing_thread};
use aurio::static_phrase::StaticPhrase;
use aurio::synth::{ADSR, Synth};
use aurio::transition::Transition;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Split};
use std::process::exit;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

fn forest_piano() -> Vec<Box<dyn Phrase + Send>> {
    let pitches = [79, 84, 86, 91];

    let notes: Vec<MidiNote> = pitches
        .iter()
        .cycle()
        .take(32)
        .enumerate()
        .map(|(i, &pitch)| MidiNote {
            pitch,
            velocity: 100,
            start_beat: (i as f32) / 2.0,
            duration_beats: 0.45,
        })
        .collect();

    vec![
        Box::new(StaticPhrase::new("Piano A".to_owned(), notes, 16.0)),
        Box::new(StaticPhrase::from_rtttl(
            "Piano B".to_owned(),
            r#"
                8a3 8c3 8e3 8f3
                8a3 8c3 8e3 8f3
                8a3 8c3 8e3 8f3
                8a3 8c3 8e3 8f3
                8a3 8c3 8e3 8f3
                8a3 8c3 8e3 8f3
                8a3 8c3 8e3 8f3
                8a3 8c3 8e3 8f3
            "#,
        )),
    ]
}

fn forest_bass(lua: &LuaRuntime) -> Vec<Box<dyn Phrase + Send>> {
    vec![
        Box::new(StaticPhrase::from_lua(
            lua,
            "Bass A".to_owned(),
            r#"
                return {
                    { pitch = 51, velocity = 100, start_beat = 0, duration_beats = 7.5 },
                    { pitch = 58, velocity = 100, start_beat = 7.5, duration_beats = 0.5 },
                    { pitch = 51, velocity = 100, start_beat = 8.0, duration_beats = 6.0 },
                    { pitch = 63, velocity = 100, start_beat = 14.0, duration_beats = 2.0 },
                }
            "#
            .to_owned(),
            16.0,
        )),
        Box::new(PhraseScript::new(
            "Bass B".to_owned(),
            r#"
                return {
                    { pitch = 62, velocity = 100, start_beat = 0, duration_beats = 7.5 },
                    { pitch = 57, velocity = 100, start_beat = 7.5, duration_beats = 0.5 },
                    { pitch = 50, velocity = 100, start_beat = 8.0, duration_beats = 6.0 },
                    { pitch = 50, velocity = 100, start_beat = 14.5, duration_beats = 0.5 },
                }
            "#
            .to_owned(),
            16.0,
        )),
    ]
}

fn main() {
    let lua = LuaRuntime::new();
    let bpm = 120.0;
    let piano_melody = forest_piano();
    let bass_melody = forest_bass(&lua);
    let piano_transitions = vec![Transition::always(0, 1), Transition::always(1, 0)];
    let bass_transitions = piano_transitions.clone();
    let piano_track = StateGraph::new(vec![0], piano_melody, piano_transitions, 0);
    let bass_track = StateGraph::new(vec![1], bass_melody, bass_transitions, 0);

    // Open audio device
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("No audio output device found");
    let config = device.default_output_config().unwrap();
    let sample_rate = config.sample_rate() as f32;
    let channels = config.channels() as usize;

    println!("Audio: {} Hz, {} channels", sample_rate, channels);
    println!("Playing melody at {} BPM...", bpm);

    let synth = Synth::new(
        0,
        ADSR {
            a: 0.025,
            d: 0.1,
            s: 1.0,
            r: 0.2,
        },
        vec![
            (Oscillator::new(Wave::Sine), 0.4),
            (Oscillator::new(Wave::Triangle), 0.2),
            (Oscillator::new(Wave::Saw), 0.1),
        ],
    );
    let bass = Synth::new(
        1,
        ADSR {
            a: 0.0,
            d: 0.1,
            s: 0.7,
            r: 0.05,
        },
        vec![(Oscillator::new(Wave::Sine), 1.0)],
    );
    let reverb = Reverb::new(0.5, 0.5, 0.8, sample_rate);
    let piano_gain = Gain::new(0.3);
    let bass_gain = Gain::new(0.6);

    let mut audio_graph = AudioGraph::new(
        vec![
            AudioNode::Source(Box::new(synth)),
            AudioNode::Source(Box::new(bass)),
            AudioNode::Processor(Box::new(reverb)),
            AudioNode::Processor(Box::new(piano_gain)),
            AudioNode::Processor(Box::new(bass_gain)),
            AudioNode::Output,
        ],
        vec![(0, 2), (2, 3), (3, 5), (1, 4), (4, 5)],
    )
    .unwrap_or_else(|err| {
        eprintln!("Error instanciating audio graph: {}", err);
        exit(1);
    });

    let ring_buffer = HeapRb::<ScheduledEvent>::new(4096);
    let (producer, mut consumer) = ring_buffer.split();

    // Shared state for the audio callback
    let sample_counter = Arc::new(AtomicU64::new(0));
    let audio_counter = sample_counter.clone();
    let timing_counter = sample_counter.clone();

    std::thread::spawn(move || {
        run_timing_thread(
            &lua,
            vec![piano_track, bass_track],
            producer,
            timing_counter,
            bpm,
            sample_rate,
        );
    });

    let stream_config: cpal::StreamConfig = config.into();

    let stream = device
        .build_output_stream(
            &stream_config,
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
