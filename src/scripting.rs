use crate::midi::MidiNote;
use mlua::{Function, Lua, RegistryKey, Table};

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
        notes.push(MidiNote {
            pitch: t.get("pitch")?,
            velocity,
            start_beat: t.get("start")?,
            duration_beats: t.get("duration")?,
        });
    }
    Ok(notes)
}
