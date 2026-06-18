use crate::midi::{MidiNote, parse_note_value};
use mlua::{Function, Lua, RegistryKey, Table, Value};

pub struct LuaRuntime {
    lua: Lua,
}

impl Default for LuaRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl LuaRuntime {
    pub fn new() -> Self {
        Self { lua: Lua::new() }
    }

    pub fn lua(&self) -> &Lua {
        &self.lua
    }

    pub fn call_condition(&self, key: &RegistryKey) -> Result<bool, mlua::Error> {
        let f: Function = self.lua.registry_value(key)?;
        f.call::<bool>(())
    }

    pub fn run_hook(&self, key: &RegistryKey) -> Result<(), mlua::Error> {
        let f: Function = self.lua.registry_value(key)?;
        f.call::<()>(())
    }

    pub fn call_notes(&self, key: &RegistryKey) -> Result<Vec<MidiNote>, mlua::Error> {
        let f: Function = self.lua.registry_value(key)?;
        let table: Table = f.call(())?;
        notes_from_table(&table)
    }
}

pub fn notes_from_table(table: &Table) -> Result<Vec<MidiNote>, mlua::Error> {
    let mut notes = Vec::new();
    for pair in table.pairs::<i64, Table>() {
        let (_, t) = pair?;
        let velocity: u8 = t.get::<Option<u8>>("velocity")?.unwrap_or(100);
        let pitch_value: Value = t.get("pitch")?;
        notes.push(MidiNote {
            pitch: parse_note_value(&pitch_value)?,
            velocity,
            start_beat: t.get("start")?,
            duration_beats: t.get("duration")?,
        });
    }
    Ok(notes)
}
