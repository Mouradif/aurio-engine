//! Print the zones of an EXS24 instrument: which keys and velocities map to
//! which recording, and whether that recording can be found on disk.
//!
//! Handy for writing phrases against a drum kit, where every key is a
//! different drum and the names only live inside the .exs.
//!
//!     cargo run --example exs_info -- "/path/to/Kit.exs"

use aurio::exs::ExsInstrument;
use std::env;
use std::path::Path;
use std::process::exit;

fn main() {
    let args: Vec<String> = env::args().collect();
    let Some(path) = args.get(1) else {
        eprintln!("Usage: exs_info <instrument.exs>");
        exit(2);
    };

    let inst = match ExsInstrument::load(path) {
        Ok(inst) => inst,
        Err(err) => {
            eprintln!("error: {path}: {err}");
            exit(1);
        }
    };

    println!(
        "{}: {} zones, {} samples",
        inst.name,
        inst.zones.len(),
        inst.samples.len()
    );
    for z in &inst.zones {
        let s = &inst.samples[z.sample];
        let found = if s.resolve(Path::new(path), &[]).is_some() {
            ""
        } else {
            "  [missing]"
        };
        println!(
            "  keys {:>3}..{:<3} root {:>3}  vel {:>3}..{:<3}  {}{}",
            z.low, z.high, z.root, z.vel_low, z.vel_high, s.name, found
        );
    }
}
