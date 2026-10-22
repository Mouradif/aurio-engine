//! Load a song file and report whether it parses, without opening an audio
//! device or playing anything.
//!
//! `Song::load` does all the Lua evaluation and validation; `play` is what
//! touches the hardware. Splitting them here lets a generated file be checked
//! in CI, or by a tool like aurio-ui, without making a sound.
//!
//!     cargo run --example check_song -- song.lua

use aurio::project::Song;
use aurio::scripting::LuaRuntime;
use std::env;
use std::process::exit;

fn main() {
    let args: Vec<String> = env::args().collect();
    let Some(path) = args.get(1) else {
        eprintln!("Usage: check_song <song.lua>");
        exit(2);
    };

    let lua = LuaRuntime::new();
    match Song::load(path, &lua) {
        Ok(_) => println!("ok: {path} parses"),
        Err(err) => {
            eprintln!("error: {path}: {err}");
            exit(1);
        }
    }
}
