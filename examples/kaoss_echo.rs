//! MIDI echo: read whatever the Kaossilator sends on Port 3 and play it back
//! at the device one second later.
//!
//! Run with `cargo run --example kaoss_echo`, then touch the pad.
//!
//! This sidesteps every guess about the CC map. A device's own output is by
//! definition a message it understands, so echoing it back removes the CC
//! numbers, the channel, and the value ranges as variables all at once.
//! Three outcomes, each conclusive:
//!
//! * Nothing prints — the KP+ isn't transmitting (check its MIDI out cable,
//!   and that it's set to send), or the router isn't feeding Port 3's input.
//! * Prints, echo is audible — the format is right and reception works.
//!   Whatever `kaoss.rs` sends differs from the printed messages; copy these.
//! * Prints, echo is silent — it transmits but ignores what it receives.
//!   That's the `*C`/`*N` "enabled" footnote in the MIDI chart: CC and note
//!   reception are off until switched on in Global settings.
//!
//! The printed log is the useful artefact either way — it *is* the device's
//! real MIDI map, observed rather than inferred.

use midir::{Ignore, MidiInput, MidiOutput};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Substring matched case-insensitively against both port names.
const PORT: &str = "Port 3";
/// How long a message waits before being echoed back.
const DELAY: Duration = Duration::from_secs(1);

fn main() {
    let mut midi_in = MidiInput::new("aurio-kaoss-echo-in").expect("failed to open MIDI input");
    // Active-sensing bytes arrive ~every 300ms on some gear and would drown
    // the log; sysex and timing are dropped for the same reason.
    midi_in.ignore(Ignore::All);
    let midi_out = MidiOutput::new("aurio-kaoss-echo-out").expect("failed to open MIDI output");

    let in_ports = midi_in.ports();
    let out_ports = midi_out.ports();

    println!("MIDI inputs:");
    for (i, p) in in_ports.iter().enumerate() {
        println!("  {}: {}", i, midi_in.port_name(p).unwrap_or_default());
    }
    println!("MIDI outputs:");
    for (i, p) in out_ports.iter().enumerate() {
        println!("  {}: {}", i, midi_out.port_name(p).unwrap_or_default());
    }

    let want = PORT.to_lowercase();
    let in_port = in_ports.iter().find(|p| {
        midi_in
            .port_name(p)
            .unwrap_or_default()
            .to_lowercase()
            .contains(&want)
    });
    let out_port = out_ports.iter().find(|p| {
        midi_out
            .port_name(p)
            .unwrap_or_default()
            .to_lowercase()
            .contains(&want)
    });

    let (in_port, out_port) = match (in_port, out_port) {
        (Some(i), Some(o)) => (i.clone(), o.clone()),
        (None, _) => {
            eprintln!("\nNo MIDI *input* port matching '{PORT}'.");
            return;
        }
        (_, None) => {
            eprintln!("\nNo MIDI *output* port matching '{PORT}'.");
            return;
        }
    };

    println!(
        "\nListening on: {}\nEchoing to:   {}\n",
        midi_in.port_name(&in_port).unwrap_or_default(),
        midi_out.port_name(&out_port).unwrap_or_default()
    );

    let mut conn_out = midi_out
        .connect(&out_port, "aurio-kaoss-echo")
        .expect("failed to connect output");

    // The input callback runs on midir's own thread and must not block, so it
    // just hands messages to the main thread, which owns the delay and the
    // output connection.
    let (tx, rx) = mpsc::channel::<(Instant, Vec<u8>)>();
    let _conn_in = midi_in
        .connect(
            &in_port,
            "aurio-kaoss-echo",
            move |_ts, msg, _| {
                println!("  in  {}", describe(msg));
                // Echo at `DELAY` from arrival, not from send time, so a burst
                // of pad movement replays with its original spacing intact.
                let _ = tx.send((Instant::now() + DELAY, msg.to_vec()));
            },
            (),
        )
        .expect("failed to connect input");

    install_ctrlc();

    println!("Touch the pad. Every message is echoed back after 1s. Ctrl-C to quit.\n");

    // Pending echoes, kept in arrival order — which is also due order, since
    // every message gets the same fixed delay.
    let mut pending: std::collections::VecDeque<(Instant, Vec<u8>)> = Default::default();

    while !STOP.load(Ordering::Relaxed) {
        while let Ok(item) = rx.try_recv() {
            pending.push_back(item);
        }

        let now = Instant::now();
        while pending.front().is_some_and(|(due, _)| *due <= now) {
            let (_, msg) = pending.pop_front().unwrap();
            println!("  out {}", describe(&msg));
            let _ = conn_out.send(&msg);
        }

        std::thread::sleep(Duration::from_millis(5));
    }

    // Anything still queued is dropped, so release the usual suspects rather
    // than leaving a gate raised by a message that was echoed but never
    // matched by its note-off.
    println!("\nQuitting — sending all-off.");
    for channel in 0..16u8 {
        let _ = conn_out.send(&[0xB0 | channel, 92, 0]); // touch gate
        let _ = conn_out.send(&[0xB0 | channel, 123, 0]); // all notes off
    }
}

/// Render a raw MIDI message as something readable, so the log doubles as the
/// device's MIDI map. Channels are shown 1-16 to match the front panel.
fn describe(msg: &[u8]) -> String {
    if msg.is_empty() {
        return "(empty)".to_string();
    }
    let status = msg[0] & 0xF0;
    let ch = (msg[0] & 0x0F) + 1;
    match status {
        0x90 if msg.len() >= 3 && msg[2] > 0 => {
            format!("ch{ch:<2} Note On   note={:<3} vel={}", msg[1], msg[2])
        }
        0x80 | 0x90 if msg.len() >= 3 => {
            format!("ch{ch:<2} Note Off  note={:<3}", msg[1])
        }
        0xB0 if msg.len() >= 3 => {
            format!("ch{ch:<2} CC        cc={:<3} val={}", msg[1], msg[2])
        }
        0xC0 if msg.len() >= 2 => format!("ch{ch:<2} Program   {}", msg[1]),
        0xE0 if msg.len() >= 3 => {
            let bend = ((msg[2] as u16) << 7) | msg[1] as u16;
            format!("ch{ch:<2} Bend      {bend}")
        }
        _ => format!("raw {msg:02X?}"),
    }
}

/// Set by the SIGINT handler, polled by the echo loop.
static STOP: AtomicBool = AtomicBool::new(false);

/// Minimal SIGINT hook, so the probe needs no extra dependency. The handler
/// only sets a flag — the MIDI teardown happens back on the main thread.
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
