use crate::frame::Frame;
use crate::midi::NoteEvent;

pub trait Instrument {
    fn id(&self) -> usize;
    fn process_event(&mut self, event: &NoteEvent);
    fn next_sample(&mut self, sample_rate: f32) -> Frame;
}
