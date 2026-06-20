//! An LV2 plugin wrapped as an Aurio graph node implementing both
//! [`Instrument`] and [`Effect`].
//!
//! LV2 is a natural fit for Aurio's block engine: audio ports are already
//! planar `float*` buffers (no de-interleave needed beyond splitting L/R), and
//! `run(n)` is block-based. The work here is the wiring: connect every port to a
//! stable buffer once, write staged MIDI into an Atom sequence each block, run,
//! then read the audio outputs back into stereo `Frame`s.

use std::os::raw::c_void;
use std::path::Path;

use crate::audio_graph::MAX_BLOCK;
use crate::effect::Effect;
use crate::frame::Frame;
use crate::instrument::Instrument;
use crate::midi::NoteEvent;

use super::host::{HostError, Lv2Plugin};
use super::sys::*;
use super::ttl::{self, PortDescriptor, PortDirection, PortKind, TtlError};

/// Size of the atom input-event buffer, in bytes. Generous for a block's worth
/// of note on/off events.
const EVENT_BUF_BYTES: usize = 16 * 1024;

#[derive(Debug)]
pub enum Lv2Error {
    Ttl(TtlError),
    Host(HostError),
}

impl std::fmt::Display for Lv2Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Lv2Error::Ttl(e) => write!(f, "{e}"),
            Lv2Error::Host(e) => write!(f, "{e}"),
        }
    }
}
impl std::error::Error for Lv2Error {}
impl From<TtlError> for Lv2Error {
    fn from(e: TtlError) -> Self {
        Lv2Error::Ttl(e)
    }
}
impl From<HostError> for Lv2Error {
    fn from(e: HostError) -> Self {
        Lv2Error::Host(e)
    }
}

/// One discovered control parameter: its port index (used as the automation
/// id), display name, LV2 symbol, and range. Unlike CLAP, LV2 params have both
/// a human name and a stable `symbol`, and either can be used to bind them.
#[derive(Clone, Debug)]
pub struct ParamInfo {
    pub id: u32,
    pub name: String,
    pub symbol: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

/// A staged note for the next `run()`, applied at frame `time`.
#[derive(Clone, Copy)]
struct StagedNote {
    time: u32,
    /// Raw 3-byte MIDI message.
    data: [u8; 3],
}

/// One control input port we can automate, keyed by symbol/name.
struct ControlParam {
    index: u32,
    symbol: String,
    name: String,
    /// The single float cell the plugin reads each run.
    value: Box<f32>,
    min: f32,
    max: f32,
}

pub struct Lv2Node {
    id: usize,
    plugin: Lv2Plugin,

    // Planar audio buffers. Stereo in/out; mono plugins use only [0].
    audio_in: [Vec<f32>; 2],
    audio_out: [Vec<f32>; 2],
    audio_in_ports: Vec<u32>,  // port indices, in channel order
    audio_out_ports: Vec<u32>, // port indices, in channel order

    // Control input params (automatable) and a sink for control outputs / other
    // ports we must connect but ignore.
    controls: Vec<ControlParam>,
    _dummy_control: Box<f32>,

    // The MIDI atom-sequence input port (if the plugin has one).
    midi_in_port: Option<u32>,
    event_buf: Vec<u8>,
    // A dummy atom buffer for atom output ports we connect but ignore.
    atom_out_buf: Vec<u8>,

    // Interned URIDs for writing the event sequence.
    urid_sequence: LV2_URID,
    urid_midi: LV2_URID,

    staged: Vec<StagedNote>,
}

unsafe impl Send for Lv2Node {}

impl Lv2Node {
    /// Load and instantiate the plugin at `bundle_dir` with URI `plugin_uri`,
    /// activate it at `sample_rate`, and connect all ports to stable buffers.
    pub fn load(
        id: usize,
        bundle_dir: &Path,
        plugin_uri: &str,
        sample_rate: f32,
    ) -> Result<Self, Lv2Error> {
        let manifest = ttl::load(bundle_dir, plugin_uri)?;
        let plugin = Lv2Plugin::instantiate(&manifest, plugin_uri, sample_rate as f64)?;

        let urid_sequence = plugin.urid().lookup(LV2_ATOM_SEQUENCE);
        let urid_midi = plugin.urid().lookup(LV2_MIDI_EVENT);

        let mut node = Lv2Node {
            id,
            plugin,
            audio_in: [vec![0.0; MAX_BLOCK], vec![0.0; MAX_BLOCK]],
            audio_out: [vec![0.0; MAX_BLOCK], vec![0.0; MAX_BLOCK]],
            audio_in_ports: Vec::new(),
            audio_out_ports: Vec::new(),
            controls: Vec::new(),
            _dummy_control: Box::new(0.0),
            midi_in_port: None,
            event_buf: vec![0; EVENT_BUF_BYTES],
            atom_out_buf: vec![0; EVENT_BUF_BYTES],
            urid_sequence,
            urid_midi,
            staged: Vec::with_capacity(256),
        };
        node.wire_ports(&manifest.ports);
        Ok(node)
    }

    /// Categorize and connect every port to a stable buffer. Called once.
    fn wire_ports(&mut self, ports: &[PortDescriptor]) {
        // First pass: build the control list and identify audio/midi ports so
        // buffer Vecs aren't reallocated after we hand out pointers.
        for p in ports {
            match (p.kind, p.direction) {
                (PortKind::Audio, PortDirection::Input) => self.audio_in_ports.push(p.index),
                (PortKind::Audio, PortDirection::Output) => self.audio_out_ports.push(p.index),
                (PortKind::Control, PortDirection::Input) => {
                    self.controls.push(ControlParam {
                        index: p.index,
                        symbol: p.symbol.clone(),
                        name: p.name.clone(),
                        value: Box::new(p.default.unwrap_or(0.0)),
                        min: p.min.unwrap_or(f32::NEG_INFINITY),
                        max: p.max.unwrap_or(f32::INFINITY),
                    });
                }
                (PortKind::AtomSequence, PortDirection::Input) if p.supports_midi => {
                    self.midi_in_port = Some(p.index);
                }
                _ => {}
            }
        }

        // Second pass: connect_port everything. Pointers below are into Vecs
        // that won't reallocate for the node's lifetime.
        for (ch, &port) in self.audio_in_ports.iter().enumerate().take(2) {
            self.plugin
                .connect_port(port, self.audio_in[ch].as_mut_ptr() as *mut c_void);
        }
        for (ch, &port) in self.audio_out_ports.iter().enumerate().take(2) {
            self.plugin
                .connect_port(port, self.audio_out[ch].as_mut_ptr() as *mut c_void);
        }
        for c in &mut self.controls {
            self.plugin
                .connect_port(c.index, c.value.as_mut() as *mut f32 as *mut c_void);
        }
        if let Some(port) = self.midi_in_port {
            self.plugin
                .connect_port(port, self.event_buf.as_mut_ptr() as *mut c_void);
        }

        // Any port we didn't explicitly handle (extra audio channels beyond 2,
        // control outputs, atom outputs, "other") still must be connected so the
        // plugin doesn't read/write an unconnected pointer. Connect them to a
        // dummy sink sized appropriately.
        for p in ports {
            let handled = self.audio_in_ports.iter().take(2).any(|&i| i == p.index)
                || self.audio_out_ports.iter().take(2).any(|&i| i == p.index)
                || self.controls.iter().any(|c| c.index == p.index)
                || self.midi_in_port == Some(p.index);
            if handled {
                continue;
            }
            let ptr = match p.kind {
                PortKind::AtomSequence => self.atom_out_buf.as_mut_ptr() as *mut c_void,
                _ => self._dummy_control.as_mut() as *mut f32 as *mut c_void,
            };
            self.plugin.connect_port(p.index, ptr);
        }
    }

    /// The automatable control params (index, symbol, name, range).
    pub fn params(&self) -> Vec<ParamInfo> {
        self.controls
            .iter()
            .map(|c| ParamInfo {
                id: c.index,
                name: c.name.clone(),
                symbol: c.symbol.clone(),
                min: c.min,
                max: c.max,
                default: *c.value,
            })
            .collect()
    }

    /// Resolve a param name *or* symbol to its port index (case-insensitive).
    pub fn param_id_by_name(&self, name: &str) -> Option<u32> {
        self.controls
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name) || c.symbol.eq_ignore_ascii_case(name))
            .map(|c| c.index)
    }

    /// Set a control param by its port index, clamped to its range.
    fn set_control(&mut self, index: u32, value: f32) {
        if let Some(c) = self.controls.iter_mut().find(|c| c.index == index) {
            *c.value = value.clamp(c.min, c.max);
        }
    }

    /// Write the staged notes into `event_buf` as an LV2 Atom Sequence, ready
    /// for the plugin to read this block. Clears the staging buffer.
    fn build_event_sequence(&mut self) {
        if self.midi_in_port.is_none() {
            self.staged.clear();
            return;
        }

        // Header: an LV2_Atom_Sequence whose `atom.size` covers the body plus
        // all events. We write events right after the header, each 64-bit
        // aligned, with a 3-byte MIDI payload.
        let header_len = std::mem::size_of::<LV2_Atom_Sequence>();
        let mut offset = header_len;

        for note in &self.staged {
            let event_header = std::mem::size_of::<LV2_Atom_Event>();
            let total = event_header + 3; // 3 MIDI bytes
            let padded = (total + 7) & !7; // round up to 8
            if offset + padded > self.event_buf.len() {
                break; // out of room — drop the rest
            }

            // LV2_Atom_Event { i64 time_frames; LV2_Atom { u32 size; u32 type } }
            let ev = LV2_Atom_Event {
                time_frames: note.time as i64,
                body: LV2_Atom {
                    size: 3, // MIDI payload size
                    type_: self.urid_midi,
                },
            };
            unsafe {
                let dst = self.event_buf.as_mut_ptr().add(offset);
                std::ptr::write_unaligned(dst as *mut LV2_Atom_Event, ev);
                let payload = dst.add(event_header);
                std::ptr::copy_nonoverlapping(note.data.as_ptr(), payload, 3);
            }
            offset += padded;
        }

        // Now write the sequence header. body.size = everything after the
        // atom header (i.e. the sequence body struct + all events).
        let seq = LV2_Atom_Sequence {
            atom: LV2_Atom {
                size: (offset - std::mem::size_of::<LV2_Atom>()) as u32,
                type_: self.urid_sequence,
            },
            body: LV2_Atom_Sequence_Body { unit: 0, pad: 0 },
        };
        unsafe {
            std::ptr::write_unaligned(
                self.event_buf.as_mut_ptr() as *mut LV2_Atom_Sequence,
                seq,
            );
        }

        self.staged.clear();
    }

    /// Fill the planar audio-input buffers from `frames` (used by effects before
    /// `run`). De-interleaves L/R; mono-input plugins use only channel 0.
    fn fill_audio_input(&mut self, frames: &[Frame]) {
        let n = frames.len().min(MAX_BLOCK);
        for (i, frame) in frames[..n].iter().enumerate() {
            self.audio_in[0][i] = frame.l;
            self.audio_in[1][i] = frame.r;
        }
    }

    /// Zero the planar audio input for `n` frames (instruments have no input).
    fn silence_audio_input(&mut self, n: usize) {
        let n = n.min(MAX_BLOCK);
        self.audio_in[0][..n].fill(0.0);
        self.audio_in[1][..n].fill(0.0);
    }

    /// Run the plugin for `n` frames. The planar audio-input buffers must
    /// already be filled (see [`fill_audio_input`]); instruments leave them as
    /// the previous block's silence. Output is read into `out`.
    fn run(&mut self, n: usize, out: &mut [Frame]) {
        let n = n.min(MAX_BLOCK).min(out.len());

        self.build_event_sequence();
        self.plugin.run(n as u32);

        // Read audio outputs back into stereo frames.
        match self.audio_out_ports.len() {
            0 => {
                for slot in out[..n].iter_mut() {
                    *slot = Frame::SILENCE;
                }
            }
            1 => {
                // Mono → duplicate to both channels.
                for (i, slot) in out[..n].iter_mut().enumerate() {
                    let v = self.audio_out[0][i];
                    *slot = Frame::new(v, v);
                }
            }
            _ => {
                for (i, slot) in out[..n].iter_mut().enumerate() {
                    *slot = Frame::new(self.audio_out[0][i], self.audio_out[1][i]);
                }
            }
        }
    }

    fn stage_note(&mut self, event: &NoteEvent) {
        // Channel 0. Note on 0x90, note off 0x80.
        let data = match event {
            NoteEvent::NoteOn { pitch, velocity } => [0x90, *pitch & 0x7F, *velocity & 0x7F],
            NoteEvent::NoteOff { pitch } => [0x80, *pitch & 0x7F, 0],
        };
        self.staged.push(StagedNote { time: 0, data });
    }
}

impl Instrument for Lv2Node {
    fn id(&self) -> usize {
        self.id
    }

    fn process_event(&mut self, event: &NoteEvent) {
        self.stage_note(event);
    }

    fn next_sample(&mut self, _sample_rate: f32) -> Frame {
        let mut out = [Frame::SILENCE; 1];
        self.silence_audio_input(1);
        self.run(1, &mut out);
        out[0]
    }

    fn render_block(&mut self, out: &mut [Frame], _sample_rate: f32) {
        self.silence_audio_input(out.len());
        self.run(out.len(), out);
    }

    fn set_param_by_id(&mut self, param_id: u32, value: f32) {
        self.set_control(param_id, value);
    }
}

impl Effect for Lv2Node {
    fn process(&mut self, frame: Frame) -> Frame {
        let mut out = [Frame::SILENCE; 1];
        self.fill_audio_input(&[frame]);
        self.run(1, &mut out);
        out[0]
    }

    fn set_param(&mut self, _idx: u8, _value: f32) {}

    fn process_block(&mut self, block: &mut [Frame]) {
        // De-interleave the block into the planar audio input first, then run
        // and overwrite the block with output. audio_in is separate storage, so
        // no aliasing and no allocation.
        let n = block.len().min(MAX_BLOCK);
        self.fill_audio_input(&block[..n]);
        self.run(n, &mut block[..n]);
    }

    fn set_param_by_id(&mut self, param_id: u32, value: f32) {
        self.set_control(param_id, value);
    }
}
