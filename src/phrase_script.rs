use crate::{
    midi::MidiNote,
    phrase::Phrase,
    scripting::{LuaRuntime, eval_or_empty},
};

pub struct PhraseScript {
    pub name: String,
    pub duration_beats: f32,
    pub code: String,
}

impl PhraseScript {
    pub fn new(name: String, code: String, duration_beats: f32) -> Self {
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

    fn notes(&self, lua: &LuaRuntime) -> Vec<MidiNote> {
        eval_or_empty(lua, &self.code)
    }
}
