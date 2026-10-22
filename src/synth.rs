use crate::{
    frame::Frame,
    instrument::Instrument,
    midi::{NoteEvent, midi_to_freq},
    oscillator::{Oscillator, Wave},
    param::Param,
};
#[derive(Clone)]
pub struct ADSR {
    pub a: f32,
    pub d: f32,
    pub s: f32,
    pub r: f32,
}

pub enum EnvelopePhase {
    Attack(f32),
    Decay(f32),
    Sustain,
    /// Elapsed time, and the level the release started from.
    Release(f32, f32),
    Done,
}

/// Shortest attack/release ramp, in seconds. A zero-length ramp steps the
/// waveform straight to (or from) full level, which is an audible click.
const MIN_RAMP: f32 = 0.003;

struct Voice {
    oscillator: Oscillator,
    /// Index into the synth's `oscillators`, so the voice can pick up that
    /// oscillator's (possibly modulated) gain each frame.
    osc_index: usize,
    pitch: u8,
    velocity: u8,
    envelope_phase: EnvelopePhase,
    envelope_level: f32,
}

impl Voice {
    pub fn new(wave: Wave, osc_index: usize, pitch: u8, velocity: u8) -> Self {
        Self {
            oscillator: Oscillator::new(wave),
            osc_index,
            pitch: pitch.clamp(0, 127),
            velocity: velocity.clamp(1, 127),
            envelope_phase: EnvelopePhase::Attack(0.0),
            envelope_level: 0.0,
        }
    }

    pub fn release(&mut self) {
        self.envelope_phase = EnvelopePhase::Release(0.0, self.envelope_level);
    }

    pub fn is_done(&self) -> bool {
        matches!(self.envelope_phase, EnvelopePhase::Done)
    }

    /// Produce this voice's sample for the frame. `gain` is the oscillator's
    /// already-evaluated gain for this frame (a `Param` is stateful, so it's
    /// sampled once per oscillator by the synth, not per voice).
    pub fn next_sample(&mut self, adsr: &ADSR, gain: f32, sample_rate: f32) -> f32 {
        let freq = midi_to_freq(self.pitch);
        let raw = self.oscillator.next_sample(freq, sample_rate);
        let velocity_scale = (self.velocity as f32) / 127.0;
        let level = self.envelope_level;
        self.advance_envelope(adsr, sample_rate);

        raw * level * velocity_scale * gain.clamp(0.0, 1.0)
    }

    fn advance_envelope(&mut self, adsr: &ADSR, sample_rate: f32) {
        let dt = 1.0 / sample_rate;

        match &mut self.envelope_phase {
            EnvelopePhase::Attack(t) => {
                *t += dt;
                let a = adsr.a.max(MIN_RAMP);
                if *t >= a {
                    self.envelope_level = 1.0;
                    self.envelope_phase = EnvelopePhase::Decay(0.0);
                } else {
                    self.envelope_level = *t / a;
                }
            }
            EnvelopePhase::Decay(t) => {
                *t += dt;
                if *t >= adsr.d {
                    self.envelope_level = adsr.s;
                    self.envelope_phase = EnvelopePhase::Sustain;
                } else {
                    let progress = if adsr.d > 0.0 { *t / adsr.d } else { 1.0 };
                    self.envelope_level = 1.0 - (1.0 - adsr.s) * progress;
                }
            }
            EnvelopePhase::Sustain => {
                self.envelope_level = adsr.s;
            }
            EnvelopePhase::Release(t, start_level) => {
                *t += dt;
                let r = adsr.r.max(MIN_RAMP);
                if *t >= r {
                    self.envelope_level = 0.0;
                    self.envelope_phase = EnvelopePhase::Done;
                } else {
                    // Linear fade from wherever the note was when released.
                    self.envelope_level = *start_level * (1.0 - *t / r);
                }
            }
            EnvelopePhase::Done => {}
        }
    }
}

pub struct Synth {
    pub id: usize,
    pub adsr: ADSR,
    /// Each oscillator carries a `Param` gain, sampled once per frame so the
    /// gain can be a constant or an LFO without per-voice phase divergence.
    pub oscillators: Vec<(Oscillator, Param)>,
    voices: Vec<Voice>,
    /// Per-frame scratch holding each oscillator's evaluated gain. Pre-sized so
    /// the audio callback never allocates.
    gain_scratch: Vec<f32>,
}

impl Synth {
    pub fn new(id: usize, adsr: ADSR, oscillators: Vec<(Oscillator, Param)>) -> Self {
        let gain_scratch = vec![0.0; oscillators.len()];
        Self {
            id,
            adsr,
            oscillators,
            voices: Vec::new(),
            gain_scratch,
        }
    }

    pub fn add_oscillator(&mut self, wave: Wave, gain: Param) {
        self.oscillators.push((Oscillator::new(wave), gain));
        self.gain_scratch.push(0.0);
    }
}

impl Instrument for Synth {
    fn process_event(&mut self, event: &NoteEvent) {
        match event {
            NoteEvent::NoteOn { pitch, velocity } => {
                // Fade out a still-sounding voice on the same pitch rather
                // than cutting it off mid-cycle.
                for voice in &mut self.voices {
                    if voice.pitch == *pitch && !matches!(voice.envelope_phase, EnvelopePhase::Release(..)) {
                        voice.release();
                    }
                }
                for (idx, (oscillator, _gain)) in self.oscillators.iter().enumerate() {
                    self.voices
                        .push(Voice::new(oscillator.wave.clone(), idx, *pitch, *velocity));
                }
            }
            NoteEvent::NoteOff { pitch } => {
                for voice in &mut self.voices {
                    if voice.pitch == *pitch {
                        voice.release();
                    }
                }
            }
        }
    }

    fn next_sample(&mut self, sample_rate: f32) -> Frame {
        // Evaluate every oscillator's gain once for this frame into scratch.
        for (i, (_, gain)) in self.oscillators.iter_mut().enumerate() {
            self.gain_scratch[i] = gain.tick(sample_rate);
        }

        let mut output = 0.0;
        for voice in &mut self.voices {
            let gain = self.gain_scratch[voice.osc_index];
            output += voice.next_sample(&self.adsr, gain, sample_rate);
        }
        self.voices.retain(|v| !v.is_done());
        Frame::mono(output)
    }

    fn id(&self) -> usize {
        self.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    /// Largest sample-to-sample jump while playing `events` at the given
    /// frame offsets, for `total` frames.
    fn max_jump(adsr: ADSR, wave: Wave, events: &[(usize, NoteEvent)], total: usize) -> f32 {
        let mut synth = Synth::new(0, adsr, vec![]);
        synth.add_oscillator(wave, Param::constant(1.0, SR));
        let (mut prev, mut worst) = (0.0f32, 0.0f32);
        for i in 0..total {
            for (at, ev) in events {
                if *at == i {
                    synth.process_event(ev);
                }
            }
            let s = synth.next_sample(SR).l;
            worst = worst.max((s - prev).abs());
            prev = s;
        }
        worst
    }

    // A 55 Hz full-scale sine moves at most ~0.0072 per sample, so anything
    // well above that is a discontinuity (a click).
    const CLICK: f32 = 0.02;

    #[test]
    fn release_mid_decay_fades_from_current_level() {
        let kick = ADSR { a: 0.0, d: 0.16, s: 0.0, r: 0.04 };
        let on = NoteEvent::NoteOn { pitch: 33, velocity: 127 };
        let off = NoteEvent::NoteOff { pitch: 33 };
        let jump = max_jump(kick, Wave::Sine, &[(0, on), (2_800, off)], 6_000);
        assert!(jump < CLICK, "jump {jump}");
    }

    #[test]
    fn retrigger_same_pitch_does_not_cut() {
        let bass = ADSR { a: 0.0, d: 0.12, s: 0.45, r: 0.04 };
        let on = || NoteEvent::NoteOn { pitch: 33, velocity: 127 };
        let jump = max_jump(bass, Wave::Sine, &[(0, on()), (1_500, on())], 4_000);
        assert!(jump < CLICK, "jump {jump}");
    }
}
