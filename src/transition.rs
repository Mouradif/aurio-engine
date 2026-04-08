#[derive(Clone)]
pub struct Transition {
    pub from: usize,
    pub to: usize,
    pub condition: String,
}

impl Transition {
    pub fn new(from: usize, to: usize, condition: String) -> Self {
        Self {
            from,
            to,
            condition,
        }
    }

    pub fn always(from: usize, to: usize) -> Self {
        Self::new(from, to, String::from("true"))
    }
}
