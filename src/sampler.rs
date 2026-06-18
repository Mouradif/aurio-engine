use std::path::Path;

use crate::instrument::Instrument;
use crate::midi::NoteEvent;

/// A loaded WAV file: mono PCM data plus the sample rate it was recorded at.
/// Owned by its `Zone`; voices reference it by index so playback touches no
/// shared ownership and allocates nothing in the audio callback.
pub struct Sample {
    pub data: Vec<f32>,
    pub source_rate: f32,
}

impl Sample {
    /// Load a WAV file from disk, downmixing to mono and normalizing integer
    /// PCM to the -1.0..=1.0 range. Float WAVs are taken as-is.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let path = path.as_ref();
        let reader = hound::WavReader::open(path)
            .map_err(|e| format!("failed to open '{}': {}", path.display(), e))?;
        let spec = reader.spec();
        let channels = spec.channels.max(1) as usize;

        let samples: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => reader
                .into_samples::<f32>()
                .collect::<Result<_, _>>()
                .map_err(|e| format!("reading '{}': {}", path.display(), e))?,
            hound::SampleFormat::Int => {
                let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
                reader
                    .into_samples::<i32>()
                    .map(|s| s.map(|v| v as f32 / max))
                    .collect::<Result<_, _>>()
                    .map_err(|e| format!("reading '{}': {}", path.display(), e))?
            }
        };

        // Downmix interleaved channels to mono by averaging.
        let data: Vec<f32> = if channels <= 1 {
            samples
        } else {
            samples
                .chunks(channels)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                .collect()
        };

        Ok(Self {
            data,
            source_rate: spec.sample_rate as f32,
        })
    }
}

/// One sample mapped to a key range, pitched relative to its root note.
/// A played note within `[low, high]` plays `sample`, pitch-shifted by the
/// interval between the note and `root`.
pub struct Zone {
    pub sample: Sample,
    pub root: u8,
    pub low: u8,
    pub high: u8,
}

/// A single playing instance of a sample. References its source data by zone
/// index; carries a fractional cursor for linear-interpolated resampling.
struct Voice {
    zone: usize,
    /// Fractional read position into the zone's sample data, in source frames.
    pos: f32,
    /// Speed multiplier from pitch-shift alone (`2^(semitones/12)`); the
    /// sample-rate conversion factor is applied per-frame in `next_sample`
    /// since the output rate isn't known until then.
    pitch_ratio: f32,
    gain: f32,
}

/// An instrument that plays back loaded WAV samples, pitch-shifted relative to
/// each sample's root note. Notes are matched to the first zone whose key
/// range contains them. Playback is one-shot: a sample always plays to its
/// end, and `NoteOff` is ignored.
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

    /// Index of the first zone whose key range contains `pitch`.
    fn zone_for(&self, pitch: u8) -> Option<usize> {
        self.zones
            .iter()
            .position(|z| pitch >= z.low && pitch <= z.high)
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
            && let Some(zone_idx) = self.zone_for(*pitch)
        {
            let zone = &self.zones[zone_idx];
            // Each semitone above/below root scales playback speed by 2^(1/12).
            let semitones = *pitch as f32 - zone.root as f32;
            self.voices.push(Voice {
                zone: zone_idx,
                pos: 0.0,
                pitch_ratio: 2.0_f32.powf(semitones / 12.0),
                gain: (*velocity as f32) / 127.0,
            });
        }
    }

    fn next_sample(&mut self, sample_rate: f32) -> f32 {
        let mut output = 0.0;

        for voice in &mut self.voices {
            let data = &self.zones[voice.zone].sample.data;
            let i = voice.pos.floor() as usize;
            if i + 1 >= data.len() {
                continue;
            }
            let frac = voice.pos - i as f32;
            let s = data[i] * (1.0 - frac) + data[i + 1] * frac;
            output += s * voice.gain;

            // Advance: pitch shift combined with source→output rate conversion.
            let step = voice.pitch_ratio * self.zones[voice.zone].sample.source_rate / sample_rate;
            voice.pos += step;
        }

        // Drop voices whose cursor has run past the end (one-shot complete).
        self.voices
            .retain(|v| (v.pos.floor() as usize) + 1 < self.zones[v.zone].sample.data.len());

        output
    }
}
