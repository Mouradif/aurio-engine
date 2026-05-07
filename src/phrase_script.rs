use crate::{midi::MidiNote, phrase::Phrase, scripting::LuaRuntime};
use mlua::RegistryKey;

pub struct PhraseScript {
    pub name: String,
    pub duration_beats: f32,
    pub notes_fn: RegistryKey,
}

impl PhraseScript {
    pub fn new(name: String, duration_beats: f32, notes_fn: RegistryKey) -> Self {
        Self {
            name,
            duration_beats,
            notes_fn,
        }
    }
}

impl Phrase for PhraseScript {
    fn name(&self) -> &str {
        &self.name
    }

    fn duration_beats(&self) -> f32 {
        self.duration_beats
    }

    fn notes(&self, lua: &LuaRuntime) -> Vec<MidiNote> {
        match lua.call_notes(&self.notes_fn) {
            Ok(notes) => notes,
            Err(err) => {
                eprintln!("[lua error] {}", err);
                Vec::new()
            }
        }
    }
}
