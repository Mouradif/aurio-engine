use crate::{
    frame::Frame,
    instrument::Instrument,
    midi::{NoteEvent, midi_to_freq},
    param::Param,
};

/// The tonewheel generator: 91 free-running wheels, one per semitone from C1
/// (32.7 Hz, MIDI 24) up. Keys tap wheels; the wheels themselves never stop
/// or reset, which is why repeated notes and shared partials on a real
/// Hammond stay phase-coherent.
const NUM_WHEELS: usize = 91;
const LOWEST_WHEEL_MIDI: i32 = 24;

/// Drawbar pitch offsets in semitones from the played key
/// (16', 5-1/3', 8', 4', 2-2/3', 2', 1-3/5', 1-1/3', 1').
const DRAWBAR_OFFSETS: [i32; 9] = [-12, 7, 0, 12, 19, 24, 28, 31, 36];

/// Percussion borrows one of the drawbar partials: "second" is the 4' (2x)
/// tap, "third" the 2-2/3' (3x) tap.
const PERC_SECOND_IDX: usize = 3;
const PERC_THIRD_IDX: usize = 4;
/// The 1' drawbar loses its key contact to the percussion circuit.
const ONE_FOOT_IDX: usize = 8;

/// Organs don't have a player-facing envelope — keys gate the tonewheels
/// almost instantly. These short ramps only keep the gate from popping.
const ATTACK_S: f32 = 0.004;
const RELEASE_S: f32 = 0.010;

/// Key-click noise burst decay.
const CLICK_TAU_S: f32 = 0.004;

/// Wheels whose output bleeds into everything when `leakage` is up — a
/// spread of the generator so the whine is broadband, like a worn B3.
const LEAK_WHEELS: [usize; 7] = [4, 17, 30, 43, 56, 69, 82];

/// Generator flutter rate in Hz when `wobble` is up (worn driveshaft).
const FLUTTER_HZ: f32 = 0.35;

/// Map a tapped pitch (key + drawbar offset) onto a wheel, folding back into
/// the generator's range an octave at a time — the top-end "foldback" is
/// part of the classic sound.
fn wheel_for(pitch: u8, offset: i32) -> usize {
    let mut w = pitch as i32 + offset - LOWEST_WHEEL_MIDI;
    while w < 0 {
        w += 12;
    }
    while w >= NUM_WHEELS as i32 {
        w -= 12;
    }
    w as usize
}

/// Registration value (0..8, continuous so a CC can sweep it) to linear
/// gain: 8 is unity, each step down -3 dB, hard-off below a threshold.
fn drawbar_gain(setting: f32) -> f32 {
    if setting < 0.05 {
        return 0.0;
    }
    let db = (setting.min(8.0) - 8.0) * 3.0;
    10.0_f32.powf(db / 20.0)
}

#[derive(Clone, Copy)]
pub struct Percussion {
    /// Index into the drawbar taps of the partial the percussion strikes.
    pub harmonic_idx: usize,
    /// Exponential decay time constant in seconds.
    pub decay: f32,
    pub level: f32,
}

impl Percussion {
    pub fn new(harmonic: u8, decay: f32, level: f32) -> Self {
        Self {
            harmonic_idx: if harmonic == 3 {
                PERC_THIRD_IDX
            } else {
                PERC_SECOND_IDX
            },
            decay: decay.max(0.01),
            level,
        }
    }
}

/// The scanner vibrato/chorus. Vibrato modulates the whole generator's
/// pitch; chorus mixes the modulated signal with the dry one (the classic
/// C1-C3 shimmer comes from that beating, not from deeper vibrato).
#[derive(Clone, Copy)]
pub struct VibratoChorus {
    /// Scanner speed in Hz (the real scanner runs at ~6.9 Hz).
    pub rate: f32,
    /// Pitch swing in cents.
    pub depth_cents: f32,
    /// Dry proportion blended back in: 0.0 = pure vibrato (V1-V3),
    /// 0.5 = equal chorus blend (C1-C3).
    pub chorus_mix: f32,
}

impl VibratoChorus {
    /// The six-position switch: V1/V2/V3 and C1/C2/C3.
    pub fn from_mode(mode: &str) -> Option<Self> {
        let (depth_cents, chorus_mix) = match mode.to_ascii_uppercase().as_str() {
            "V1" => (3.0, 0.0),
            "V2" => (6.0, 0.0),
            "V3" => (9.0, 0.0),
            "C1" => (3.0, 0.5),
            "C2" => (6.0, 0.5),
            "C3" => (9.0, 0.5),
            _ => return None,
        };
        Some(Self {
            rate: 6.9,
            depth_cents,
            chorus_mix,
        })
    }
}

/// The vintage-character block: all default to 0.0 (a clean, new organ).
#[derive(Clone, Copy, Default)]
pub struct OrganCharacter {
    /// Tonewheel crosstalk: a faint broadband whine from the generator that
    /// is always present, keys down or not.
    pub leakage: f32,
    /// Preamp saturation. Applied *after* the swell gain, so digging into
    /// the expression pedal drives it harder, like the real amp stage.
    pub drive: f32,
    /// Generator age: static per-wheel detune plus a slow global flutter.
    pub wobble: f32,
}

enum VoicePhase {
    Attack,
    Sustain,
    Release,
    Done,
}

struct Voice {
    pitch: u8,
    velocity: f32,
    /// Wheel index tapped by each drawbar for this key.
    wheels: [usize; 9],
    phase: VoicePhase,
    level: f32,
    /// Key-click noise burst level (retriggered on release too).
    click_env: f32,
    /// Percussion strike level; 0.0 when this voice didn't trigger it.
    perc_env: f32,
    rng_state: u32,
}

impl Voice {
    fn new(pitch: u8, velocity: u8, click: f32, perc_triggered: bool, perc_level: f32) -> Self {
        let mut wheels = [0; 9];
        for (w, offset) in wheels.iter_mut().zip(DRAWBAR_OFFSETS.iter()) {
            *w = wheel_for(pitch, *offset);
        }
        Self {
            pitch,
            velocity: (velocity.clamp(1, 127) as f32) / 127.0,
            wheels,
            phase: VoicePhase::Attack,
            level: 0.0,
            click_env: click,
            perc_env: if perc_triggered { perc_level } else { 0.0 },
            rng_state: 0x9e3779b9 ^ (pitch as u32).wrapping_mul(0x85eb_ca6b),
        }
    }

    fn release(&mut self, click: f32) {
        if !matches!(self.phase, VoicePhase::Done) {
            self.phase = VoicePhase::Release;
            self.click_env = self.click_env.max(click * 0.6);
        }
    }

    fn is_held(&self) -> bool {
        matches!(self.phase, VoicePhase::Attack | VoicePhase::Sustain)
    }

    fn is_done(&self) -> bool {
        matches!(self.phase, VoicePhase::Done)
    }

    fn noise(&mut self) -> f32 {
        let mut x = self.rng_state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng_state = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    /// One mono sample for this voice, tapping the shared wheel banks.
    /// `gains` is the per-frame evaluated drawbar gain set; `chorus_mix`
    /// blends the dry bank in for the C1-C3 modes.
    #[allow(clippy::too_many_arguments)]
    fn next_sample(
        &mut self,
        gains: &[f32; 9],
        perc: Option<&Percussion>,
        wet_phases: &[f32; NUM_WHEELS],
        dry_phases: &[f32; NUM_WHEELS],
        chorus_mix: f32,
        sample_rate: f32,
    ) -> f32 {
        let dt = 1.0 / sample_rate;
        let mut out = 0.0;

        for (i, &g) in gains.iter().enumerate() {
            let mut tap = g;
            if let Some(p) = perc
                && i == p.harmonic_idx
            {
                tap += self.perc_env;
            }
            if tap > 0.0 {
                let w = self.wheels[i];
                let wet = (wet_phases[w] * std::f32::consts::TAU).sin();
                let s = if chorus_mix > 0.0 {
                    let dry = (dry_phases[w] * std::f32::consts::TAU).sin();
                    wet * (1.0 - chorus_mix) + dry * chorus_mix
                } else {
                    wet
                };
                out += tap * s;
            }
        }

        if self.perc_env > 0.0
            && let Some(p) = perc
        {
            self.perc_env *= (-dt / p.decay).exp();
            if self.perc_env < 1e-4 {
                self.perc_env = 0.0;
            }
        }

        if self.click_env > 1e-4 {
            out += self.noise() * self.click_env;
            self.click_env *= (-dt / CLICK_TAU_S).exp();
        }

        match self.phase {
            VoicePhase::Attack => {
                self.level += dt / ATTACK_S;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.phase = VoicePhase::Sustain;
                }
            }
            VoicePhase::Release => {
                self.level -= dt / RELEASE_S;
                if self.level <= 0.0 {
                    self.level = 0.0;
                    self.phase = VoicePhase::Done;
                }
            }
            VoicePhase::Sustain | VoicePhase::Done => {}
        }

        out * self.level * self.velocity
    }
}

/// A tonewheel-organ instrument: a shared free-running 91-wheel generator
/// tapped by nine drawbars per key, with foldback, key click, single-trigger
/// percussion, scanner vibrato/chorus, and a vintage-character block
/// (leakage, preamp drive, generator wobble).
pub struct Organ {
    pub id: usize,
    /// Live drawbar registrations (0..8 each), addressable via `set_param`
    /// with indices `PARAM_DRAWBAR_BASE..PARAM_DRAWBAR_BASE+9`.
    drawbars: [Param; 9],
    /// Swell: master gain, addressable via `set_param(PARAM_GAIN)`.
    gain: Param,
    click: f32,
    percussion: Option<Percussion>,
    vibrato: Option<VibratoChorus>,
    character: OrganCharacter,
    /// Per-wheel frequency, including the static `wobble` detune.
    wheel_freqs: [f32; NUM_WHEELS],
    /// Vibrato-modulated bank (what you normally hear).
    wet_phases: [f32; NUM_WHEELS],
    /// Unmodulated bank, blended in by the chorus modes.
    dry_phases: [f32; NUM_WHEELS],
    vibrato_phase: f32,
    flutter_phase: f32,
    /// Per-frame drawbar gain scratch (no allocation on the audio thread).
    gain_scratch: [f32; 9],
    rng_state: u32,
    voices: Vec<Voice>,
}

impl Organ {
    pub const PARAM_DRAWBAR_BASE: u8 = 0;
    pub const PARAM_GAIN: u8 = 9;

    pub fn new(
        id: usize,
        drawbars: [Param; 9],
        gain: Param,
        click: f32,
        percussion: Option<Percussion>,
        vibrato: Option<VibratoChorus>,
        character: OrganCharacter,
    ) -> Self {
        // Static per-wheel detune scales with `wobble`: worn wheels don't sit
        // exactly on pitch. Seeded per wheel so it's stable across runs.
        let mut rng: u32 = 0xc0ffee42 ^ id as u32;
        let mut next_rand = move || {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            (rng as f32 / u32::MAX as f32) * 2.0 - 1.0
        };
        let mut wheel_freqs = [0.0; NUM_WHEELS];
        for (i, f) in wheel_freqs.iter_mut().enumerate() {
            let base = midi_to_freq((LOWEST_WHEEL_MIDI + i as i32) as u8);
            *f = base * (1.0 + character.wobble.clamp(0.0, 1.0) * 0.0008 * next_rand());
        }

        Self {
            id,
            drawbars,
            gain,
            click: click.clamp(0.0, 1.0) * 0.15,
            percussion,
            vibrato,
            character,
            wheel_freqs,
            wet_phases: [0.0; NUM_WHEELS],
            dry_phases: [0.0; NUM_WHEELS],
            vibrato_phase: 0.0,
            flutter_phase: 0.0,
            gain_scratch: [0.0; 9],
            rng_state: 0x51ab_beef,
            voices: Vec::new(),
        }
    }

    fn noise(&mut self) -> f32 {
        let mut x = self.rng_state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng_state = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

impl Instrument for Organ {
    fn id(&self) -> usize {
        self.id
    }

    fn set_param(&mut self, idx: u8, value: f32) {
        match idx {
            i if i < 9 => self.drawbars[i as usize].set_target(value.clamp(0.0, 8.0)),
            Self::PARAM_GAIN => self.gain.set_target(value.max(0.0)),
            _ => {}
        }
    }

    fn process_event(&mut self, event: &NoteEvent) {
        match event {
            NoteEvent::NoteOn { pitch, velocity } => {
                // Percussion is single-trigger: it only fires when no other
                // key is held, like the real percussion charge circuit.
                let perc_triggered =
                    self.percussion.is_some() && !self.voices.iter().any(|v| v.is_held());
                let perc_level = self.percussion.map(|p| p.level).unwrap_or(0.0);
                self.voices.retain(|v| v.pitch != *pitch);
                self.voices.push(Voice::new(
                    *pitch,
                    *velocity,
                    self.click,
                    perc_triggered,
                    perc_level,
                ));
            }
            NoteEvent::NoteOff { pitch } => {
                for voice in &mut self.voices {
                    if voice.pitch == *pitch {
                        voice.release(self.click);
                    }
                }
            }
        }
    }

    fn next_sample(&mut self, sample_rate: f32) -> Frame {
        let dt = 1.0 / sample_rate;

        // Slow generator flutter (wobble) affects both banks; the scanner
        // vibrato affects only the wet one. Both are organ-wide: one LFO
        // each, evaluated once per frame.
        let flutter_mul = if self.character.wobble > 0.0 {
            let s = (self.flutter_phase * std::f32::consts::TAU).sin();
            self.flutter_phase += FLUTTER_HZ * dt;
            if self.flutter_phase >= 1.0 {
                self.flutter_phase -= 1.0;
            }
            1.0 + s * self.character.wobble * 0.0005
        } else {
            1.0
        };

        let (wet_mul, chorus_mix) = match self.vibrato {
            Some(v) => {
                let s = (self.vibrato_phase * std::f32::consts::TAU).sin();
                self.vibrato_phase += v.rate * dt;
                if self.vibrato_phase >= 1.0 {
                    self.vibrato_phase -= 1.0;
                }
                // cents → ratio, linearized (depths are a few cents).
                let m = 1.0 + s * v.depth_cents * (std::f32::consts::LN_2 / 1200.0);
                (flutter_mul * m, v.chorus_mix)
            }
            None => (flutter_mul, 0.0),
        };

        // Advance the free-running generator — every wheel, every frame,
        // whether or not a key taps it.
        for (i, &freq) in self.wheel_freqs.iter().enumerate() {
            self.wet_phases[i] += freq * wet_mul * dt;
            if self.wet_phases[i] >= 1.0 {
                self.wet_phases[i] -= 1.0;
            }
            self.dry_phases[i] += freq * flutter_mul * dt;
            if self.dry_phases[i] >= 1.0 {
                self.dry_phases[i] -= 1.0;
            }
        }

        // Evaluate the live drawbar registrations into linear gains.
        for (i, p) in self.drawbars.iter_mut().enumerate() {
            self.gain_scratch[i] = drawbar_gain(p.next(sample_rate));
        }
        // Percussion steals the 1' drawbar's key contact.
        if self.percussion.is_some() {
            self.gain_scratch[ONE_FOOT_IDX] = 0.0;
        }

        let mut output = 0.0;
        for voice in &mut self.voices {
            output += voice.next_sample(
                &self.gain_scratch,
                self.percussion.as_ref(),
                &self.wet_phases,
                &self.dry_phases,
                chorus_mix,
                sample_rate,
            );
        }
        self.voices.retain(|v| !v.is_done());

        // Tonewheel leakage: a faint tap on a spread of wheels plus a wisp of
        // noise, present even with no keys down.
        let leakage = self.character.leakage;
        if leakage > 0.0 {
            let mut leak = 0.0;
            for &w in LEAK_WHEELS.iter() {
                leak += (self.wet_phases[w] * std::f32::consts::TAU).sin();
            }
            output += leak * leakage * 0.004 + self.noise() * leakage * 0.0015;
        }

        // Percussion drops the overall level slightly while switched on.
        let perc_dip = if self.percussion.is_some() { 0.85 } else { 1.0 };

        // A full 888888888 registration sums to ~4.9x; pull it back so hot
        // registrations don't clip before the drive/gain stages.
        let pre = output * 0.25 * perc_dip * self.gain.next(sample_rate);

        // Preamp drive sits after the swell gain so the pedal pushes it.
        let drive = self.character.drive;
        let out = if drive > 0.0 {
            let k = 1.0 + drive * 4.0;
            (pre * k).tanh() / (1.0 + drive * 1.5)
        } else {
            pre
        };

        Frame::mono(out)
    }
}
