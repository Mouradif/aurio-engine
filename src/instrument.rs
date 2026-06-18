use crate::frame::Frame;
use crate::midi::NoteEvent;

pub trait Instrument {
    fn id(&self) -> usize;
    fn process_event(&mut self, event: &NoteEvent);
    fn next_sample(&mut self, sample_rate: f32) -> Frame;

    /// Render a block of frames into `out`, summing onto whatever it already
    /// holds is *not* expected — `out` is written, not accumulated. The default
    /// impl drives `next_sample` per frame; native nodes can leave it. Plugin
    /// wrappers (later) override this to feed the host a real buffer.
    fn render_block(&mut self, out: &mut [Frame], sample_rate: f32) {
        for slot in out.iter_mut() {
            *slot = self.next_sample(sample_rate);
        }
    }
}
