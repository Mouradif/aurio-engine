/// Audio-thread effect node. `process` runs once per sample; `set_param`
/// applies a live parameter update (default no-op for effects without
/// controllable params).
pub trait Effect {
    fn process(&mut self, sample: f32) -> f32;
    fn set_param(&mut self, _idx: u8, _value: f32) {}
}

/// One-pole low-pass smoother for parameter values. Avoids zipper noise
/// when a target value jumps (e.g. on each MIDI CC tick).
pub struct Smoothed {
    target: f32,
    current: f32,
    coeff: f32,
}

impl Smoothed {
    pub fn new(initial: f32, time_constant_seconds: f32, sample_rate: f32) -> Self {
        let tau = time_constant_seconds.max(1e-6);
        let coeff = 1.0 - (-1.0 / (tau * sample_rate)).exp();
        Self {
            target: initial,
            current: initial,
            coeff,
        }
    }

    pub fn set_target(&mut self, value: f32) {
        self.target = value;
    }

    pub fn next(&mut self) -> f32 {
        self.current += self.coeff * (self.target - self.current);
        self.current
    }
}

pub struct Gain {
    level: Smoothed,
}

impl Gain {
    pub const PARAM_LEVEL: u8 = 0;

    pub fn new(level: f32, sample_rate: f32) -> Self {
        Self {
            level: Smoothed::new(level, 0.01, sample_rate),
        }
    }
}

impl Effect for Gain {
    fn process(&mut self, sample: f32) -> f32 {
        sample * self.level.next()
    }

    fn set_param(&mut self, idx: u8, value: f32) {
        if idx == Self::PARAM_LEVEL {
            self.level.set_target(value);
        }
    }
}

pub struct Reverb {
    buffer: Vec<f32>,
    write_pos: usize,
    feedback: f32,
    mix: f32,
}

impl Reverb {
    pub fn new(delay_seconds: f32, feedback: f32, mix: f32, sample_rate: f32) -> Self {
        let buffer_size = (delay_seconds * sample_rate) as usize;
        Self {
            buffer: vec![0.0; buffer_size],
            write_pos: 0,
            feedback: feedback.clamp(0.0, 0.99),
            mix: mix.clamp(0.0, 1.0),
        }
    }
}

impl Effect for Reverb {
    fn process(&mut self, sample: f32) -> f32 {
        if self.buffer.is_empty() {
            return sample;
        }

        let delayed = self.buffer[self.write_pos];
        let wet = sample + delayed * self.feedback;

        self.buffer[self.write_pos] = wet;
        self.write_pos = (self.write_pos + 1) % self.buffer.len();

        sample * (1.0 - self.mix) + wet * self.mix
    }
}

pub struct Lowpass {
    alpha: f32,
    last: f32,
}

impl Lowpass {
    pub fn new(cutoff: f32, sample_rate: f32) -> Self {
        let dt = 1.0 / sample_rate;
        let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff.max(1.0));
        Self {
            alpha: dt / (rc + dt),
            last: 0.0,
        }
    }
}

impl Effect for Lowpass {
    fn process(&mut self, sample: f32) -> f32 {
        self.last += self.alpha * (sample - self.last);
        self.last
    }
}

pub struct Highpass {
    alpha: f32,
    last_in: f32,
    last_out: f32,
}

impl Highpass {
    pub fn new(cutoff: f32, sample_rate: f32) -> Self {
        let dt = 1.0 / sample_rate;
        let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff.max(1.0));
        Self {
            alpha: rc / (rc + dt),
            last_in: 0.0,
            last_out: 0.0,
        }
    }
}

impl Effect for Highpass {
    fn process(&mut self, sample: f32) -> f32 {
        let out = self.alpha * (self.last_out + sample - self.last_in);
        self.last_in = sample;
        self.last_out = out;
        out
    }
}

pub struct Distortion {
    drive: Smoothed,
    mix: Smoothed,
}

impl Distortion {
    pub const PARAM_DRIVE: u8 = 0;
    pub const PARAM_MIX: u8 = 1;

    pub fn new(drive: f32, mix: f32, sample_rate: f32) -> Self {
        Self {
            drive: Smoothed::new(drive.max(0.0), 0.01, sample_rate),
            mix: Smoothed::new(mix.clamp(0.0, 1.0), 0.01, sample_rate),
        }
    }
}

impl Effect for Distortion {
    fn process(&mut self, sample: f32) -> f32 {
        let drive = self.drive.next();
        let mix = self.mix.next();
        let wet = (sample * drive).tanh();
        sample * (1.0 - mix) + wet * mix
    }

    fn set_param(&mut self, idx: u8, value: f32) {
        match idx {
            Self::PARAM_DRIVE => self.drive.set_target(value.max(0.0)),
            Self::PARAM_MIX => self.mix.set_target(value.clamp(0.0, 1.0)),
            _ => {}
        }
    }
}

pub struct Tremolo {
    depth: f32,
    phase_inc: f32,
    phase: f32,
}

impl Tremolo {
    pub fn new(rate: f32, depth: f32, sample_rate: f32) -> Self {
        Self {
            depth: depth.clamp(0.0, 1.0),
            phase_inc: rate.max(0.0) / sample_rate,
            phase: 0.0,
        }
    }
}

impl Effect for Tremolo {
    fn process(&mut self, sample: f32) -> f32 {
        let lfo = (self.phase * 2.0 * std::f32::consts::PI).sin();
        // depth=0 → amp=1 always; depth=1 → amp swings 0..1
        let amp = 1.0 - self.depth * 0.5 * (1.0 - lfo);
        self.phase += self.phase_inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        sample * amp
    }
}

pub struct Bitcrusher {
    levels: f32,
}

impl Bitcrusher {
    pub fn new(bits: f32) -> Self {
        let bits = bits.clamp(1.0, 24.0);
        Self {
            levels: 2.0_f32.powf(bits - 1.0),
        }
    }
}

impl Effect for Bitcrusher {
    fn process(&mut self, sample: f32) -> f32 {
        (sample * self.levels).round() / self.levels
    }
}
