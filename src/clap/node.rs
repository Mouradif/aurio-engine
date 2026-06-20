//! A CLAP plugin wrapped as an Aurio graph node implementing both
//! [`Instrument`] and [`Effect`].
//!
//! The graph speaks interleaved stereo [`Frame`]s; CLAP speaks planar per-channel
//! `f32` buffers plus a sample-sorted event list. This node owns pre-allocated
//! planar scratch (sized to `MAX_BLOCK`) and an event staging buffer, so a
//! `process_block`/`render_block` call de-interleaves input → planar, builds the
//! event list, calls `process()`, and re-interleaves the planar output → frames.
//! No allocation happens on the audio thread.

use std::os::raw::c_void;
use std::path::Path;

use crate::audio_graph::MAX_BLOCK;
use crate::effect::Effect;
use crate::frame::Frame;
use crate::instrument::Instrument;
use crate::midi::NoteEvent;

use super::host::{ClapPlugin, HostError, ParamInfo};
use super::sys::*;

pub use super::host::HostError as PluginError;

/// One pending input event for the next `process()` call. Stored in the node's
/// staging vector and materialized into CLAP event structs on demand.
#[derive(Clone, Copy)]
enum StagedEvent {
    Note {
        time: u32,
        type_: u16,
        key: i16,
        velocity: f64,
    },
    Param {
        time: u32,
        param_id: clap_id,
        value: f64,
    },
}

/// A fully materialized CLAP event with stable storage. A plugin may hold the
/// pointers returned by `ev_get` for the duration of `process()` and read them
/// in any order, so every event needs its *own* live struct (not a shared
/// scratch slot). These are kept in `EventList::materialized`.
union MaterializedEvent {
    header: clap_event_header,
    note: clap_event_note,
    param: clap_event_param_value,
}

/// Backing storage for the `clap_input_events` vtable. `staged` accumulates
/// events as the host/graph produces them; `materialize()` converts them into
/// `materialized` (stable per-event storage) right before `process()`.
struct EventList {
    staged: Vec<StagedEvent>,
    materialized: Vec<MaterializedEvent>,
}

impl EventList {
    fn new() -> Self {
        EventList {
            staged: Vec::with_capacity(256),
            materialized: Vec::with_capacity(256),
        }
    }

    /// Build the stable CLAP event structs from the staged list. Called once
    /// per `process()` so all `ev_get` pointers remain valid simultaneously.
    fn materialize(&mut self) {
        self.materialized.clear();
        for ev in &self.staged {
            let m = match *ev {
                StagedEvent::Note {
                    time,
                    type_,
                    key,
                    velocity,
                } => MaterializedEvent {
                    note: clap_event_note {
                        header: clap_event_header {
                            size: std::mem::size_of::<clap_event_note>() as u32,
                            time,
                            space_id: CLAP_CORE_EVENT_SPACE_ID,
                            type_,
                            flags: CLAP_EVENT_IS_LIVE,
                        },
                        note_id: -1,
                        port_index: 0,
                        channel: 0,
                        key,
                        velocity,
                    },
                },
                StagedEvent::Param {
                    time,
                    param_id,
                    value,
                } => MaterializedEvent {
                    param: clap_event_param_value {
                        header: clap_event_header {
                            size: std::mem::size_of::<clap_event_param_value>() as u32,
                            time,
                            space_id: CLAP_CORE_EVENT_SPACE_ID,
                            type_: CLAP_EVENT_PARAM_VALUE,
                            flags: 0,
                        },
                        param_id,
                        cookie: std::ptr::null_mut(),
                        note_id: -1,
                        port_index: -1,
                        channel: -1,
                        key: -1,
                        value,
                    },
                },
            };
            self.materialized.push(m);
        }
    }
}

unsafe extern "C" fn ev_size(list: *const clap_input_events) -> u32 {
    let el = unsafe { &*((*list).ctx as *const EventList) };
    el.materialized.len() as u32
}

unsafe extern "C" fn ev_get(
    list: *const clap_input_events,
    index: u32,
) -> *const clap_event_header {
    let el = unsafe { &*((*list).ctx as *const EventList) };
    match el.materialized.get(index as usize) {
        // SAFETY: every variant begins with a `clap_event_header` (repr(C)),
        // so reading `.header` is valid regardless of the actual event type.
        Some(m) => unsafe { &m.header as *const clap_event_header },
        None => std::ptr::null(),
    }
}

unsafe extern "C" fn out_try_push(
    _list: *const clap_output_events,
    _event: *const clap_event_header,
) -> bool {
    // Aurio doesn't consume plugin output events (no automation recording).
    true
}

/// A CLAP plugin as a graph node.
pub struct PluginNode {
    id: usize,
    plugin: ClapPlugin,
    params: Vec<ParamInfo>,

    /// Planar input/output channel buffers (L, R), each `MAX_BLOCK` long.
    in_l: Vec<f32>,
    in_r: Vec<f32>,
    out_l: Vec<f32>,
    out_r: Vec<f32>,

    /// Channel pointer arrays handed to CLAP via `clap_audio_buffer.data32`.
    in_ptrs: [*mut f32; 2],
    out_ptrs: [*mut f32; 2],

    events: Box<EventList>,
    steady: i64,
}

// All raw pointers point into this struct's own boxed/owned storage; the node is
// only ever touched from the audio thread once built.
unsafe impl Send for PluginNode {}

impl PluginNode {
    /// Load a `.clap` at `path`, instantiate `plugin_id`, and activate it for
    /// `sample_rate` and up to `MAX_BLOCK` frames per call.
    pub fn load(
        id: usize,
        path: &Path,
        plugin_id: &str,
        sample_rate: f32,
    ) -> Result<Self, HostError> {
        let mut plugin = ClapPlugin::load(path, plugin_id)?;
        plugin.activate(sample_rate as f64, 1, MAX_BLOCK as u32)?;
        let params = plugin.params();

        let mut node = PluginNode {
            id,
            plugin,
            params,
            in_l: vec![0.0; MAX_BLOCK],
            in_r: vec![0.0; MAX_BLOCK],
            out_l: vec![0.0; MAX_BLOCK],
            out_r: vec![0.0; MAX_BLOCK],
            in_ptrs: [std::ptr::null_mut(); 2],
            out_ptrs: [std::ptr::null_mut(); 2],
            events: Box::new(EventList::new()),
            steady: 0,
        };
        node.in_ptrs = [node.in_l.as_mut_ptr(), node.in_r.as_mut_ptr()];
        node.out_ptrs = [node.out_l.as_mut_ptr(), node.out_r.as_mut_ptr()];
        Ok(node)
    }

    /// The plugin's discovered parameters (id, name, range).
    pub fn params(&self) -> &[ParamInfo] {
        &self.params
    }

    /// Resolve a parameter name to its stable CLAP id (case-insensitive).
    pub fn param_id_by_name(&self, name: &str) -> Option<clap_id> {
        self.params
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
            .map(|p| p.id)
    }

    pub fn name(&self) -> &str {
        self.plugin.name()
    }

    /// Run the plugin over `block` frames: de-interleave into the planar input,
    /// call `process()`, and read the planar output back into `out`. `input` may
    /// be `None` for instruments (silent input). Events already staged in
    /// `self.events` are delivered; the staging buffer is cleared afterward.
    ///
    /// The planar input channels (`self.in_l`/`self.in_r`) must already be
    /// populated for `n` frames before calling — `run` does not touch them, so
    /// it never needs to borrow an external input slice while holding `&mut
    /// self`. Use [`fill_silent_input`] or [`fill_input_from`] to prepare them.
    fn run(&mut self, n: usize, out: &mut [Frame]) {
        let n = n.min(MAX_BLOCK).min(out.len());

        let audio_in = clap_audio_buffer {
            data32: self.in_ptrs.as_mut_ptr(),
            data64: std::ptr::null_mut(),
            channel_count: 2,
            latency: 0,
            constant_mask: 0,
        };
        let mut audio_out = clap_audio_buffer {
            data32: self.out_ptrs.as_mut_ptr(),
            data64: std::ptr::null_mut(),
            channel_count: 2,
            latency: 0,
            constant_mask: 0,
        };

        // Materialize staged events into stable storage so every `ev_get`
        // pointer the plugin reads during process() stays valid at once.
        self.events.materialize();

        let in_events = clap_input_events {
            ctx: self.events.as_mut() as *mut EventList as *mut c_void,
            size: Some(ev_size),
            get: Some(ev_get),
        };
        let out_events = clap_output_events {
            ctx: std::ptr::null_mut(),
            try_push: Some(out_try_push),
        };

        let process = clap_process {
            steady_time: self.steady,
            frames_count: n as u32,
            transport: std::ptr::null(),
            audio_inputs: &audio_in,
            audio_inputs_count: 1,
            audio_outputs: &mut audio_out,
            audio_outputs_count: 1,
            in_events: &in_events,
            out_events: &out_events,
        };

        let status = unsafe {
            match (*self.plugin.plugin_ptr()).process {
                Some(f) => f(self.plugin.plugin_ptr(), &process),
                None => CLAP_PROCESS_ERROR,
            }
        };

        if status == CLAP_PROCESS_ERROR {
            // On error, emit silence rather than garbage.
            for slot in out.iter_mut().take(n) {
                *slot = Frame::SILENCE;
            }
        } else {
            for (slot, (&l, &r)) in out[..n]
                .iter_mut()
                .zip(self.out_l[..n].iter().zip(&self.out_r[..n]))
            {
                *slot = Frame::new(l, r);
            }
        }

        self.events.staged.clear();
        self.steady += n as i64;
    }

    /// Zero the planar input for `n` frames (instruments have no audio input).
    fn fill_silent_input(&mut self, n: usize) {
        let n = n.min(MAX_BLOCK);
        self.in_l[..n].fill(0.0);
        self.in_r[..n].fill(0.0);
    }

    /// De-interleave `frames` into the planar input channels.
    fn fill_input_from(&mut self, frames: &[Frame]) {
        let n = frames.len().min(MAX_BLOCK);
        for (i, frame) in frames[..n].iter().enumerate() {
            self.in_l[i] = frame.l;
            self.in_r[i] = frame.r;
        }
    }

    fn stage_note(&mut self, event: &NoteEvent) {
        let staged = match event {
            NoteEvent::NoteOn { pitch, velocity } => StagedEvent::Note {
                time: 0,
                type_: CLAP_EVENT_NOTE_ON,
                key: *pitch as i16,
                velocity: (*velocity as f64) / 127.0,
            },
            NoteEvent::NoteOff { pitch } => StagedEvent::Note {
                time: 0,
                type_: CLAP_EVENT_NOTE_OFF,
                key: *pitch as i16,
                velocity: 0.0,
            },
        };
        self.events.staged.push(staged);
    }
}

impl Instrument for PluginNode {
    fn id(&self) -> usize {
        self.id
    }

    fn process_event(&mut self, event: &NoteEvent) {
        self.stage_note(event);
    }

    fn next_sample(&mut self, _sample_rate: f32) -> Frame {
        // Per-sample is never used for plugins; render a 1-frame block.
        let mut out = [Frame::SILENCE; 1];
        self.fill_silent_input(1);
        self.run(1, &mut out);
        out[0]
    }

    fn render_block(&mut self, out: &mut [Frame], _sample_rate: f32) {
        self.fill_silent_input(out.len());
        self.run(out.len(), out);
    }

    fn set_param_by_id(&mut self, param_id: u32, value: f32) {
        self.stage_param(param_id, value);
    }
}

impl Effect for PluginNode {
    fn process(&mut self, frame: Frame) -> Frame {
        let mut out = [Frame::SILENCE; 1];
        self.fill_input_from(&[frame]);
        self.run(1, &mut out);
        out[0]
    }

    fn set_param(&mut self, _idx: u8, _value: f32) {
        // Plugins are automated by CLAP param id, not by the u8 index used for
        // native effects. See `set_param_by_id`, driven from the graph's
        // resolved bindings.
    }

    fn process_block(&mut self, block: &mut [Frame]) {
        // De-interleave the block straight into the planar input, then write the
        // plugin's output back over it. The input lives in self.in_l/in_r, so no
        // external borrow is held across run() and nothing is allocated.
        let n = block.len().min(MAX_BLOCK);
        self.fill_input_from(&block[..n]);
        self.run(n, &mut block[..n]);
    }

    fn set_param_by_id(&mut self, param_id: u32, value: f32) {
        self.stage_param(param_id, value);
    }
}

impl PluginNode {
    /// Stage a parameter change by CLAP id, applied at the next `process()`.
    fn stage_param(&mut self, param_id: clap_id, value: f32) {
        self.events.staged.push(StagedEvent::Param {
            time: 0,
            param_id,
            value: value as f64,
        });
    }
}
