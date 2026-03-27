use crate::{midi::MidiNote, scripting::LuaRuntime};

pub trait Phrase {
    fn name(&self) -> &str;

    fn duration_beats(&self) -> f32;

    fn notes(&self, lua: &LuaRuntime) -> Vec<MidiNote>;
}
