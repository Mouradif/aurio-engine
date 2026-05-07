use std::char;

use crate::midi::MidiNote;

pub fn parse_rtttl(rtttl: &str) -> (f32, Vec<MidiNote>) {
    let mut beat: f32 = 0.0;

    let notes = rtttl
        .split_whitespace()
        .filter_map(|w| {
            let note = parse_rtttl_note(w, beat)?;
            beat += note.duration_beats;
            if note.pitch == 0 { None } else { Some(note) }
        })
        .collect();
    (beat, notes)
}

fn parse_rtttl_note(word: &str, beat: f32) -> Option<MidiNote> {
    let mut chars = word.char_indices().peekable();

    let start = 0;
    let mut end = 0;
    while let Some(&(i, ch)) = chars.peek() {
        if ch.is_ascii_digit() {
            end = i + 1;
            chars.next();
        } else {
            break;
        }
    }
    if end == 0 || end - start > 2 {
        return None;
    }
    let number: u8 = word[start..end].parse().ok()?;
    let mut duration_beats = 4.0 / number as f32;

    let (_, note) = chars.next()?;
    let is_rest = note == '-';
    if !is_rest && !matches!(note, 'a'..='g') {
        return None;
    }

    let sharp = matches!(chars.peek(), Some(&(_, '#')));
    if sharp {
        chars.next();
    }
    if sharp && (is_rest || note == 'b' || note == 'e') {
        return None;
    }

    let dot = matches!(chars.peek(), Some(&(_, '.')));
    if dot {
        chars.next();
        duration_beats *= 1.5;
    }

    let octave = match chars.peek() {
        Some(&(_, ch @ '1'..='4')) => {
            chars.next();
            ch as u8 - b'0'
        }
        _ => 0,
    };
    if is_rest && octave > 0 {
        return None;
    }

    if chars.next().is_some() {
        return None;
    }

    Some(MidiNote {
        pitch: rtttl_note_to_midi(note, sharp, octave),
        velocity: 100,
        start_beat: beat,
        duration_beats,
    })
}

fn rtttl_note_to_midi(note: char, sharp: bool, octave: u8) -> u8 {
    let base = match note {
        '-' => 0,
        'c' => 36,
        'd' => 38,
        'e' => 40,
        'f' => 41,
        'g' => 43,
        'a' => 45,
        'b' => 47,
        _ => unreachable!(),
    };
    base + sharp as u8 + 12 * octave
}
