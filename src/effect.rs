use crate::frame::Frame;
use crate::param::Param;

/// Audio-thread effect node. `process` runs once per stereo frame; `set_param`
/// applies a live parameter update (default no-op for effects without
/// controllable params).
pub trait Effect {
    fn process(&mut self, frame: Frame) -> Frame;
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
    level: Param,
    sample_rate: f32,
}

impl Gain {
    pub const PARAM_LEVEL: u8 = 0;

    pub fn new(level: Param, sample_rate: f32) -> Self {
        Self { level, sample_rate }
    }
}

impl Effect for Gain {
    fn process(&mut self, frame: Frame) -> Frame {
        frame * self.level.next(self.sample_rate)
    }

    fn set_param(&mut self, idx: u8, value: f32) {
        if idx == Self::PARAM_LEVEL {
            self.level.set_target(value);
        }
    }
}

/// Equal-power stereo panner. `amount` runs -1.0 (hard left) .. +1.0 (hard
/// right), 0.0 = centered. Uses the constant-power sin/cos law so a centered
/// signal isn't louder than a panned one.
pub struct Pan {
    amount: Param,
    sample_rate: f32,
}

impl Pan {
    pub const PARAM_AMOUNT: u8 = 0;

    pub fn new(amount: Param, sample_rate: f32) -> Self {
        Self {
            amount,
            sample_rate,
        }
    }
}

impl Effect for Pan {
    fn process(&mut self, frame: Frame) -> Frame {
        // Map amount -1..1 to an angle 0..π/2.
        let amount = self.amount.next(self.sample_rate).clamp(-1.0, 1.0);
        let angle = (amount + 1.0) * 0.25 * std::f32::consts::PI;
        Frame::new(frame.l * angle.cos(), frame.r * angle.sin())
    }

    fn set_param(&mut self, idx: u8, value: f32) {
        if idx == Self::PARAM_AMOUNT {
            self.amount.set_target(value.clamp(-1.0, 1.0));
        }
    }
}

/// A feedback delay (the project's "reverb"). The delay buffer is allocated at
/// a fixed maximum length up front so `delay` can be modulated without ever
/// reallocating on the audio thread: the read tap is a fractional position
/// `delay_samples` behind the write head, linearly interpolated. Sweeping the
/// delay this way gives chorus/flanger-style motion.
pub struct Reverb {
    buffer: Vec<Frame>,
    write_pos: usize,
    delay: Param,
    feedback: Param,
    mix: Param,
    sample_rate: f32,
}

impl Reverb {
    pub const PARAM_DELAY: u8 = 0;
    pub const PARAM_FEEDBACK: u8 = 1;
    pub const PARAM_MIX: u8 = 2;

    /// Maximum delay time the buffer can hold; modulated delay is clamped here.
    pub const MAX_DELAY_SECONDS: f32 = 2.0;

    pub fn new(delay: Param, feedback: Param, mix: Param, sample_rate: f32) -> Self {
        let buffer_size = ((Self::MAX_DELAY_SECONDS * sample_rate) as usize).max(1);
        Self {
            buffer: vec![Frame::SILENCE; buffer_size],
            write_pos: 0,
            delay,
            feedback,
            mix,
            sample_rate,
        }
    }

    /// Read the buffer at a fractional number of samples behind the write head,
    /// with linear interpolation between the two surrounding slots.
    fn read_delayed(&self, delay_samples: f32) -> Frame {
        let len = self.buffer.len();
        // Position the read tap `delay_samples` behind the write head.
        let read_pos = self.write_pos as f32 - delay_samples;
        // Wrap into [0, len).
        let read_pos = read_pos.rem_euclid(len as f32);
        let i0 = read_pos.floor() as usize % len;
        let i1 = (i0 + 1) % len;
        let frac = read_pos - read_pos.floor();
        self.buffer[i0] * (1.0 - frac) + self.buffer[i1] * frac
    }
}

impl Effect for Reverb {
    fn process(&mut self, frame: Frame) -> Frame {
        let len = self.buffer.len();
        let delay = self
            .delay
            .next(self.sample_rate)
            .clamp(0.0, Self::MAX_DELAY_SECONDS);
        let feedback = self.feedback.next(self.sample_rate).clamp(0.0, 0.99);
        let mix = self.mix.next(self.sample_rate).clamp(0.0, 1.0);

        // Delay in samples, kept at least 1 sample below the buffer length so
        // the read tap can't collide with the write head.
        let delay_samples = (delay * self.sample_rate).clamp(0.0, (len - 1) as f32);

        let delayed = self.read_delayed(delay_samples);
        let wet = frame + delayed * feedback;

        self.buffer[self.write_pos] = wet;
        self.write_pos = (self.write_pos + 1) % len;

        frame * (1.0 - mix) + wet * mix
    }

    fn set_param(&mut self, idx: u8, value: f32) {
        match idx {
            Self::PARAM_DELAY => self
                .delay
                .set_target(value.clamp(0.0, Self::MAX_DELAY_SECONDS)),
            Self::PARAM_FEEDBACK => self.feedback.set_target(value.clamp(0.0, 0.99)),
            Self::PARAM_MIX => self.mix.set_target(value.clamp(0.0, 1.0)),
            _ => {}
        }
    }
}

/// One-pole RC low-pass. The filter coefficient `alpha` is derived from the
/// cutoff each frame, so a modulated cutoff sweeps the filter.
pub struct Lowpass {
    cutoff: Param,
    dt: f32,
    sample_rate: f32,
    last: Frame,
}

impl Lowpass {
    pub const PARAM_CUTOFF: u8 = 0;

    pub fn new(cutoff: Param, sample_rate: f32) -> Self {
        Self {
            cutoff,
            dt: 1.0 / sample_rate,
            sample_rate,
            last: Frame::SILENCE,
        }
    }

    fn alpha(&mut self) -> f32 {
        let cutoff = self.cutoff.next(self.sample_rate).max(1.0);
        let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff);
        self.dt / (rc + self.dt)
    }
}

impl Effect for Lowpass {
    fn process(&mut self, frame: Frame) -> Frame {
        let alpha = self.alpha();
        self.last += (frame + self.last * -1.0) * alpha;
        self.last
    }

    fn set_param(&mut self, idx: u8, value: f32) {
        if idx == Self::PARAM_CUTOFF {
            self.cutoff.set_target(value);
        }
    }
}

/// One-pole RC high-pass. Like `Lowpass`, `alpha` is recomputed from the
/// cutoff each frame so the cutoff can be modulated.
pub struct Highpass {
    cutoff: Param,
    dt: f32,
    sample_rate: f32,
    last_in: Frame,
    last_out: Frame,
}

impl Highpass {
    pub const PARAM_CUTOFF: u8 = 0;

    pub fn new(cutoff: Param, sample_rate: f32) -> Self {
        Self {
            cutoff,
            dt: 1.0 / sample_rate,
            sample_rate,
            last_in: Frame::SILENCE,
            last_out: Frame::SILENCE,
        }
    }

    fn alpha(&mut self) -> f32 {
        let cutoff = self.cutoff.next(self.sample_rate).max(1.0);
        let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff);
        rc / (rc + self.dt)
    }
}

impl Effect for Highpass {
    fn process(&mut self, frame: Frame) -> Frame {
        let alpha = self.alpha();
        let out = (self.last_out + frame + self.last_in * -1.0) * alpha;
        self.last_in = frame;
        self.last_out = out;
        out
    }

    fn set_param(&mut self, idx: u8, value: f32) {
        if idx == Self::PARAM_CUTOFF {
            self.cutoff.set_target(value);
        }
    }
}

pub struct Distortion {
    drive: Param,
    mix: Param,
    sample_rate: f32,
}

impl Distortion {
    pub const PARAM_DRIVE: u8 = 0;
    pub const PARAM_MIX: u8 = 1;

    pub fn new(drive: Param, mix: Param, sample_rate: f32) -> Self {
        Self {
            drive,
            mix,
            sample_rate,
        }
    }
}

impl Effect for Distortion {
    fn process(&mut self, frame: Frame) -> Frame {
        let drive = self.drive.next(self.sample_rate);
        let mix = self.mix.next(self.sample_rate);
        frame.map(|s| {
            let wet = (s * drive).tanh();
            s * (1.0 - mix) + wet * mix
        })
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
    rate: Param,
    depth: Param,
    sample_rate: f32,
    phase: f32,
}

impl Tremolo {
    pub const PARAM_RATE: u8 = 0;
    pub const PARAM_DEPTH: u8 = 1;

    pub fn new(rate: Param, depth: Param, sample_rate: f32) -> Self {
        Self {
            rate,
            depth,
            sample_rate,
            phase: 0.0,
        }
    }
}

impl Effect for Tremolo {
    fn process(&mut self, frame: Frame) -> Frame {
        let rate = self.rate.next(self.sample_rate).max(0.0);
        let depth = self.depth.next(self.sample_rate).clamp(0.0, 1.0);
        let lfo = (self.phase * 2.0 * std::f32::consts::PI).sin();
        // depth=0 → amp=1 always; depth=1 → amp swings 0..1
        let amp = 1.0 - depth * 0.5 * (1.0 - lfo);
        self.phase += rate / self.sample_rate;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        frame * amp
    }

    fn set_param(&mut self, idx: u8, value: f32) {
        match idx {
            Self::PARAM_RATE => self.rate.set_target(value.max(0.0)),
            Self::PARAM_DEPTH => self.depth.set_target(value.clamp(0.0, 1.0)),
            _ => {}
        }
    }
}

pub struct Bitcrusher {
    bits: Param,
    sample_rate: f32,
}

impl Bitcrusher {
    pub const PARAM_BITS: u8 = 0;

    pub fn new(bits: Param, sample_rate: f32) -> Self {
        Self { bits, sample_rate }
    }
}

impl Effect for Bitcrusher {
    fn process(&mut self, frame: Frame) -> Frame {
        let bits = self.bits.next(self.sample_rate).clamp(1.0, 24.0);
        let levels = 2.0_f32.powf(bits - 1.0);
        frame.map(|s| (s * levels).round() / levels)
    }

    fn set_param(&mut self, idx: u8, value: f32) {
        if idx == Self::PARAM_BITS {
            self.bits.set_target(value.clamp(1.0, 24.0));
        }
    }
}

/// A stereo delay line. When `glide` is set, the read tap's delay time is
/// *eased* toward its target each sample, so changing the time (constant, MIDI
/// CC, or LFO) makes the read head move at a different rate than the write head
/// — resampling the buffered audio into the characteristic tape Doppler pitch
/// bend. With `glide = 0` (the default) the time tracks its target directly and
/// there's no Doppler.
///
/// Per-channel time lets the left and right taps differ; `feedback` repeats the
/// echoes, and `ping_pong` routes each channel's feedback into the *other*
/// channel so echoes bounce L↔R. Time params are in **milliseconds**.
pub struct Delay {
    buffer: Vec<Frame>,
    write_pos: usize,
    /// Per-channel target delay time in ms (constant / MIDI / modulator).
    time_l: Param,
    time_r: Param,
    /// Internally glided current delay in *samples* per channel.
    current_l: f32,
    current_r: f32,
    /// One-pole coefficient for easing `current_*` toward target. 1.0 = no
    /// glide (instant, no Doppler); smaller = slower, more pitch bend.
    glide_coeff: f32,
    feedback: Param,
    mix: Param,
    ping_pong: bool,
    sample_rate: f32,
}

impl Delay {
    pub const PARAM_TIME_L: u8 = 0;
    pub const PARAM_TIME_R: u8 = 1;
    pub const PARAM_FEEDBACK: u8 = 2;
    pub const PARAM_MIX: u8 = 3;

    /// Maximum delay the buffer can hold; times are clamped to this.
    pub const MAX_DELAY_SECONDS: f32 = 4.0;

    /// Convert a glide *time constant* in seconds to the per-sample one-pole
    /// coefficient. `glide_seconds <= 0` means no glide → coefficient 1.0
    /// (the current delay snaps to its target every sample, no Doppler).
    fn glide_coeff(glide_seconds: f32, sample_rate: f32) -> f32 {
        if glide_seconds <= 0.0 {
            1.0
        } else {
            1.0 - (-1.0 / (glide_seconds * sample_rate)).exp()
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        time_l: Param,
        time_r: Param,
        feedback: Param,
        mix: Param,
        ping_pong: bool,
        glide_seconds: f32,
        initial_samples_l: f32,
        initial_samples_r: f32,
        sample_rate: f32,
    ) -> Self {
        let buffer_size = ((Self::MAX_DELAY_SECONDS * sample_rate) as usize).max(1);
        Self {
            buffer: vec![Frame::SILENCE; buffer_size],
            write_pos: 0,
            time_l,
            time_r,
            // Start already at the target so the first echo lands in time
            // rather than gliding up from zero.
            current_l: initial_samples_l,
            current_r: initial_samples_r,
            glide_coeff: Self::glide_coeff(glide_seconds, sample_rate),
            feedback,
            mix,
            ping_pong,
            sample_rate,
        }
    }

    /// Read one channel of the buffer at a fractional sample distance behind the
    /// write head, linearly interpolated. `pick` selects L or R from the frame.
    fn read_tap(&self, delay_samples: f32, pick: fn(Frame) -> f32) -> f32 {
        let len = self.buffer.len();
        let read_pos = (self.write_pos as f32 - delay_samples).rem_euclid(len as f32);
        let i0 = read_pos.floor() as usize % len;
        let i1 = (i0 + 1) % len;
        let frac = read_pos - read_pos.floor();
        pick(self.buffer[i0]) * (1.0 - frac) + pick(self.buffer[i1]) * frac
    }
}

impl Effect for Delay {
    fn process(&mut self, frame: Frame) -> Frame {
        let len = self.buffer.len();
        let max_samples = (len - 1) as f32;

        // Targets in samples, glided toward for the analog character.
        let target_l = (self.time_l.next(self.sample_rate) * 0.001 * self.sample_rate)
            .clamp(1.0, max_samples);
        let target_r = (self.time_r.next(self.sample_rate) * 0.001 * self.sample_rate)
            .clamp(1.0, max_samples);
        self.current_l += (target_l - self.current_l) * self.glide_coeff;
        self.current_r += (target_r - self.current_r) * self.glide_coeff;

        let feedback = self.feedback.next(self.sample_rate).clamp(0.0, 0.95);
        let mix = self.mix.next(self.sample_rate).clamp(0.0, 1.0);

        // Read the delayed signal for each channel.
        let delayed_l = self.read_tap(self.current_l, |f| f.l);
        let delayed_r = self.read_tap(self.current_r, |f| f.r);

        // Write input plus feedback into the buffer. Ping-pong cross-feeds each
        // channel's echo into the opposite channel.
        let (fb_l, fb_r) = if self.ping_pong {
            (delayed_r * feedback, delayed_l * feedback)
        } else {
            (delayed_l * feedback, delayed_r * feedback)
        };
        self.buffer[self.write_pos] = Frame::new(frame.l + fb_l, frame.r + fb_r);
        self.write_pos = (self.write_pos + 1) % len;

        // Dry/wet blend.
        Frame::new(
            frame.l * (1.0 - mix) + delayed_l * mix,
            frame.r * (1.0 - mix) + delayed_r * mix,
        )
    }

    fn set_param(&mut self, idx: u8, value: f32) {
        match idx {
            Self::PARAM_TIME_L => self.time_l.set_target(value),
            Self::PARAM_TIME_R => self.time_r.set_target(value),
            Self::PARAM_FEEDBACK => self.feedback.set_target(value.clamp(0.0, 0.95)),
            Self::PARAM_MIX => self.mix.set_target(value.clamp(0.0, 1.0)),
            _ => {}
        }
    }
}
