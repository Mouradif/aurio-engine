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
        let sample = match self.wave {
            Wave::Sine => (self.phase * 2.0 * std::f32::consts::PI).sin(),

            Wave::Square => {
                if self.phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }

            Wave::Saw => 2.0 * self.phase - 1.0,

            Wave::Triangle => 2.0 * (2.0 * self.phase - 1.0).abs() - 1.0,

            Wave::Noise => self.noise(),
        };

        self.phase += freq / sample_rate;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }

        sample
    }
}
