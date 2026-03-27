use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use crate::{
    midi::{ScheduledEvent, schedule_phrase},
    phrase::Phrase,
    scripting::LuaRuntime,
    transition::Transition,
};
use ringbuf::HeapProd;

pub struct StateGraph {
    phrases: Vec<Box<dyn Phrase + Send>>,
    transitions: Vec<Transition>,
    initial: usize,
}

impl StateGraph {
    pub fn new(
        phrases: Vec<Box<dyn Phrase + Send>>,
        transitions: Vec<Transition>,
        initial: usize,
    ) -> Self {
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

    pub fn phrase(&self, id: usize) -> &dyn Phrase {
        &*self.phrases[id]
    }

    pub fn next_phrase(&self, current: usize, lua: &LuaRuntime) -> usize {
        for transition in &self.transitions {
            if transition.from == current {
                match lua.eval_condition(&transition.condition) {
                    Ok(true) => return transition.to,
                    Ok(false) => continue,
                    Err(err) => {
                        eprintln!(
                            "[lua error] edge {} -> {}: {}",
                            transition.from, transition.to, err
                        );
                        continue;
                    }
                }
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
    let lua = LuaRuntime::new();
    let mut current_phrase = graph.initial();

    let phrase = graph.phrase(current_phrase);
    let notes = phrase.notes(&lua);
    let mut phrase_end = schedule_phrase(
        &notes,
        phrase.duration_beats(),
        bpm,
        sample_rate,
        0,
        &mut producer,
    );

    println!("[timing] Starting with phrase: {}", phrase.name());

    loop {
        let current_sample = sample_counter.load(Ordering::Relaxed);
        if current_sample < phrase_end {
            continue;
        }
        let next = graph.next_phrase(current_phrase, &lua);
        let phrase = graph.phrase(next);

        println!("[timing] Transition to {}", phrase.name());

        current_phrase = next;
        let notes = phrase.notes(&lua);

        phrase_end = schedule_phrase(
            &notes,
            phrase.duration_beats(),
            bpm,
            sample_rate,
            phrase_end,
            &mut producer,
        );
    }
}
