use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use crate::{
    midi::{ScheduledEvent, schedule_phrase},
    phrase::PhraseEntry,
    scripting::LuaRuntime,
    transition::Transition,
};
use mlua::RegistryKey;
use ringbuf::{HeapProd, traits::Producer};

pub struct StateGraph {
    targets: Vec<usize>,
    phrases: Vec<PhraseEntry>,
    transitions: Vec<Transition>,
    initial: usize,
}

impl StateGraph {
    pub fn new(
        targets: Vec<usize>,
        phrases: Vec<PhraseEntry>,
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

    pub fn entry(&self, id: usize) -> &PhraseEntry {
        &self.phrases[id]
    }

    pub fn next_phrase(&self, current: usize, lua: &LuaRuntime) -> usize {
        for transition in &self.transitions {
            if transition.from == current {
                let result = match &transition.condition {
                    None => Ok(true),
                    Some(key) => lua.call_condition(key),
                };
                match result {
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

fn run_hook(lua: &LuaRuntime, key: &Option<RegistryKey>) {
    if let Some(k) = key
        && let Err(err) = lua.run_hook(k)
    {
        eprintln!("[lua hook error] {}", err);
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
        let entry = graph.entry(current_phrases[i]);

        run_hook(lua, &entry.hooks.on_enter);

        let notes = entry.phrase.notes(lua);
        let (phrase_end, events) = schedule_phrase(
            &graph.targets,
            &notes,
            entry.phrase.duration_beats(),
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
            let from_idx = current_phrases[i];
            let to_idx = graph.next_phrase(from_idx, lua);

            let from_entry = graph.entry(from_idx);
            let to_entry = graph.entry(to_idx);

            if from_idx == to_idx {
                run_hook(lua, &from_entry.hooks.on_loop);
            } else {
                run_hook(lua, &from_entry.hooks.on_exit);
                run_hook(lua, &to_entry.hooks.on_enter);
            }

            println!("[timing] Transition to {}", to_entry.phrase.name());

            current_phrases[i] = to_idx;
            let notes = to_entry.phrase.notes(lua);

            let (phrase_end, events) = schedule_phrase(
                &graph.targets,
                &notes,
                to_entry.phrase.duration_beats(),
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
