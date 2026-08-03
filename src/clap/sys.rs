//! Hand-rolled raw FFI bindings for the CLAP plugin ABI (v1.2.8).
//!
//! Transcribed directly from the CLAP C headers (`free-audio/clap`). Only the
//! subset Aurio needs to *host* an instrument/effect is declared: the entry
//! point, plugin factory, plugin instance, host vtable, the process/audio/event
//! structs, and the `params` extension. GUI, state, threading, and the many
//! draft extensions are intentionally omitted.
//!
//! ABI notes:
//! - On macOS/Linux `CLAP_ABI` expands to nothing, so every function pointer is
//!   plain `extern "C"`. (On Windows it would be `__cdecl`, which is also the
//!   default for `extern "C"` there.)
//! - C function pointers are nullable, so each is modelled as
//!   `Option<unsafe extern "C" fn(...)>` and must be null-checked before use.
//! - All structs are `#[repr(C)]` to match the C layout exactly.
#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_void};

pub const CLAP_VERSION_MAJOR: u32 = 1;
pub const CLAP_VERSION_MINOR: u32 = 2;
pub const CLAP_VERSION_REVISION: u32 = 8;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct clap_version {
    pub major: u32,
    pub minor: u32,
    pub revision: u32,
}

pub const CLAP_VERSION: clap_version = clap_version {
    major: CLAP_VERSION_MAJOR,
    minor: CLAP_VERSION_MINOR,
    revision: CLAP_VERSION_REVISION,
};

pub type clap_id = u32;
pub const CLAP_INVALID_ID: clap_id = u32::MAX;

/// `const char[]` factory id: "clap.plugin-factory".
pub const CLAP_PLUGIN_FACTORY_ID: &[u8] = b"clap.plugin-factory\0";
/// `const char[]` params extension id: "clap.params".
pub const CLAP_EXT_PARAMS: &[u8] = b"clap.params\0";

pub const CLAP_NAME_SIZE: usize = 256;
pub const CLAP_PATH_SIZE: usize = 1024;

// ---------------------------------------------------------------------------
// Entry point (entry.h)
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct clap_plugin_entry {
    pub clap_version: clap_version,
    pub init: Option<unsafe extern "C" fn(plugin_path: *const c_char) -> bool>,
    pub deinit: Option<unsafe extern "C" fn()>,
    pub get_factory: Option<unsafe extern "C" fn(factory_id: *const c_char) -> *const c_void>,
}

// ---------------------------------------------------------------------------
// Plugin factory (factory/plugin-factory.h)
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct clap_plugin_factory {
    pub get_plugin_count: Option<unsafe extern "C" fn(factory: *const clap_plugin_factory) -> u32>,
    pub get_plugin_descriptor: Option<
        unsafe extern "C" fn(
            factory: *const clap_plugin_factory,
            index: u32,
        ) -> *const clap_plugin_descriptor,
    >,
    pub create_plugin: Option<
        unsafe extern "C" fn(
            factory: *const clap_plugin_factory,
            host: *const clap_host,
            plugin_id: *const c_char,
        ) -> *const clap_plugin,
    >,
}

// ---------------------------------------------------------------------------
// Plugin descriptor + instance (plugin.h)
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct clap_plugin_descriptor {
    pub clap_version: clap_version,
    pub id: *const c_char,
    pub name: *const c_char,
    pub vendor: *const c_char,
    pub url: *const c_char,
    pub manual_url: *const c_char,
    pub support_url: *const c_char,
    pub version: *const c_char,
    pub description: *const c_char,
    pub features: *const *const c_char,
}

#[repr(C)]
pub struct clap_plugin {
    pub desc: *const clap_plugin_descriptor,
    pub plugin_data: *mut c_void,
    pub init: Option<unsafe extern "C" fn(plugin: *const clap_plugin) -> bool>,
    pub destroy: Option<unsafe extern "C" fn(plugin: *const clap_plugin)>,
    pub activate: Option<
        unsafe extern "C" fn(
            plugin: *const clap_plugin,
            sample_rate: f64,
            min_frames_count: u32,
            max_frames_count: u32,
        ) -> bool,
    >,
    pub deactivate: Option<unsafe extern "C" fn(plugin: *const clap_plugin)>,
    pub start_processing: Option<unsafe extern "C" fn(plugin: *const clap_plugin) -> bool>,
    pub stop_processing: Option<unsafe extern "C" fn(plugin: *const clap_plugin)>,
    pub reset: Option<unsafe extern "C" fn(plugin: *const clap_plugin)>,
    pub process: Option<
        unsafe extern "C" fn(
            plugin: *const clap_plugin,
            process: *const clap_process,
        ) -> clap_process_status,
    >,
    pub get_extension: Option<
        unsafe extern "C" fn(plugin: *const clap_plugin, id: *const c_char) -> *const c_void,
    >,
    pub on_main_thread: Option<unsafe extern "C" fn(plugin: *const clap_plugin)>,
}

// ---------------------------------------------------------------------------
// Host (host.h)
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct clap_host {
    pub clap_version: clap_version,
    pub host_data: *mut c_void,
    pub name: *const c_char,
    pub vendor: *const c_char,
    pub url: *const c_char,
    pub version: *const c_char,
    pub get_extension: Option<
        unsafe extern "C" fn(host: *const clap_host, extension_id: *const c_char) -> *const c_void,
    >,
    pub request_restart: Option<unsafe extern "C" fn(host: *const clap_host)>,
    pub request_process: Option<unsafe extern "C" fn(host: *const clap_host)>,
    pub request_callback: Option<unsafe extern "C" fn(host: *const clap_host)>,
}

// The host vtable is shared across threads via raw pointers. We only ever read
// it, and the callbacks we install are no-ops that touch nothing, so it is safe
// to mark it Send/Sync for storage alongside the plugin node.
unsafe impl Send for clap_host {}
unsafe impl Sync for clap_host {}

// ---------------------------------------------------------------------------
// Audio buffers + process (audio-buffer.h, process.h)
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct clap_audio_buffer {
    pub data32: *mut *mut f32,
    pub data64: *mut *mut f64,
    pub channel_count: u32,
    pub latency: u32,
    pub constant_mask: u64,
}

pub type clap_process_status = i32;
pub const CLAP_PROCESS_ERROR: clap_process_status = 0;
pub const CLAP_PROCESS_CONTINUE: clap_process_status = 1;

#[repr(C)]
pub struct clap_process {
    pub steady_time: i64,
    pub frames_count: u32,
    pub transport: *const c_void, // clap_event_transport; unused, may be null
    pub audio_inputs: *const clap_audio_buffer,
    pub audio_outputs: *mut clap_audio_buffer,
    pub audio_inputs_count: u32,
    pub audio_outputs_count: u32,
    pub in_events: *const clap_input_events,
    pub out_events: *const clap_output_events,
}

// ---------------------------------------------------------------------------
// Events (events.h)
// ---------------------------------------------------------------------------

pub const CLAP_CORE_EVENT_SPACE_ID: u16 = 0;

pub const CLAP_EVENT_NOTE_ON: u16 = 0;
pub const CLAP_EVENT_NOTE_OFF: u16 = 1;
pub const CLAP_EVENT_PARAM_VALUE: u16 = 5;

pub const CLAP_EVENT_IS_LIVE: u32 = 1 << 0;
pub const CLAP_EVENT_DONT_RECORD: u32 = 1 << 1;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct clap_event_header {
    pub size: u32,
    pub time: u32,
    pub space_id: u16,
    pub type_: u16,
    pub flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct clap_event_note {
    pub header: clap_event_header,
    pub note_id: i32,
    pub port_index: i16,
    pub channel: i16,
    pub key: i16,
    pub velocity: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct clap_event_param_value {
    pub header: clap_event_header,
    pub param_id: clap_id,
    pub cookie: *mut c_void,
    pub note_id: i32,
    pub port_index: i16,
    pub channel: i16,
    pub key: i16,
    pub value: f64,
}

#[repr(C)]
pub struct clap_input_events {
    pub ctx: *mut c_void,
    pub size: Option<unsafe extern "C" fn(list: *const clap_input_events) -> u32>,
    pub get: Option<
        unsafe extern "C" fn(
            list: *const clap_input_events,
            index: u32,
        ) -> *const clap_event_header,
    >,
}

#[repr(C)]
pub struct clap_output_events {
    pub ctx: *mut c_void,
    pub try_push: Option<
        unsafe extern "C" fn(
            list: *const clap_output_events,
            event: *const clap_event_header,
        ) -> bool,
    >,
}

// ---------------------------------------------------------------------------
// Params extension (ext/params.h)
// ---------------------------------------------------------------------------

pub type clap_param_info_flags = u32;

#[repr(C)]
pub struct clap_param_info {
    pub id: clap_id,
    pub flags: clap_param_info_flags,
    pub cookie: *mut c_void,
    pub name: [c_char; CLAP_NAME_SIZE],
    pub module: [c_char; CLAP_PATH_SIZE],
    pub min_value: f64,
    pub max_value: f64,
    pub default_value: f64,
}

#[repr(C)]
pub struct clap_plugin_params {
    pub count: Option<unsafe extern "C" fn(plugin: *const clap_plugin) -> u32>,
    pub get_info: Option<
        unsafe extern "C" fn(
            plugin: *const clap_plugin,
            param_index: u32,
            param_info: *mut clap_param_info,
        ) -> bool,
    >,
    pub get_value: Option<
        unsafe extern "C" fn(
            plugin: *const clap_plugin,
            param_id: clap_id,
            out_value: *mut f64,
        ) -> bool,
    >,
    pub value_to_text: Option<
        unsafe extern "C" fn(
            plugin: *const clap_plugin,
            param_id: clap_id,
            value: f64,
            out_buffer: *mut c_char,
            out_buffer_capacity: u32,
        ) -> bool,
    >,
    pub text_to_value: Option<
        unsafe extern "C" fn(
            plugin: *const clap_plugin,
            param_id: clap_id,
            param_value_text: *const c_char,
            out_value: *mut f64,
        ) -> bool,
    >,
    pub flush: Option<
        unsafe extern "C" fn(
            plugin: *const clap_plugin,
            in_: *const clap_input_events,
            out: *const clap_output_events,
        ),
    >,
}
