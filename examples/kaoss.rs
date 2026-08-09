//! Standalone Kaossilator Pro+ probe — the same MIDI traffic `kaoss.lua`
//! produces, with no audio engine underneath it.
//!
//! Run with `cargo run --example kaoss`. If this makes sound, the CC map is
//! right and any silence from `kaoss.lua` is aurio's problem; if it stays
//! quiet, the CC numbers or the port are wrong and the engine is blameless.
//!
//! It runs three stages, announcing each:
//!   1. a static tap — gate on, hold, gate off, at a fixed pad position
//!   2. an X/Y sweep with the gate held down
//!   3. the gated sweep `kaoss.lua` plays
//!
//! Stage 1 is the real diagnostic: if a plain gate-on at a fixed position
//! makes no sound, nothing about the sweeps will either.
//!
//! Ctrl-C is caught so the pad is always released — a bare exit here would
//! leave the gate raised and the Kaossilator droning.

use midir::{MidiOutput, MidiOutputConnection};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Substring matched case-insensitively against the MIDI port name.
const PORT: &str = "Port 3";
/// MIDI channel, 0-indexed here — 1 is what the Kaossilator calls channel 2.
///
/// Confirmed by echoing the unit's own output back at it: every message it
/// transmits when the pad is touched carries status `0xB1`. Sending to
/// channel 1 (`0`) is silently ignored, which looks exactly like a broken
/// CC map but isn't.
const CHANNEL: u8 = 1;

// CC map, confirmed from the device's own transmissions rather than the
// manual: touching the pad emits exactly these three.
const CC_X: u8 = 12;
const CC_Y: u8 = 13;
const CC_GATE: u8 = 92; // touch on/off — nothing sounds unless this is up
const CC_VOLUME: u8 = 94;
const CC_ARP_ON: u8 = 90;
const CC_ARP_SPEED: u8 = 91;
const CC_ARP_TIME: u8 = 93;

fn main() {
    let midi_out = MidiOutput::new("aurio-kaoss-probe").expect("failed to open MIDI output");

    let ports = midi_out.ports();
    if ports.is_empty() {
        eprintln!("No MIDI output ports available.");
        return;
    }

    println!("Available MIDI outputs:");
    for (i, p) in ports.iter().enumerate() {
        println!("  {}: {}", i, midi_out.port_name(p).unwrap_or_default());
    }

    let want = PORT.to_lowercase();
    let port = match ports.iter().find(|p| {
        midi_out
            .port_name(p)
            .unwrap_or_default()
            .to_lowercase()
            .contains(&want)
    }) {
        Some(p) => p.clone(),
        None => {
            eprintln!("\nNo output port matching '{PORT}'.");
            return;
        }
    };

    let port_name = midi_out.port_name(&port).unwrap_or_default();
    println!("\nSending on: {port_name}\n");

    let mut conn = midi_out
        .connect(&port, "aurio-kaoss")
        .expect("failed to connect");

    // Any exit path has to lower the gate, so the handler just flips a flag
    // and every sleep below checks it.
    install_ctrlc();

    run(&mut conn);

    println!("\nReleasing pad.");
    all_off(&mut conn);
}

fn run(conn: &mut MidiOutputConnection) {
    // Known state first: volume up, arp off, gate down.
    cc(conn, CC_VOLUME, 100);
    cc(conn, CC_ARP_ON, 0);
    cc(conn, CC_GATE, 0);

    // ---- Stage 1: a static tap ------------------------------------------
    // Four one-second notes at a fixed pad position. If you hear nothing
    // here, the CC map or the port is wrong — skip ahead no further.
    println!("[1/3] static taps at centre pad (4 x 1s)");
    for i in 1..=4 {
        if STOP.load(Ordering::Relaxed) {
            return;
        }
        println!("      tap {i}: pad down");
        pad_down(conn, 64, 64);
        if !sleep_until(Duration::from_millis(1000)) {
            return;
        }
        pad_up(conn);
        if !sleep_until(Duration::from_millis(500)) {
            return;
        }
    }

    // ---- Stage 2: held sweep --------------------------------------------
    // Gate held down while X and Y move, so you hear the pad position change
    // rather than a series of taps.
    println!("[2/3] X/Y sweep, gate held (8s)");
    // Touch down at the sweep's starting corner, then move while held.
    pad_down(conn, 10, 30);
    let steps = 160;
    for i in 0..steps {
        if STOP.load(Ordering::Relaxed) {
            return;
        }
        let phase = i as f32 / steps as f32;
        // X traces a full sine; Y a slower triangle, so the pair moves in a
        // figure across the pad instead of a straight diagonal.
        let x = 10.0 + 108.0 * (0.5 * (1.0 - (phase * 2.0 * std::f32::consts::PI).cos()));
        let y = 30.0 + 70.0 * (1.0 - (2.0 * (phase * 2.0).fract() - 1.0).abs());
        cc(conn, CC_X, x as u8);
        cc(conn, CC_Y, y as u8);
        if !sleep_until(Duration::from_millis(50)) {
            return;
        }
    }
    pad_up(conn);
    if !sleep_until(Duration::from_millis(500)) {
        return;
    }

    // ---- Stage 3: the kaoss.lua pattern ---------------------------------
    // What the Lua example plays: a 0.5 Hz square wave on the gate with the
    // pad sweeping underneath it.
    println!("[3/3] gated sweep, as kaoss.lua (10s)");
    let steps = 200;
    let mut gate_up = false;
    for i in 0..steps {
        if STOP.load(Ordering::Relaxed) {
            return;
        }
        let t = i as f32 * 0.05;

        let x = (10.0 + 108.0 * (0.5 * (1.0 - (t * 0.2 * 2.0 * std::f32::consts::PI).cos()))) as u8;
        let y = (30.0 + 70.0 * (1.0 - (2.0 * (t * 0.07).fract() - 1.0).abs())) as u8;

        // Square wave at 0.5 Hz: one second on, one second off. A rising edge
        // goes through the full touch-down sequence so the note starts at the
        // right pad position rather than wherever the last release left it.
        let want_gate = (t * 0.5).fract() >= 0.5;
        if want_gate != gate_up {
            gate_up = want_gate;
            if gate_up {
                pad_down(conn, x, y);
            } else {
                pad_up(conn);
            }
        } else if gate_up {
            // Held: just move.
            cc(conn, CC_X, x);
            cc(conn, CC_Y, y);
        }

        if !sleep_until(Duration::from_millis(50)) {
            return;
        }
    }
}

fn cc(conn: &mut MidiOutputConnection, cc: u8, value: u8) {
    let _ = conn.send(&[0xB0 | CHANNEL, cc & 0x7F, value.min(127)]);
}

/// Press the pad at (x, y), reproducing the message order the Kaossilator
/// itself emits on a real touch: Y, X, X, Y, then the gate. Position is
/// established before the gate goes up — the unit latches where the touch
/// landed, so a gate raised against a stale coordinate is asking for trouble.
/// The doubled X/Y is what the hardware does; harmless, and it costs nothing
/// to match.
fn pad_down(conn: &mut MidiOutputConnection, x: u8, y: u8) {
    cc(conn, CC_Y, y);
    cc(conn, CC_X, x);
    cc(conn, CC_X, x);
    cc(conn, CC_Y, y);
    cc(conn, CC_GATE, 127);
}

/// Release the pad. Position is left where it was, exactly as a real touch
/// leaves it.
fn pad_up(conn: &mut MidiOutputConnection) {
    cc(conn, CC_GATE, 0);
}

/// Zero everything this probe raises. All Notes Off is not enough on its own:
/// it speaks to the note engine, and the pad is held by a CC.
fn all_off(conn: &mut MidiOutputConnection) {
    for n in [CC_GATE, CC_ARP_ON, CC_ARP_SPEED, CC_ARP_TIME] {
        cc(conn, n, 0);
    }
    let _ = conn.send(&[0xB0 | CHANNEL, 123, 0]);
}

/// Sleep in short slices so Ctrl-C is noticed promptly rather than at the end
/// of a long wait. Returns false if a stop was requested.
fn sleep_until(total: Duration) -> bool {
    let slice = Duration::from_millis(25);
    let mut left = total;
    while left > Duration::ZERO {
        if STOP.load(Ordering::Relaxed) {
            return false;
        }
        let step = slice.min(left);
        std::thread::sleep(step);
        left -= step;
    }
    !STOP.load(Ordering::Relaxed)
}

/// Set by the SIGINT handler, polled by every sleep.
static STOP: AtomicBool = AtomicBool::new(false);

/// Minimal SIGINT hook, so the probe needs no extra dependency. The handler
/// only sets a flag — the MIDI teardown happens back on the main thread,
/// where it's safe to do real work.
fn install_ctrlc() {
    unsafe extern "C" {
        fn signal(sig: i32, handler: usize) -> usize;
    }
    extern "C" fn handle(_sig: i32) {
        STOP.store(true, Ordering::Relaxed);
    }
    unsafe {
        signal(2 /* SIGINT */, handle as *const () as usize);
    }
}
