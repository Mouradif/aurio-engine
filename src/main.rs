use aurio::audio_graph::{AudioGraph, AudioNode};
use aurio::effect::{Gain, Reverb};
use aurio::midi::{MidiNote, ScheduledEvent};
use aurio::oscillator::{Oscillator, Wave};
use aurio::state_graph::{Phrase, StateGraph, Transition, run_timing_thread};
use aurio::synth::{ADSR, Synth};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Split};
use std::process::exit;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

fn forest_melody() -> Vec<Phrase> {
    let pitches_a = [79, 84, 86, 91];
    let pitches_b = [81, 84, 88, 89];

    let notes_a: Vec<MidiNote> = pitches_a
        .iter()
        .cycle()
        .take(32)
        .enumerate()
        .map(|(i, &pitch)| MidiNote {
            pitch,
            velocity: 100,
            start_beat: (i as f32) / 4.0,
            duration_beats: 0.2,
        })
        .collect();
    let notes_b: Vec<MidiNote> = pitches_b
        .iter()
        .cycle()
        .take(32)
        .enumerate()
        .map(|(i, &pitch)| MidiNote {
            pitch,
            velocity: 100,
            start_beat: (i as f32) / 4.0,
            duration_beats: 0.2,
        })
        .collect();

    vec![
        Phrase::new("A".to_owned(), notes_a, 8.0),
        Phrase::new("B".to_owned(), notes_b, 8.0),
    ]
}

fn main() {
    let bpm = 120.0;
    let melody = forest_melody();
    let edges = vec![Transition::always(0, 1), Transition::always(1, 0)];
    let state_graph = StateGraph::new(melody, edges, 0);

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
        ADSR {
            a: 0.01,
            d: 0.1,
            s: 0.7,
            r: 0.2,
        },
        vec![
            (Oscillator::new(Wave::Sine), 0.4),
            (Oscillator::new(Wave::Triangle), 0.2),
            (Oscillator::new(Wave::Saw), 0.1),
        ],
    );
    let reverb = Reverb::new(0.5, 0.5, 0.8, sample_rate);
    let gain = Gain::new(0.1);

    let mut audio_graph = AudioGraph::new(
        vec![
            AudioNode::Source(Box::new(synth)),
            AudioNode::Processor(Box::new(reverb)),
            AudioNode::Processor(Box::new(gain)),
        ],
        vec![(0, 1), (1, 2)],
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
        run_timing_thread(state_graph, producer, timing_counter, bpm, sample_rate);
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
                        audio_graph.process_event(&event.event);
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
