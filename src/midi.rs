#[derive(Debug, Clone)]
pub struct MidiNote {
    pub pitch: u8,
    pub velocity: u8,
    pub start_beat: f32,
    pub duration_beats: f32,
}

#[derive(Clone, Debug)]
pub enum NoteEvent {
    NoteOn { pitch: u8, velocity: u8 },
    NoteOff { pitch: u8 },
}

#[derive(Clone, Debug)]
pub struct ScheduledEvent {
    pub targets: Vec<usize>,
    pub sample: u64,
    pub event: NoteEvent,
}

impl ScheduledEvent {
    pub fn new(targets: Vec<usize>, sample: u64, event: NoteEvent) -> Self {
        Self {
            targets,
            sample,
            event,
        }
    }
}

pub fn midi_to_freq(note: u8) -> f32 {
    440.0 * 2.0_f32.powf((note as f32 - 69.0) / 12.0)
}

pub fn schedule_phrase(
    targets: &[usize],
    notes: &[MidiNote],
    duration_beats: f32,
    bpm: f32,
    sample_rate: f32,
    start_sample: u64,
) -> (u64, Vec<ScheduledEvent>) {
    let sample_per_beat = (60.0 / bpm) * sample_rate;
    let phrase_duration_samples = (duration_beats * sample_per_beat) as u64;
    let phrase_end = start_sample + phrase_duration_samples;

    let mut events = Vec::new();

    for note in notes {
        let on_sample = start_sample + (note.start_beat * sample_per_beat) as u64;
        if on_sample > phrase_end {
            continue;
        }

        let off_sample = on_sample + (note.duration_beats * sample_per_beat) as u64;

        events.push(ScheduledEvent::new(
            targets.to_vec(),
            on_sample,
            NoteEvent::NoteOn {
                pitch: note.pitch,
                velocity: note.velocity,
            },
        ));
        events.push(ScheduledEvent::new(
            targets.to_vec(),
            off_sample.clamp(0, phrase_end),
            NoteEvent::NoteOff { pitch: note.pitch },
        ));
    }
    events.sort_by_key(|e| e.sample);
    (phrase_end, events)
}
