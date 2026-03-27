use crate::midi::MidiNote;
use mlua::Lua;

pub struct LuaRuntime {
    lua: Lua,
}

impl LuaRuntime {
    pub fn new() -> Self {
        Self { lua: Lua::new() }
    }

    pub fn eval_pattern(&self, code: &str) -> Result<Vec<MidiNote>, mlua::Error> {
        let result: mlua::Table = self.lua.load(code).eval()?;

        let mut notes = Vec::new();
        for pair in result.pairs::<usize, mlua::Table>() {
            let (_, table) = pair?;
            notes.push(MidiNote {
                pitch: table.get("pitch")?,
                velocity: table.get("velocity")?,
                start_beat: table.get("start_beat")?,
                duration_beats: table.get("duration_beats")?,
            });
        }

        Ok(notes)
    }

    pub fn eval_condition(&self, code: &str) -> Result<bool, mlua::Error> {
        let result: bool = self.lua.load(code).eval()?;
        Ok(result)
    }
}
