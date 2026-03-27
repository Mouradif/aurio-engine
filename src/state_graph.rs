use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread::sleep,
    time::Duration,
};

use crate::midi::{MidiNote, ScheduledEvent, schedule_phrase};
use ringbuf::HeapProd;

#[derive(Debug)]
pub struct Phrase {
    pub name: String,
    pub duration_beats: f32,
    pub notes: Vec<MidiNote>,
}

impl Phrase {
    pub fn new(name: String, notes: Vec<MidiNote>, duration_beats: f32) -> Self {
        Self {
            name,
            duration_beats,
            notes,
        }
    }
}

pub struct Transition {
    pub from: usize,
    pub to: usize,
    pub condition: Box<dyn Fn() -> bool + Send>,
}

impl Transition {
    pub fn new(from: usize, to: usize, condition: Box<dyn Fn() -> bool + Send>) -> Self {
        Self {
            from,
            to,
            condition,
        }
    }

    pub fn always(from: usize, to: usize) -> Self {
        Self::new(from, to, Box::new(|| true))
    }
}

pub struct StateGraph {
    phrases: Vec<Phrase>,
    transitions: Vec<Transition>,
    initial: usize,
}

impl StateGraph {
    pub fn new(phrases: Vec<Phrase>, transitions: Vec<Transition>, initial: usize) -> Self {
        let max = phrases.len() - 1;
        Self {
            phrases,
            transitions,
            initial: initial.clamp(0, max),
        }
    }

    pub fn initial(&self) -> usize {
        self.initial
    }

    pub fn phrase(&self, id: usize) -> &Phrase {
        &self.phrases[id]
    }

    pub fn next_phrase(&self, current: usize) -> usize {
        for transition in &self.transitions {
            if transition.from == current && (transition.condition)() {
                return transition.to;
            }
        }
        current
    }
}

pub fn run_timing_thread(
    graph: StateGraph,
    mut producer: HeapProd<ScheduledEvent>,
    sample_counter: Arc<AtomicU64>,
    bpm: f32,
    sample_rate: f32,
) {
    let mut current_phrase = graph.initial();

    let phrase = graph.phrase(current_phrase);
    let mut phrase_end = schedule_phrase(
        &phrase.notes,
        phrase.duration_beats,
        bpm,
        sample_rate,
        0,
        &mut producer,
    );

    println!("[timing] Starting with phrase: {}", phrase.name);

    loop {
        sleep(Duration::from_millis(1));
        let current_sample = sample_counter.load(Ordering::Relaxed);
        if current_sample < phrase_end {
            continue;
        }
        let next = graph.next_phrase(current_phrase);
        let phrase = graph.phrase(next);

        println!("[timing] Transition to {}", phrase.name);

        current_phrase = next;

        phrase_end = schedule_phrase(
            &phrase.notes,
            phrase.duration_beats,
            bpm,
            sample_rate,
            phrase_end,
            &mut producer,
        );
    }
}
