use crate::effect::Smoothed;

/// A low-frequency modulation source, evaluated once per audio frame on the
/// audio thread. Cheap, allocation-free, and lock-free — these are the
/// Rust-side equivalents of the LFO shapes a Lua function would otherwise
/// compute, without the cost (or thread-safety hazard) of calling into Lua
/// per sample.
///
/// Every shape oscillates between `low` and `high` at `freq` Hz.
#[derive(Clone, Copy, Debug)]
pub enum Modulator {
    /// Smooth sinusoid.
    Sine { freq: f32, low: f32, high: f32 },
    /// Linear up-then-down ramp (symmetric triangle).
    Triangle { freq: f32, low: f32, high: f32 },
    /// Rising sawtooth: jumps from `high` back to `low` each cycle.
    Ramp { freq: f32, low: f32, high: f32 },
    /// Hard switch between `low` (first half of the cycle) and `high`.
    Square { freq: f32, low: f32, high: f32 },
}

impl Modulator {
    fn bounds(&self) -> (f32, f32, f32) {
        match *self {
            Modulator::Sine { freq, low, high }
            | Modulator::Triangle { freq, low, high }
            | Modulator::Ramp { freq, low, high }
            | Modulator::Square { freq, low, high } => (freq, low, high),
        }
    }

    /// Map a normalized 0..1 phase to this shape's unipolar 0..1 value.
    fn shape(&self, phase: f32) -> f32 {
        match self {
            Modulator::Sine { .. } => {
                // sin goes -1..1; remap to 0..1.
                0.5 * (1.0 - (phase * 2.0 * std::f32::consts::PI).cos())
            }
            Modulator::Triangle { .. } => {
                // 0 at phase 0, 1 at phase 0.5, back to 0 at phase 1.
                1.0 - (2.0 * phase - 1.0).abs()
            }
            Modulator::Ramp { .. } => phase,
            Modulator::Square { .. } => {
                if phase < 0.5 {
                    0.0
                } else {
                    1.0
                }
            }
        }
    }
}

/// Runtime state for a modulator: the shape plus its running phase.
#[derive(Clone, Copy, Debug)]
pub struct ModState {
    modulator: Modulator,
    phase: f32,
}

impl ModState {
    pub fn new(modulator: Modulator) -> Self {
        Self {
            modulator,
            phase: 0.0,
        }
    }

    /// Advance one frame and return the current modulated value.
    pub fn next(&mut self, sample_rate: f32) -> f32 {
        let (freq, low, high) = self.modulator.bounds();
        let unipolar = self.modulator.shape(self.phase);

        self.phase += (freq / sample_rate).max(0.0);
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
        }

        low + unipolar * (high - low)
    }

    /// Sample the value at the current phase, then advance the phase by `frames`
    /// frames in one step. Used to drive plugin parameters once per processing
    /// block: the value is held for the whole block while the phase stays
    /// continuous over time. `frames` should be modest (the audio callback caps
    /// segments at `MAX_BLOCK` and splits at events) to keep the LFO smooth.
    pub fn next_block(&mut self, sample_rate: f32, frames: usize) -> f32 {
        let (freq, low, high) = self.modulator.bounds();
        let value = low + self.modulator.shape(self.phase) * (high - low);

        self.phase += (freq / sample_rate).max(0.0) * frames as f32;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
        }

        value
    }
}

/// A controllable parameter value. Either a constant (smoothed to avoid zipper
/// noise when changed live, e.g. via MIDI CC) or a modulator that varies on
/// its own each frame. Every effect/synth param is a `Param`, so any of them
/// can be driven by an LFO from Lua.
pub enum Param {
    Const(Smoothed),
    Mod(ModState),
}

impl Param {
    pub fn constant(value: f32, sample_rate: f32) -> Self {
        Param::Const(Smoothed::new(value, 0.01, sample_rate))
    }

    pub fn modulator(modulator: Modulator) -> Self {
        Param::Mod(ModState::new(modulator))
    }

    /// Current value for this frame, advancing internal state.
    #[inline]
    pub fn next(&mut self, sample_rate: f32) -> f32 {
        match self {
            Param::Const(s) => s.next(),
            Param::Mod(m) => m.next(sample_rate),
        }
    }

    /// Set a new target for a constant param. No-op for modulated params —
    /// a live CC can't fight an LFO that owns the value.
    pub fn set_target(&mut self, value: f32) {
        if let Param::Const(s) = self {
            s.set_target(value);
        }
    }
}
