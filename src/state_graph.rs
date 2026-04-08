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
use ringbuf::{HeapProd, traits::Producer};

pub struct StateGraph {
    targets: Vec<usize>,
    phrases: Vec<Box<dyn Phrase + Send>>,
    transitions: Vec<Transition>,
    initial: usize,
}

impl StateGraph {
    pub fn new(
        targets: Vec<usize>,
        phrases: Vec<Box<dyn Phrase + Send>>,
        transitions: Vec<Transition>,
        initial: usize,
    ) -> Self {
        let max = phrases.len() - 1;
        Self {
            targets,
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
    lua: &LuaRuntime,
    graphs: Vec<StateGraph>,
    mut producer: HeapProd<ScheduledEvent>,
    sample_counter: Arc<AtomicU64>,
    bpm: f32,
    sample_rate: f32,
) {
    let tracks = graphs.len();
    let mut current_phrases: Vec<usize> = vec![0; tracks];
    let mut phrase_ends: Vec<u64> = vec![0; tracks];
    let mut pending_events: Vec<ScheduledEvent> = Vec::with_capacity(128);
    for i in 0..tracks {
        let graph = &graphs[i];

        current_phrases[i] = graph.initial();
        let phrase = graph.phrase(current_phrases[i]);
        let notes = phrase.notes(&lua);
        let (phrase_end, events) = schedule_phrase(
            &graph.targets,
            &notes,
            phrase.duration_beats(),
            bpm,
            sample_rate,
            0,
        );
        phrase_ends[i] = phrase_end;
        pending_events.extend(events);
    }
    let cutoff = phrase_ends.iter().cloned().min().unwrap_or(0);
    pending_events.sort_by_key(|e| e.sample);
    let cut = pending_events.partition_point(|e| e.sample < cutoff);
    pending_events.drain(..cut).for_each(|e| {
        let _ = producer.try_push(e);
    });

    loop {
        for i in 0..tracks {
            let graph = &graphs[i];
            let current_sample = sample_counter.load(Ordering::Relaxed);
            if current_sample < phrase_ends[i] {
                continue;
            }
            let next = graph.next_phrase(current_phrases[i], &lua);
            let phrase = graph.phrase(next);

            println!("[timing] Transition to {}", phrase.name());

            current_phrases[i] = next;
            let notes = phrase.notes(&lua);

            let (phrase_end, events) = schedule_phrase(
                &graph.targets,
                &notes,
                phrase.duration_beats(),
                bpm,
                sample_rate,
                phrase_ends[i],
            );
            phrase_ends[i] = phrase_end;
            pending_events.extend(events);
        }
        let cutoff = phrase_ends.iter().cloned().min().unwrap_or(0);
        pending_events.sort_by_key(|e| e.sample);
        let cut = pending_events.partition_point(|e| e.sample < cutoff);
        pending_events.drain(..cut).for_each(|e| {
            let _ = producer.try_push(e);
        });
    }
}
