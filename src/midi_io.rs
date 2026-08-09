//! Hardware MIDI I/O: sending phrases out to external gear, and taking notes
//! in from a controller.
//!
//! Both directions are graph nodes so they compose with everything else — a
//! `MidiOutNode` is a valid `song:track()` target (the same phrases that drive
//! a `Synth` can drive a hardware synth instead), and a `MidiInNode` feeds note
//! events to the instruments it's connected to.
//!
//! Neither node makes sound itself. They sit in the graph as `Source`s that
//! render silence, because the graph's edge model is strictly `Frame` buffers
//! and their real payload is events, not audio.

use crate::frame::Frame;
use crate::instrument::Instrument;
use crate::midi::NoteEvent;
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Outbound queue depth. Note on/off pairs at musical rates use a tiny
/// fraction of this; the headroom is for dense chords landing on one beat.
const OUT_QUEUE: usize = 1024;

/// How long the sender thread sleeps when it has nothing due. Short enough
/// that a note lands within about a millisecond of its sample deadline, long
/// enough that the thread costs nothing while idle.
const SENDER_TICK: std::time::Duration = std::time::Duration::from_micros(500);

/// A MIDI message bound for a hardware port, tagged with the sample it should
/// be sent at. The sender thread compares `sample` against the audio clock.
#[derive(Clone, Copy, Debug)]
pub struct TimedMessage {
    pub sample: u64,
    /// A whole MIDI message. Note on/off are 3 bytes; the array is fixed so
    /// the queue stays allocation-free.
    pub bytes: [u8; 3],
}

/// One outbound Control Change stream: a CC number driven by a modulator.
///
/// This is how gear that has no note input is played. A Kaossilator, for
/// instance, makes no sound from note on/off at all — its pad position *is*
/// the instrument, and the pad is addressed by CC.
#[derive(Clone, Debug)]
pub struct CcAutomation {
    pub cc: u8,
    pub modulator: crate::param::Modulator,
}

/// Declaration for a `song:midi_out(...)` node.
#[derive(Clone, Debug)]
pub struct MidiOutSpec {
    /// Case-insensitive substring matched against the port name, so
    /// `port = "Port 1"` finds "U6MIDI Pro Port 1". `None` takes the first
    /// available output port.
    pub port: Option<String>,
    /// MIDI channel, 1-16 in Lua, stored 0-indexed here.
    pub channel: u8,
    /// CC streams sent continuously while the song plays.
    pub controls: Vec<CcAutomation>,
    /// Constant CCs sent once at startup, for setting a level and leaving it.
    pub initial: Vec<(u8, u8)>,
}

/// Declaration for a `song:midi_in(...)` node.
#[derive(Clone, Debug)]
pub struct MidiInSpec {
    /// Matched like `MidiOutSpec::port`, against input ports.
    pub port: Option<String>,
    /// `None` listens on every channel; `Some(n)` filters to channel n
    /// (0-indexed here, 1-16 in Lua).
    pub channel: Option<u8>,
    /// Semitones added to every incoming pitch, letting a controller drive an
    /// instrument an octave or two away from where it's played.
    pub transpose: i32,
}

/// The graph node behind `song:midi_out(...)`.
///
/// It receives note events exactly like an instrument does — that's what makes
/// it usable as a track target — but instead of synthesising, it stamps each
/// one with the current sample and hands it to the sender thread. It renders
/// silence, so connecting it to an output is harmless but pointless.
///
/// It also carries any CC automation declared on the node, which is how gear
/// without a note input is played at all: a Kaossilator's pad, for instance,
/// responds to X/Y/gate CCs and ignores note on/off entirely. A node may do
/// both — notes to the voice engine, CCs sweeping a filter underneath.
pub struct MidiOutNode {
    id: usize,
    channel: u8,
    /// Shared so `midi_emit` can queue onto the same port from a Lua handler.
    /// The audio thread locks it uncontended in the common case; the only
    /// other claimant is a MIDI callback thread.
    producer: Arc<Mutex<HeapProd<TimedMessage>>>,
    /// Shared with the audio callback so outbound events carry the same clock
    /// the rest of the graph runs on.
    audio_counter: Arc<AtomicU64>,
    /// Set once when the queue overflows, so a stuck sender warns instead of
    /// spamming the console every block.
    warned_overflow: bool,
    /// Running CC automation, each with the last value sent so unchanged
    /// values aren't re-sent. MIDI is a 31250-baud serial link, and a CC
    /// resent every block would swamp it and delay the notes behind it.
    controls: Vec<(u8, crate::param::ModState, Option<u8>)>,
}

impl MidiOutNode {
    /// Build the node and start its sender thread. Returns the node; a port
    /// that can't be opened is reported by the spawning helper and leaves the
    /// node silently discarding messages, so one missing cable doesn't stop
    /// the song.
    pub fn new(
        id: usize,
        spec: &MidiOutSpec,
        audio_counter: Arc<AtomicU64>,
    ) -> Result<Self, String> {
        let rb = HeapRb::<TimedMessage>::new(OUT_QUEUE);
        let (mut producer, consumer) = rb.split();

        spawn_sender(spec.clone(), consumer, audio_counter.clone())?;

        // Constant CCs go out once, before anything else is queued, so the
        // gear is in a known state when the first note or sweep arrives.
        for (cc, value) in &spec.initial {
            let _ = producer.try_push(TimedMessage {
                sample: 0,
                bytes: [0xB0 | spec.channel, *cc & 0x7F, *value & 0x7F],
            });
        }
        let producer = Arc::new(Mutex::new(producer));

        let controls = spec
            .controls
            .iter()
            .map(|c| (c.cc, crate::param::ModState::new(c.modulator), None))
            .collect();

        Ok(Self {
            id,
            channel: spec.channel,
            producer,
            audio_counter,
            warned_overflow: false,
            controls,
        })
    }

    /// A handle Lua can emit through, addressing this node's open port.
    pub fn emitter(&self) -> MidiEmitter {
        MidiEmitter {
            channel: self.channel,
            producer: self.producer.clone(),
            audio_counter: self.audio_counter.clone(),
        }
    }

    fn push(&mut self, bytes: [u8; 3]) {
        let now = self.audio_counter.load(Ordering::Relaxed);
        self.push_at(now, bytes);
    }

    fn push_at(&mut self, sample: u64, bytes: [u8; 3]) {
        let msg = TimedMessage { sample, bytes };
        // `try_lock`, never `lock`: this runs on the audio thread, and blocking
        // on a lower-priority MIDI thread is how you get a dropout. The lock is
        // held only for a queue push, so losing the race is rare — and a lost
        // message here is a CC that will be re-sent on the next change anyway.
        let pushed = match self.producer.try_lock() {
            Ok(mut p) => p.try_push(msg).is_ok(),
            Err(_) => false,
        };
        if !pushed && !self.warned_overflow {
            self.warned_overflow = true;
            eprintln!(
                "[midi-out] socket {}: outbound queue full, dropping messages",
                self.id
            );
        }
    }
}

impl Instrument for MidiOutNode {
    fn id(&self) -> usize {
        self.id
    }

    fn process_event(&mut self, event: &NoteEvent) {
        match event {
            NoteEvent::NoteOn { pitch, velocity } => {
                self.push([0x90 | self.channel, *pitch & 0x7F, *velocity & 0x7F]);
            }
            NoteEvent::NoteOff { pitch } => {
                self.push([0x80 | self.channel, *pitch & 0x7F, 0]);
            }
        }
    }

    fn next_sample(&mut self, _sample_rate: f32) -> Frame {
        Frame::SILENCE
    }

    /// Renders silence, and doubles as the tick for outbound CC automation:
    /// the graph calls it once per block with the block length, which is the
    /// cadence a control stream wants. A CC is only sent when its 7-bit value
    /// actually changes, so a slow sweep costs a handful of messages a second
    /// rather than one per block.
    fn render_block(&mut self, out: &mut [Frame], sample_rate: f32) {
        out.fill(Frame::SILENCE);

        if self.controls.is_empty() {
            return;
        }

        // Indexed rather than iterated so the queue push (which borrows
        // `self.producer`) doesn't overlap the borrow of `self.controls`.
        // No allocation happens here — this runs on the audio thread.
        let now = self.audio_counter.load(Ordering::Relaxed);
        let channel = self.channel;
        for i in 0..self.controls.len() {
            let (cc, ref mut state, last) = self.controls[i];
            let value = state.next_block(sample_rate, out.len());
            let byte = value.clamp(0.0, 127.0).round() as u8;
            if last == Some(byte) {
                continue;
            }
            self.controls[i].2 = Some(byte);
            self.push_at(now, [0xB0 | channel, cc & 0x7F, byte]);
        }
    }
}

/// Open the output port and run the pump that drains `consumer` in time.
///
/// The thread exists because sending MIDI is a syscall and the audio callback
/// must not make one. It sleeps in short ticks, sending everything whose
/// deadline the audio clock has reached.
fn spawn_sender(
    spec: MidiOutSpec,
    mut consumer: HeapCons<TimedMessage>,
    audio_counter: Arc<AtomicU64>,
) -> Result<(), String> {
    use midir::MidiOutput;

    let midi_out = MidiOutput::new("aurio").map_err(|e| format!("MIDI output init failed: {e}"))?;
    let ports = midi_out.ports();
    if ports.is_empty() {
        return Err("no MIDI output ports available".to_string());
    }

    let port = match &spec.port {
        Some(want) => {
            let want_lc = want.to_lowercase();
            ports
                .iter()
                .find(|p| {
                    midi_out
                        .port_name(p)
                        .unwrap_or_default()
                        .to_lowercase()
                        .contains(&want_lc)
                })
                .cloned()
                .ok_or_else(|| {
                    let available: Vec<String> = ports
                        .iter()
                        .map(|p| midi_out.port_name(p).unwrap_or_default())
                        .collect();
                    format!(
                        "no MIDI output port matching '{}' (available: {})",
                        want,
                        available.join(", ")
                    )
                })?
        }
        None => ports[0].clone(),
    };

    let port_name = midi_out.port_name(&port).unwrap_or_else(|_| "?".into());
    let mut conn = midi_out
        .connect(&port, "aurio-output")
        .map_err(|e| format!("MIDI output connect failed: {e}"))?;
    println!("[midi-out] sending on: {}", port_name);

    let channel = spec.channel;
    let shutdown = shutdown_flag();

    std::thread::spawn(move || {
        // Track what's sounding so a stop mid-note doesn't leave the hardware
        // droning. Without this, Ctrl-C during a held bass note hangs it.
        let mut sounding = [false; 128];
        // Gate-style CCs (a Kaossilator's touch gate, say) hold sound on just
        // as a note-on does, but All Notes Off won't release them — that only
        // speaks to the note engine. Remember every CC left at a nonzero value
        // so shutdown can zero it.
        let mut cc_held = [false; 128];

        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }

            let now = audio_counter.load(Ordering::Relaxed);
            // Messages are queued in time order, so stop at the first one that
            // isn't due yet rather than scanning the whole queue.
            while consumer.first().is_some_and(|m| m.sample <= now) {
                let msg = consumer.try_pop().unwrap();
                let status = msg.bytes[0] & 0xF0;
                let pitch = (msg.bytes[1] & 0x7F) as usize;
                if status == 0x90 && msg.bytes[2] > 0 {
                    sounding[pitch] = true;
                } else if status == 0x80 || status == 0x90 {
                    sounding[pitch] = false;
                } else if status == 0xB0 {
                    // `pitch` is the CC number for a Control Change.
                    cc_held[pitch] = msg.bytes[2] > 0;
                }
                let _ = conn.send(&msg.bytes);
            }

            std::thread::sleep(SENDER_TICK);
        }

        // Explicit note-offs for anything still held, then All Notes Off as a
        // backstop for gear that ignores one or the other.
        for (pitch, held) in sounding.iter().enumerate() {
            if *held {
                let _ = conn.send(&[0x80 | channel, pitch as u8, 0]);
            }
        }
        // Zero every CC left raised, so a gate-driven pad stops with the song.
        // Skipped for CC 123 itself, which is sent next as the note backstop.
        for (cc, held) in cc_held.iter().enumerate() {
            if *held && cc != 123 {
                let _ = conn.send(&[0xB0 | channel, cc as u8, 0]);
            }
        }
        let _ = conn.send(&[0xB0 | channel, 123, 0]);
    });

    Ok(())
}

/// Process-wide stop signal for every sender thread, so shutdown silences all
/// connected gear rather than only the node that noticed first.
fn shutdown_flag() -> Arc<AtomicBool> {
    use std::sync::OnceLock;
    static FLAG: OnceLock<Arc<AtomicBool>> = OnceLock::new();
    FLAG.get_or_init(|| Arc::new(AtomicBool::new(false))).clone()
}

/// A handle onto an open `midi_out`, so Lua's `midi_emit` can send through a
/// port that a node already owns — the connection, its channel, and the
/// shutdown bookkeeping all stay in one place.
///
/// Emitted messages go through the same timed queue as everything else on
/// that port, so ordering is preserved: a gate CC queued after a position CC
/// can never overtake it on the wire.
#[derive(Clone)]
pub struct MidiEmitter {
    channel: u8,
    producer: Arc<Mutex<HeapProd<TimedMessage>>>,
    audio_counter: Arc<AtomicU64>,
}

impl MidiEmitter {
    /// Queue a Control Change. `channel` overrides the node's own channel when
    /// given, both 0-indexed.
    pub fn send_cc(&self, cc: u8, value: u8, channel: Option<u8>) {
        self.queue([
            0xB0 | channel.unwrap_or(self.channel),
            cc & 0x7F,
            value.min(127),
        ]);
    }

    /// Queue a note on (velocity 0 is a note off by convention).
    pub fn send_note_on(&self, note: u8, velocity: u8, channel: Option<u8>) {
        self.queue([
            0x90 | channel.unwrap_or(self.channel),
            note & 0x7F,
            velocity.min(127),
        ]);
    }

    pub fn send_note_off(&self, note: u8, channel: Option<u8>) {
        self.queue([0x80 | channel.unwrap_or(self.channel), note & 0x7F, 0]);
    }

    fn queue(&self, bytes: [u8; 3]) {
        let msg = TimedMessage {
            sample: self.audio_counter.load(Ordering::Relaxed),
            bytes,
        };
        // Called from a MIDI callback thread, never the audio thread, so a
        // brief lock here is safe.
        if let Ok(mut p) = self.producer.lock() {
            let _ = p.try_push(msg);
        }
    }
}

/// Ask every MIDI sender thread to flush its note-offs and exit. Call before
/// the process leaves `play()`.
pub fn shutdown_senders() {
    shutdown_flag().store(true, Ordering::Relaxed);
    // Give the threads a tick to send their note-offs before the process tears
    // the connections down underneath them.
    std::thread::sleep(SENDER_TICK * 6);
}

/// MIDI clock is 24 pulses per quarter note, by the standard.
const PULSES_PER_QUARTER: u32 = 24;

/// Turns a stream of MIDI clock pulses into a BPM estimate.
///
/// Timing one pulse against the next would be far too noisy — USB scheduling
/// alone jitters by a millisecond, which at 120 BPM is ~5% of a pulse period.
/// Instead this averages a whole quarter note (24 pulses) between estimates,
/// which divides the jitter by 24 and lands on a musically meaningful unit.
pub struct ClockTracker {
    pulses: u32,
    window_start: Option<std::time::Instant>,
}

impl Default for ClockTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ClockTracker {
    pub fn new() -> Self {
        Self {
            pulses: 0,
            window_start: None,
        }
    }

    /// Feed one 0xF8 pulse. Returns a BPM estimate once per quarter note, and
    /// `None` on the pulses in between.
    pub fn pulse(&mut self) -> Option<f32> {
        let now = std::time::Instant::now();
        let start = match self.window_start {
            None => {
                // First pulse only starts the window; there's no interval to
                // measure yet.
                self.window_start = Some(now);
                self.pulses = 0;
                return None;
            }
            Some(start) => start,
        };

        self.pulses += 1;
        if self.pulses < PULSES_PER_QUARTER {
            return None;
        }

        let elapsed = now.duration_since(start).as_secs_f32();
        self.window_start = Some(now);
        self.pulses = 0;

        // A quarter note is a beat, so beats-per-minute is just how many of
        // these windows fit in a minute.
        if elapsed > 0.0 {
            Some(60.0 / elapsed)
        } else {
            None
        }
    }

    /// Forget the current window. Called on transport stop, so the gap while
    /// stopped isn't measured as one enormous beat.
    pub fn reset(&mut self) {
        self.pulses = 0;
        self.window_start = None;
    }
}

/// The graph node behind `song:midi_in(...)`.
///
/// Renders silence and holds no state: incoming messages are turned into
/// `ScheduledEvent`s by the input manager and delivered through the same
/// param ring buffer the audio callback already drains, so they land on a
/// block boundary like every other event. The node exists so the Lua graph
/// has something to name and connect.
pub struct MidiInNode {
    id: usize,
}

impl MidiInNode {
    pub fn new(id: usize) -> Self {
        Self { id }
    }
}

impl Instrument for MidiInNode {
    fn id(&self) -> usize {
        self.id
    }

    fn process_event(&mut self, _event: &NoteEvent) {}

    fn next_sample(&mut self, _sample_rate: f32) -> Frame {
        Frame::SILENCE
    }

    fn render_block(&mut self, out: &mut [Frame], _sample_rate: f32) {
        out.fill(Frame::SILENCE);
    }
}

#[cfg(test)]
mod clock_tests {
    use super::*;

    /// Feed 24 pulses spaced to imply `bpm`, return the estimate.
    fn estimate_at(bpm: f32) -> f32 {
        let mut t = ClockTracker::new();
        let quarter = std::time::Duration::from_secs_f32(60.0 / bpm);
        let per_pulse = quarter / PULSES_PER_QUARTER;
        // First pulse only opens the window.
        assert_eq!(t.pulse(), None);
        let mut got = None;
        for i in 0..PULSES_PER_QUARTER {
            std::thread::sleep(per_pulse);
            let r = t.pulse();
            if i < PULSES_PER_QUARTER - 1 {
                assert_eq!(r, None, "estimated early at pulse {i}");
            } else {
                got = r;
            }
        }
        got.expect("no estimate after a full quarter note")
    }

    #[test]
    fn estimates_bpm_from_pulse_rate() {
        // Sleep-based timing is coarse, so allow generous tolerance — this
        // checks the arithmetic and the 24-pulse window, not clock precision.
        let est = estimate_at(120.0);
        assert!(
            (est - 120.0).abs() < 25.0,
            "expected ~120 BPM from 24 pulses/quarter, got {est}"
        );
    }

    #[test]
    fn reset_discards_the_open_window() {
        let mut t = ClockTracker::new();
        t.pulse();
        t.pulse();
        t.reset();
        // After a reset the next pulse reopens the window rather than
        // measuring the whole stopped gap as one beat.
        assert_eq!(t.pulse(), None);
    }
}
