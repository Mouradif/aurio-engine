#[derive(Clone)]
pub enum Wave {
    Sine,
    Square,
    Saw,
    Triangle,
    Noise,
}

#[derive(Clone)]
pub struct Oscillator {
    pub wave: Wave,
    pub phase: f32,
    rng_state: u32, // for noise
}

impl Oscillator {
    pub fn new(wave: Wave) -> Self {
        Self {
            wave,
            phase: 0.0,
            rng_state: 0x12345678, // simple seed
        }
    }

    /// PolyBLEP correction for a step discontinuity.
    ///
    /// A naive square/saw is a sum of infinitely many harmonics, so every
    /// harmonic above Nyquist folds back down as inharmonic aliasing. This
    /// adds a 2-sample-wide polynomial around each edge that approximates a
    /// band-limited step, which cancels most of that. `t` is the phase, `dt`
    /// the per-sample phase increment; the correction is zero outside the
    /// edges, so the cost is a few multiplies on ~2 samples per transition.
    fn poly_blep(t: f32, dt: f32) -> f32 {
        if t < dt {
            // just after the edge
            let t = t / dt;
            t + t - t * t - 1.0
        } else if t > 1.0 - dt {
            // just before the next edge
            let t = (t - 1.0) / dt;
            t * t + t + t + 1.0
        } else {
            0.0
        }
    }

    fn noise(&mut self) -> f32 {
        // xorshift32 (fast, good enough for audio noise)
        let mut x = self.rng_state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng_state = x;

        // map to [-1.0, 1.0]
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    pub fn next_sample(&mut self, freq: f32, sample_rate: f32) -> f32 {
        let dt = freq / sample_rate;

        let sample = match self.wave {
            Wave::Sine => (self.phase * 2.0 * std::f32::consts::PI).sin(),

            Wave::Square => {
                // Slightly-off duty cycle: an analog comparator is never
                // perfectly symmetric, and the even harmonics that asymmetry
                // introduces are a good part of why a real square sounds
                // thicker than a mathematical one.
                const DUTY: f32 = 0.48;

                let mut s = if self.phase < DUTY { 1.0 } else { -1.0 };

                // Cancel aliasing at both edges: the rising one at phase 0 and
                // the falling one at phase DUTY.
                s += Self::poly_blep(self.phase, dt);
                let falling = self.phase - DUTY;
                s -= Self::poly_blep(if falling < 0.0 { falling + 1.0 } else { falling }, dt);

                s
            }

            // Same discontinuity, same fix: one edge per cycle at phase 0.
            Wave::Saw => 2.0 * self.phase - 1.0 - Self::poly_blep(self.phase, dt),

            Wave::Triangle => 2.0 * (2.0 * self.phase - 1.0).abs() - 1.0,

            Wave::Noise => self.noise(),
        };

        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }

        sample
    }
}
