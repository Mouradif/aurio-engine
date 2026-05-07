use mlua::RegistryKey;

pub struct Transition {
    pub from: usize,
    pub to: usize,
    pub condition: Option<RegistryKey>,
}

impl Transition {
    pub fn new(from: usize, to: usize, condition: Option<RegistryKey>) -> Self {
        Self {
            from,
            to,
            condition,
        }
    }
}
