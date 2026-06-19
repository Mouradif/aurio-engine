//! Safe-ish hosting layer over the raw CLAP ABI.
//!
//! Owns the dynamic library, the initialized entry, and a created plugin
//! instance, exposing just enough to activate and run `process()`. The library
//! is loaded with `libloading`; everything else is hand-rolled against
//! [`super::sys`].

// This module is a thin hand-rolled FFI wrapper: every `unsafe fn` body is one
// contiguous block of pointer work, so the edition-2024 requirement to re-mark
// each op `unsafe` adds noise without adding clarity. Opt the file out.
#![allow(unsafe_op_in_unsafe_fn)]

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::path::{Path, PathBuf};
use std::ptr;

use libloading::Library;

use super::sys::*;

/// A loaded, initialized CLAP plugin instance, ready to be activated.
///
/// Field order matters for drop: the plugin is destroyed, then the entry is
/// deinited, then the library is unloaded — declared in that order so Rust's
/// drop glue (top-to-bottom) tears down correctly.
pub struct ClapPlugin {
    plugin: *const clap_plugin,
    entry: *const clap_plugin_entry,
    /// Boxed so the pointer we hand to the plugin stays stable for the
    /// instance's lifetime even if `ClapPlugin` moves.
    _host: Box<clap_host>,
    /// Keeps the dylib mapped. Dropped last.
    _lib: Library,
    descriptor_name: String,
    activated: bool,
}

// The CLAP audio thread contract: once activated, `process()` is called from a
// single audio thread. We move the whole instance to that thread, so it must be
// Send. The raw pointers are owned exclusively by this struct.
unsafe impl Send for ClapPlugin {}

/// One discovered parameter: its stable CLAP id plus display name and range.
#[derive(Clone, Debug)]
pub struct ParamInfo {
    pub id: clap_id,
    pub name: String,
    pub min: f64,
    pub max: f64,
    pub default: f64,
}

#[derive(Debug)]
pub enum HostError {
    Load(String),
    NoEntry(String),
    InitFailed,
    NoFactory,
    NotFound(String),
    CreateFailed,
    PluginInitFailed,
    ActivateFailed,
    StartFailed,
}

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HostError::Load(e) => write!(f, "failed to load CLAP library: {e}"),
            HostError::NoEntry(p) => write!(f, "no `clap_entry` symbol in {p}"),
            HostError::InitFailed => write!(f, "clap_entry.init() returned false"),
            HostError::NoFactory => write!(f, "plugin has no plugin-factory"),
            HostError::NotFound(id) => write!(f, "plugin id '{id}' not found in bundle"),
            HostError::CreateFailed => write!(f, "create_plugin() returned null"),
            HostError::PluginInitFailed => write!(f, "plugin.init() returned false"),
            HostError::ActivateFailed => write!(f, "plugin.activate() returned false"),
            HostError::StartFailed => write!(f, "plugin.start_processing() returned false"),
        }
    }
}

impl std::error::Error for HostError {}

/// Resolve the actual dynamic library path inside a `.clap`.
///
/// On macOS a `.clap` is a bundle directory; the dylib lives at
/// `Contents/MacOS/<name>`. On other platforms the `.clap` *is* the shared
/// object. If the path is already a file we use it directly.
fn resolve_library_path(path: &Path) -> PathBuf {
    if path.is_dir() {
        let macos = path.join("Contents/MacOS");
        if let Ok(entries) = std::fs::read_dir(&macos) {
            // A bundle's MacOS dir normally holds exactly one binary.
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() {
                    return p;
                }
            }
        }
    }
    path.to_path_buf()
}

// Host callbacks. Aurio acts as a "free running" host: it never asks the plugin
// to restart, and never schedules main-thread callbacks (we don't drive a GUI).
// These are deliberately inert.
unsafe extern "C" fn host_get_extension(
    _host: *const clap_host,
    _id: *const c_char,
) -> *const c_void {
    ptr::null()
}
unsafe extern "C" fn host_request_restart(_host: *const clap_host) {}
unsafe extern "C" fn host_request_process(_host: *const clap_host) {}
unsafe extern "C" fn host_request_callback(_host: *const clap_host) {}

impl ClapPlugin {
    /// Load `path` (a `.clap` bundle or dylib) and instantiate the plugin whose
    /// id is `plugin_id`. The DSO is initialized and `plugin.init()` is called;
    /// the instance is left deactivated.
    pub fn load(path: &Path, plugin_id: &str) -> Result<Self, HostError> {
        let lib_path = resolve_library_path(path);

        // SAFETY: loading an arbitrary dylib is inherently unsafe; the caller
        // vouches for the plugin path. Library init runs at load time.
        let lib = unsafe { Library::new(&lib_path) }
            .map_err(|e| HostError::Load(format!("{}: {e}", lib_path.display())))?;

        // The entry point is the exported `clap_entry` symbol.
        let entry: *const clap_plugin_entry = unsafe {
            let sym = lib
                .get::<*const clap_plugin_entry>(b"clap_entry\0")
                .map_err(|_| HostError::NoEntry(lib_path.display().to_string()))?;
            *sym
        };
        if entry.is_null() {
            return Err(HostError::NoEntry(lib_path.display().to_string()));
        }

        // init(plugin_path) — pass the *bundle* path, not the inner dylib, per
        // the spec (macOS hands the plugin its bundle).
        let bundle_cstr = CString::new(path.to_string_lossy().into_owned())
            .map_err(|_| HostError::Load("path contains NUL".into()))?;
        unsafe {
            let init = (*entry).init.ok_or(HostError::InitFailed)?;
            if !init(bundle_cstr.as_ptr()) {
                return Err(HostError::InitFailed);
            }
        }

        // From here, on any failure we must deinit the entry before returning.
        let result = Self::create_after_init(lib, entry, path, plugin_id);
        if result.is_err() {
            unsafe {
                if let Some(deinit) = (*entry).deinit {
                    deinit();
                }
            }
        }
        result
    }

    fn create_after_init(
        lib: Library,
        entry: *const clap_plugin_entry,
        bundle_path: &Path,
        plugin_id: &str,
    ) -> Result<Self, HostError> {
        let mut host = Box::new(clap_host {
            clap_version: CLAP_VERSION,
            host_data: ptr::null_mut(),
            name: c"Aurio".as_ptr(),
            vendor: c"Aurio".as_ptr(),
            url: c"https://github.com/aurio".as_ptr(),
            version: c"0.1.0".as_ptr(),
            get_extension: Some(host_get_extension),
            request_restart: Some(host_request_restart),
            request_process: Some(host_request_process),
            request_callback: Some(host_request_callback),
        });

        let factory = unsafe {
            let get_factory = (*entry).get_factory.ok_or(HostError::NoFactory)?;
            let ptr = get_factory(CLAP_PLUGIN_FACTORY_ID.as_ptr() as *const c_char)
                as *const clap_plugin_factory;
            if ptr.is_null() {
                return Err(HostError::NoFactory);
            }
            ptr
        };

        // Confirm the requested id exists (and capture its display name) before
        // creating, so we can give a clear error if it's absent.
        let desc_name = unsafe { find_descriptor(factory, plugin_id)? };

        let id_cstr =
            CString::new(plugin_id).map_err(|_| HostError::NotFound(plugin_id.into()))?;
        let plugin = unsafe {
            let create = (*factory).create_plugin.ok_or(HostError::CreateFailed)?;
            let p = create(factory, host.as_mut() as *mut clap_host, id_cstr.as_ptr());
            if p.is_null() {
                return Err(HostError::CreateFailed);
            }
            p
        };

        unsafe {
            let init = (*plugin).init.ok_or(HostError::PluginInitFailed)?;
            if !init(plugin) {
                if let Some(destroy) = (*plugin).destroy {
                    destroy(plugin);
                }
                return Err(HostError::PluginInitFailed);
            }
        }

        let _ = bundle_path;
        Ok(ClapPlugin {
            plugin,
            entry,
            _host: host,
            _lib: lib,
            descriptor_name: desc_name,
            activated: false,
        })
    }

    pub fn name(&self) -> &str {
        &self.descriptor_name
    }

    /// Enumerate the plugin's automatable parameters via the `params` extension.
    /// Returns an empty list if the plugin doesn't implement it. [main-thread]
    pub fn params(&self) -> Vec<ParamInfo> {
        let ext = unsafe { self.get_extension::<clap_plugin_params>(CLAP_EXT_PARAMS) };
        let Some(params) = ext else {
            return Vec::new();
        };
        let mut out = Vec::new();
        unsafe {
            let count = match (*params).count {
                Some(f) => f(self.plugin),
                None => return out,
            };
            let Some(get_info) = (*params).get_info else {
                return out;
            };
            for i in 0..count {
                let mut info: clap_param_info = std::mem::zeroed();
                if get_info(self.plugin, i, &mut info) {
                    out.push(ParamInfo {
                        id: info.id,
                        name: cstr_to_string(info.name.as_ptr()),
                        min: info.min_value,
                        max: info.max_value,
                        default: info.default_value,
                    });
                }
            }
        }
        out
    }

    /// Activate at the given sample rate and block bounds, then start
    /// processing. After this the plugin is ready for `process()`.
    pub fn activate(
        &mut self,
        sample_rate: f64,
        min_frames: u32,
        max_frames: u32,
    ) -> Result<(), HostError> {
        unsafe {
            let activate = (*self.plugin).activate.ok_or(HostError::ActivateFailed)?;
            if !activate(self.plugin, sample_rate, min_frames, max_frames) {
                return Err(HostError::ActivateFailed);
            }
            self.activated = true;
            let start = (*self.plugin)
                .start_processing
                .ok_or(HostError::StartFailed)?;
            if !start(self.plugin) {
                return Err(HostError::StartFailed);
            }
        }
        Ok(())
    }

    /// Raw plugin pointer, for the node's `process()` calls.
    pub fn plugin_ptr(&self) -> *const clap_plugin {
        self.plugin
    }

    unsafe fn get_extension<T>(&self, id: &[u8]) -> Option<*const T> {
        let get_extension = (*self.plugin).get_extension?;
        let ptr = get_extension(self.plugin, id.as_ptr() as *const c_char) as *const T;
        if ptr.is_null() { None } else { Some(ptr) }
    }
}

impl Drop for ClapPlugin {
    fn drop(&mut self) {
        unsafe {
            if self.activated {
                if let Some(stop) = (*self.plugin).stop_processing {
                    stop(self.plugin);
                }
                if let Some(deactivate) = (*self.plugin).deactivate {
                    deactivate(self.plugin);
                }
                self.activated = false;
            }
            if let Some(destroy) = (*self.plugin).destroy {
                destroy(self.plugin);
            }
            if let Some(deinit) = (*self.entry).deinit {
                deinit();
            }
        }
        // _lib unloads here.
    }
}

/// Walk the factory's descriptors and return the display name for `wanted_id`.
unsafe fn find_descriptor(
    factory: *const clap_plugin_factory,
    wanted_id: &str,
) -> Result<String, HostError> {
    let count = match (*factory).get_plugin_count {
        Some(f) => f(factory),
        None => return Err(HostError::NoFactory),
    };
    let Some(get_desc) = (*factory).get_plugin_descriptor else {
        return Err(HostError::NoFactory);
    };
    for i in 0..count {
        let desc = get_desc(factory, i);
        if desc.is_null() {
            continue;
        }
        let id = cstr_to_string((*desc).id);
        if id == wanted_id {
            return Ok(cstr_to_string((*desc).name));
        }
    }
    Err(HostError::NotFound(wanted_id.into()))
}

/// Read a possibly-null C string into an owned `String` (lossy, empty if null).
fn cstr_to_string(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}
