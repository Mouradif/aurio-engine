use crate::phrase::Phrase;

pub struct PhraseScript {
    pub name: String,
    pub duration_beats: f32,
    pub code: String,
}

impl PhraseScript {
    pub fn new(name: String, duration_beats: f32, code: String) -> Self {
        Self {
            name,
            duration_beats,
            code,
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

    fn notes(&self, lua: &crate::scripting::LuaRuntime) -> Vec<crate::midi::MidiNote> {
        match lua.eval_pattern(&self.code) {
            Ok(notes) => notes,
            Err(err) => {
                eprintln!("[lua_error] {}: {}", self.name, err);
                Vec::new()
            }
        }
    }
}
