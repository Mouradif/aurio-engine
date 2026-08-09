/// The song's tempo, shared across threads and safe to change while playing.
///
/// Stored as bits in an `AtomicU32` rather than behind a lock: the timing
/// thread reads it at every phrase boundary, and a Lua handler or an incoming
/// MIDI clock may write it at any moment, so neither side can afford to block
/// the other.
///
/// A change takes effect on the **next phrase to be scheduled**, not on the
/// one already playing. Notes are planned a whole phrase ahead, and nothing
/// already scheduled is disturbed — so a tempo change is never audible as a
/// cut or a retrigger, at the cost of waiting out the current phrase.
#[derive(Debug)]
pub struct Tempo {
    bits: std::sync::atomic::AtomicU32,
}

impl Tempo {
    /// Tempo is clamped to a musically sane range. A zero or negative BPM
    /// would divide by zero in `schedule_phrase`; an absurd one would flood
    /// the event queue.
    pub const MIN_BPM: f32 = 20.0;
    pub const MAX_BPM: f32 = 400.0;

    pub fn new(bpm: f32) -> Self {
        let t = Self {
            bits: std::sync::atomic::AtomicU32::new(0),
        };
        t.set(bpm);
        t
    }

    pub fn get(&self) -> f32 {
        f32::from_bits(self.bits.load(std::sync::atomic::Ordering::Relaxed))
    }

    /// Set the tempo, clamping to the supported range. Returns what was
    /// actually stored, so callers can report a clamp.
    ///
    /// Cheap enough to call at MIDI-clock rates: it's a single atomic store,
    /// and the timing thread only reads it when it next schedules a phrase.
    pub fn set(&self, bpm: f32) -> f32 {
        let clamped = if bpm.is_finite() {
            bpm.clamp(Self::MIN_BPM, Self::MAX_BPM)
        } else {
            120.0
        };
        self.bits
            .store(clamped.to_bits(), std::sync::atomic::Ordering::Relaxed);
        clamped
    }
}

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

/// What a `ScheduledEvent` actually does at its sample. Notes route to
/// instrument sources by id; param updates route to a specific processor
/// node by its pre-resolved index in the audio graph.
#[derive(Clone, Debug)]
pub enum EventKind {
    Note {
        targets: Vec<usize>,
        event: NoteEvent,
    },
    SetParam {
        node_idx: usize,
        param_idx: u8,
        value: f32,
    },
    /// Parameter automation for a plugin node (CLAP param id or LV2 control-port
    /// index), addressed by the plugin's stable id, not the u8 index native
    /// effects use.
    #[cfg(any(feature = "clap-host", feature = "lv2-host"))]
    SetPluginParam {
        node_idx: usize,
        param_id: u32,
        value: f32,
    },
}

#[derive(Clone, Debug)]
pub struct ScheduledEvent {
    pub sample: u64,
    pub kind: EventKind,
}

impl ScheduledEvent {
    pub fn note(targets: Vec<usize>, sample: u64, event: NoteEvent) -> Self {
        Self {
            sample,
            kind: EventKind::Note { targets, event },
        }
    }

    pub fn set_param(sample: u64, node_idx: usize, param_idx: u8, value: f32) -> Self {
        Self {
            sample,
            kind: EventKind::SetParam {
                node_idx,
                param_idx,
                value,
            },
        }
    }

    #[cfg(any(feature = "clap-host", feature = "lv2-host"))]
    pub fn set_plugin_param(sample: u64, node_idx: usize, param_id: u32, value: f32) -> Self {
        Self {
            sample,
            kind: EventKind::SetPluginParam {
                node_idx,
                param_id,
                value,
            },
        }
    }
}

pub fn midi_to_freq(note: u8) -> f32 {
    440.0 * 2.0_f32.powf((note as f32 - 69.0) / 12.0)
}

/// Parse a note name like `"c3"`, `"f#2"`, `"A4"` into a MIDI pitch.
///
/// Uses the same octave convention as the RTTTL parser: `c` at octave 0 is
/// MIDI 36, so `c3` is 72. Accepts upper/lowercase letters, an optional `#`
/// sharp, and a single-digit octave (defaults to 3 if omitted).
pub fn note_name_to_midi(name: &str) -> Result<u8, String> {
    let mut chars = name.trim().chars().peekable();

    let letter = chars
        .next()
        .ok_or_else(|| format!("empty note name: '{}'", name))?
        .to_ascii_lowercase();
    let base: i32 = match letter {
        'c' => 36,
        'd' => 38,
        'e' => 40,
        'f' => 41,
        'g' => 43,
        'a' => 45,
        'b' => 47,
        _ => return Err(format!("invalid note letter in '{}'", name)),
    };

    let mut semitone = base;
    if matches!(chars.peek(), Some('#')) {
        chars.next();
        semitone += 1;
    } else if matches!(chars.peek(), Some('b')) {
        chars.next();
        semitone -= 1;
    }

    let octave: i32 = match chars.next() {
        Some(d @ '0'..='9') => (d as u8 - b'0') as i32,
        None => 3,
        Some(c) => return Err(format!("invalid octave '{}' in '{}'", c, name)),
    };

    if chars.next().is_some() {
        return Err(format!("trailing characters in note name: '{}'", name));
    }

    let pitch = semitone + 12 * octave;
    u8::try_from(pitch).map_err(|_| format!("note '{}' out of MIDI range", name))
}

/// Parse a pitch from a Lua value: either a note-name string like `"c3"` /
/// `"f#2"` or a raw MIDI number. Shared by every Lua surface that takes a
/// pitch (phrase notes, sampler zones, ...).
pub fn parse_note_value(v: &mlua::Value) -> mlua::Result<u8> {
    match v {
        mlua::Value::String(s) => {
            note_name_to_midi(&s.to_str()?).map_err(mlua::Error::RuntimeError)
        }
        mlua::Value::Integer(i) => u8::try_from(*i)
            .map_err(|_| mlua::Error::RuntimeError(format!("note out of range: {}", i))),
        mlua::Value::Number(n) => u8::try_from(*n as i64)
            .map_err(|_| mlua::Error::RuntimeError(format!("note out of range: {}", n))),
        _ => Err(mlua::Error::RuntimeError(
            "expected a note name string or MIDI number".into(),
        )),
    }
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

        events.push(ScheduledEvent::note(
            targets.to_vec(),
            on_sample,
            NoteEvent::NoteOn {
                pitch: note.pitch,
                velocity: note.velocity,
            },
        ));
        events.push(ScheduledEvent::note(
            targets.to_vec(),
            off_sample.clamp(0, phrase_end),
            NoteEvent::NoteOff { pitch: note.pitch },
        ));
    }
    events.sort_by_key(|e| e.sample);
    (phrase_end, events)
}

#[cfg(test)]
mod tempo_tests {
    use super::*;

    #[test]
    fn tempo_roundtrips_and_clamps() {
        let t = Tempo::new(120.0);
        assert_eq!(t.get(), 120.0);

        assert_eq!(t.set(140.0), 140.0);
        assert_eq!(t.get(), 140.0);

        // Out of range is clamped, not rejected.
        assert_eq!(t.set(5.0), Tempo::MIN_BPM);
        assert_eq!(t.set(10_000.0), Tempo::MAX_BPM);

        // A non-finite tempo would poison every later schedule_phrase call.
        t.set(f32::NAN);
        assert!(t.get().is_finite());
        t.set(f32::INFINITY);
        assert!(t.get().is_finite());
    }

    #[test]
    fn tempo_changes_phrase_scheduling() {
        // The point of a live tempo: the same phrase scheduled at a faster
        // tempo must end sooner.
        let notes = vec![MidiNote {
            pitch: 60,
            velocity: 100,
            start_beat: 0.0,
            duration_beats: 1.0,
        }];
        let (end_slow, _) = schedule_phrase(&[0], &notes, 4.0, 60.0, 48_000.0, 0);
        let (end_fast, _) = schedule_phrase(&[0], &notes, 4.0, 120.0, 48_000.0, 0);
        assert_eq!(end_slow, 4 * 48_000);
        assert_eq!(end_fast, 2 * 48_000);
    }
}
