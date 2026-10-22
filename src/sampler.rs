use std::path::Path;
use std::sync::Arc;

use crate::audio_file;
use crate::frame::Frame;
use crate::instrument::Instrument;
use crate::midi::NoteEvent;

/// Loaded PCM: stereo data plus the sample rate it was recorded at.
///
/// Zones hold this behind an `Arc` because a consolidated library maps every
/// zone onto ranges of one big audio file; sharing means that file is decoded
/// once rather than once per zone. Voices reference their zone by index, so
/// playback touches no shared ownership and allocates nothing in the callback.
pub struct Sample {
    pub data: Vec<Frame>,
    pub source_rate: f32,
}

impl Sample {
    /// Load an audio file from disk as stereo, normalizing integer PCM to the
    /// -1.0..=1.0 range. WAV, AIFF/AIFC and CAF are supported.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        Self::load_range(path, None)
    }

    /// Load only `range` (a frame offset and count) out of a file. Used by
    /// consolidated sampler libraries, where one file holds every recording
    /// back to back and each zone owns a slice of it.
    pub fn load_range<P: AsRef<Path>>(
        path: P,
        range: audio_file::FrameRange,
    ) -> Result<Self, String> {
        let d = audio_file::load(path, range)?;
        Ok(Self {
            data: d.data,
            source_rate: d.source_rate,
        })
    }
}

/// One sample mapped to a key range, pitched relative to its root note.
///
/// A played note picks the first zone whose key *and* velocity range contain
/// it, then plays `sample` shifted by the interval from `root`. Velocity
/// layers let one key select between several recordings by how hard it is
/// struck, which is how sampled instruments capture playing dynamics.
pub struct Zone {
    pub sample: Arc<Sample>,
    pub root: u8,
    pub low: u8,
    pub high: u8,
    pub vel_low: u8,
    pub vel_high: u8,
    /// Frame offset into `sample` where this zone's audio begins.
    pub start: usize,
    /// Frame count, or `None` to play to the end of the data.
    pub length: Option<usize>,
    /// Detune in cents, applied on top of the root-relative interval.
    pub fine_tune: f32,
    /// Linear gain multiplier for this zone.
    pub gain: f32,
    /// When false the zone ignores the played key and plays at its recorded
    /// pitch, which suits drums and other unpitched material.
    pub pitch: bool,
}

impl Zone {
    /// A zone covering the whole keyboard at every velocity, rooted at `root`.
    pub fn full_range(sample: Arc<Sample>, root: u8) -> Self {
        Self {
            sample,
            root,
            low: 0,
            high: 127,
            vel_low: 0,
            vel_high: 127,
            start: 0,
            length: None,
            fine_tune: 0.0,
            gain: 1.0,
            pitch: true,
        }
    }

    /// Index one past the last frame this zone may read.
    fn end(&self) -> usize {
        match self.length {
            Some(n) => (self.start + n).min(self.sample.data.len()),
            None => self.sample.data.len(),
        }
    }
}

/// A single playing instance of a sample. References its source data by zone
/// index; carries a fractional cursor for linear-interpolated resampling.
struct Voice {
    zone: usize,
    /// Fractional read position into the zone's sample data, in source frames.
    /// Absolute within the file, so a zone playing a slice of a consolidated
    /// library starts at that slice's offset rather than at zero.
    pos: f32,
    /// Speed multiplier from pitch-shift alone (`2^(semitones/12)`); the
    /// sample-rate conversion factor is applied per-frame in `next_sample`
    /// since the output rate isn't known until then.
    pitch_ratio: f32,
    gain: f32,
}

/// An instrument that plays back loaded samples, pitch-shifted relative to
/// each sample's root note. Notes are matched to the first zone whose key and
/// velocity ranges contain them. Playback is one-shot: a sample always plays
/// to its end, and `NoteOff` is ignored.
pub struct Sampler {
    pub id: usize,
    zones: Vec<Zone>,
    voices: Vec<Voice>,
}

impl Sampler {
    pub fn new(id: usize, zones: Vec<Zone>) -> Self {
        Self {
            id,
            zones,
            voices: Vec::new(),
        }
    }

    /// Index of the first zone matching both the key and the velocity. Falls
    /// back to a key-only match so a note still sounds when an instrument's
    /// velocity layers leave gaps.
    fn zone_for(&self, pitch: u8, velocity: u8) -> Option<usize> {
        self.zones
            .iter()
            .position(|z| {
                pitch >= z.low
                    && pitch <= z.high
                    && velocity >= z.vel_low
                    && velocity <= z.vel_high
            })
            .or_else(|| {
                self.zones
                    .iter()
                    .position(|z| pitch >= z.low && pitch <= z.high)
            })
    }
}

impl Instrument for Sampler {
    fn id(&self) -> usize {
        self.id
    }

    fn process_event(&mut self, event: &NoteEvent) {
        // One-shot: only NoteOn starts playback; NoteOff is ignored so the
        // sample always plays through to its natural end.
        if let NoteEvent::NoteOn { pitch, velocity } = event
            && let Some(zone_idx) = self.zone_for(*pitch, *velocity)
        {
            let zone = &self.zones[zone_idx];
            // Each semitone above/below root scales playback speed by 2^(1/12);
            // fine tune adds a fraction of a semitone on top.
            let semitones = if zone.pitch {
                *pitch as f32 - zone.root as f32
            } else {
                0.0
            } + zone.fine_tune / 100.0;
            self.voices.push(Voice {
                zone: zone_idx,
                pos: zone.start as f32,
                pitch_ratio: 2.0_f32.powf(semitones / 12.0),
                gain: (*velocity as f32) / 127.0 * zone.gain,
            });
        }
    }

    fn next_sample(&mut self, sample_rate: f32) -> Frame {
        let mut output = Frame::SILENCE;

        for voice in &mut self.voices {
            let zone = &self.zones[voice.zone];
            let data = &zone.sample.data;
            let end = zone.end();
            let i = voice.pos.floor() as usize;
            if i + 1 >= end {
                continue;
            }
            let frac = voice.pos - i as f32;
            // Linear interpolation per channel, via frame arithmetic.
            let s = data[i] * (1.0 - frac) + data[i + 1] * frac;
            output += s * voice.gain;

            // Advance: pitch shift combined with source→output rate conversion.
            let step = voice.pitch_ratio * zone.sample.source_rate / sample_rate;
            voice.pos += step;
        }

        // Drop voices whose cursor has run past the end (one-shot complete).
        let zones = &self.zones;
        self.voices
            .retain(|v| (v.pos.floor() as usize) + 1 < zones[v.zone].end());

        output
    }
}
