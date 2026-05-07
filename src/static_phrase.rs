use crate::{midi::MidiNote, phrase::Phrase, scripting::LuaRuntime};

pub struct StaticPhrase {
    pub name: String,
    pub notes: Vec<MidiNote>,
    pub duration_beats: f32,
}

impl StaticPhrase {
    pub fn new(name: String, notes: Vec<MidiNote>, duration_beats: f32) -> Self {
        Self {
            name,
            notes,
            duration_beats,
        }
    }
}

impl Phrase for StaticPhrase {
    fn name(&self) -> &str {
        &self.name
    }

    fn duration_beats(&self) -> f32 {
        self.duration_beats
    }

    fn notes(&self, _lua: &LuaRuntime) -> Vec<MidiNote> {
        self.notes.clone()
    }
}
