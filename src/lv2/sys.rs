//! Hand-rolled raw FFI bindings for the LV2 plugin ABI.
//!
//! Transcribed directly from the LV2 C headers (`lv2/lv2`, in
//! `/Users/mkejji/dev/c/lv2`): `core/lv2.h`, `urid/urid.h`, `atom/atom.h`. Only
//! the subset needed to *host* an instrument/effect is declared.
//!
//! ABI notes:
//! - Plugin/host functions are plain `extern "C"` (LV2 has no per-platform
//!   calling-convention macro like CLAP).
//! - Nullable C function pointers are `Option<unsafe extern "C" fn(...)>`.
//! - Atom structs are `#[repr(C)]`; events within a Sequence are **64-bit
//!   aligned** (the writer in `node.rs` pads each event up to 8 bytes).
#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_void};

// ---------------------------------------------------------------------------
// Core (core/lv2.h)
// ---------------------------------------------------------------------------

pub type LV2_Handle = *mut c_void;

/// A host->plugin feature: a URI plus opaque data (e.g. the URID map).
#[repr(C)]
pub struct LV2_Feature {
    pub uri: *const c_char,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct LV2_Descriptor {
    pub uri: *const c_char,
    pub instantiate: Option<
        unsafe extern "C" fn(
            descriptor: *const LV2_Descriptor,
            sample_rate: f64,
            bundle_path: *const c_char,
            features: *const *const LV2_Feature,
        ) -> LV2_Handle,
    >,
    pub connect_port:
        Option<unsafe extern "C" fn(instance: LV2_Handle, port: u32, data_location: *mut c_void)>,
    pub activate: Option<unsafe extern "C" fn(instance: LV2_Handle)>,
    pub run: Option<unsafe extern "C" fn(instance: LV2_Handle, sample_count: u32)>,
    pub deactivate: Option<unsafe extern "C" fn(instance: LV2_Handle)>,
    pub cleanup: Option<unsafe extern "C" fn(instance: LV2_Handle)>,
    pub extension_data: Option<unsafe extern "C" fn(uri: *const c_char) -> *const c_void>,
}

/// `const LV2_Descriptor* lv2_descriptor(uint32_t index)` — the library entry.
pub type LV2_Descriptor_Function =
    unsafe extern "C" fn(index: u32) -> *const LV2_Descriptor;

// ---------------------------------------------------------------------------
// URID (urid/urid.h)
// ---------------------------------------------------------------------------

pub type LV2_URID = u32;
pub type LV2_URID_Map_Handle = *mut c_void;

/// The `urid:map` feature: interns URI strings into stable integer ids.
#[repr(C)]
pub struct LV2_URID_Map {
    pub handle: LV2_URID_Map_Handle,
    pub map: Option<unsafe extern "C" fn(handle: LV2_URID_Map_Handle, uri: *const c_char) -> LV2_URID>,
}

// Feature URIs we supply / require (null-terminated for FFI).
pub const LV2_URID_MAP_URI: &[u8] = b"http://lv2plug.in/ns/ext/urid#map\0";
pub const LV2_BUF_SIZE_BOUNDED: &[u8] =
    b"http://lv2plug.in/ns/ext/buf-size#boundedBlockLength\0";

// URIs we intern through the map.
pub const LV2_ATOM_SEQUENCE: &[u8] = b"http://lv2plug.in/ns/ext/atom#Sequence\0";
pub const LV2_MIDI_EVENT: &[u8] = b"http://lv2plug.in/ns/ext/midi#MidiEvent\0";

// ---------------------------------------------------------------------------
// Atom (atom/atom.h)
// ---------------------------------------------------------------------------

/// The header of an `atom:Atom`: `{ size, type }`. `size` excludes this header.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LV2_Atom {
    pub size: u32,
    pub type_: u32,
}

/// The body of an `atom:Sequence`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LV2_Atom_Sequence_Body {
    pub unit: u32,
    pub pad: u32,
}

/// An `atom:Sequence`: an atom header followed by a body, then a series of
/// 64-bit-aligned `LV2_Atom_Event`s.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LV2_Atom_Sequence {
    pub atom: LV2_Atom,
    pub body: LV2_Atom_Sequence_Body,
}

/// The header of an `atom:Event` inside a Sequence. NOTE: this is *not* itself
/// an `LV2_Atom` — the 8-byte time field (a union of i64 frames / f64 beats)
/// comes first, then the event body's atom header. For `run()` the time unit
/// is audio frames, so we always use `time_frames`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LV2_Atom_Event {
    pub time_frames: i64,
    pub body: LV2_Atom,
    // The event payload (e.g. raw MIDI bytes) follows here.
}
