use crate::{midi::MidiNote, scripting::LuaRuntime};
use mlua::RegistryKey;

pub trait Phrase {
    fn name(&self) -> &str;

    fn duration_beats(&self) -> f32;

    fn notes(&self, lua: &LuaRuntime) -> Vec<MidiNote>;
}

pub struct PhraseHooks {
    pub on_enter: Option<RegistryKey>,
    pub on_loop: Option<RegistryKey>,
    pub on_exit: Option<RegistryKey>,
}

pub struct PhraseEntry {
    pub phrase: Box<dyn Phrase + Send>,
    pub hooks: PhraseHooks,
}
