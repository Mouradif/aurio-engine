use crate::{midi::MidiNote, scripting::LuaRuntime};

pub trait Phrase {
    fn name(&self) -> &str;

    fn duration_beats(&self) -> f32;

    fn notes(&self, lua: &LuaRuntime) -> Vec<MidiNote>;
}

#[derive(Default, Clone)]
pub struct PhraseHooks {
    pub on_enter: String,
    pub on_loop: String,
    pub on_exit: String,
}

pub struct PhraseEntry {
    pub phrase: Box<dyn Phrase + Send>,
    pub hooks: PhraseHooks,
}
