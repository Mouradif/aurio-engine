use aurio::project::Song;
use aurio::scripting::LuaRuntime;
use std::env;
use std::process::exit;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        let prog = args.first().map(String::as_str).unwrap_or("aurio");
        eprintln!("Usage: {} <song.lua>", prog);
        exit(1);
    }
    let path = &args[1];
    let lua = LuaRuntime::new();
    let song = match Song::load(path, &lua) {
        Ok(s) => s,
        Err(err) => {
            eprintln!("Failed to load {}: {}", path, err);
            exit(1);
        }
    };
    song.play(lua);
}
