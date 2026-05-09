pub trait Effect {
    fn process(&mut self, sample: f32) -> f32;
}

pub struct Gain {
    pub level: f32,
}

impl Gain {
    pub fn new(level: f32) -> Self {
        Self { level }
    }
}

impl Effect for Gain {
    fn process(&mut self, sample: f32) -> f32 {
        sample * self.level
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
    drive: f32,
    mix: f32,
}

impl Distortion {
    pub fn new(drive: f32, mix: f32) -> Self {
        Self {
            drive: drive.max(0.0),
            mix: mix.clamp(0.0, 1.0),
        }
    }
}

impl Effect for Distortion {
    fn process(&mut self, sample: f32) -> f32 {
        let wet = (sample * self.drive).tanh();
        sample * (1.0 - self.mix) + wet * self.mix
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
