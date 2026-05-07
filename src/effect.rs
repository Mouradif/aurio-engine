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
