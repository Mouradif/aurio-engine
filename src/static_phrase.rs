use crate::{
    midi::MidiNote,
    phrase::Phrase,
    rtttl::parse_rtttl,
    scripting::{LuaRuntime, eval_or_empty},
};

#[derive(Debug)]
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

    pub fn from_lua(lua: &LuaRuntime, name: String, code: String, duration_beats: f32) -> Self {
        Self {
            name,
            notes: eval_or_empty(lua, &code),
            duration_beats,
        }
    }

    pub fn from_rtttl(name: String, rtttl: &str) -> Self {
        let (duration_beats, notes) = parse_rtttl(rtttl);
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
