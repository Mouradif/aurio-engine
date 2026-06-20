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

    /// Stage a parameter change addressed by a plugin param id. Default no-op;
    /// only plugin instrument nodes (CLAP/LV2) override it.
    #[cfg(any(feature = "clap-host", feature = "lv2-host"))]
    fn set_param_by_id(&mut self, _param_id: u32, _value: f32) {}
}
