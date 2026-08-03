//! Safe-ish hosting layer over the raw LV2 ABI: load the bundle binary, find
//! the descriptor by URI, provide the `urid:map` feature, instantiate, activate.

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::sync::Mutex;

use libloading::Library;

use super::sys::*;
use super::ttl::PluginManifest;

/// A host-owned URI→id interner backing the `urid:map` feature. Boxed and kept
/// alive for the plugin instance's lifetime. The plugin may call `map` from the
/// audio thread, but the LV2 spec says plugins SHOULD cache URIDs at
/// instantiate, so the mutex is effectively uncontended after setup.
pub struct UridMap {
    inner: Mutex<UridInner>,
}

struct UridInner {
    map: HashMap<CString, LV2_URID>,
    next: LV2_URID,
}

impl UridMap {
    fn new() -> Box<Self> {
        Box::new(UridMap {
            inner: Mutex::new(UridInner {
                map: HashMap::new(),
                next: 1, // 0 is reserved per the spec
            }),
        })
    }

    fn intern(&self, uri: &CStr) -> LV2_URID {
        let mut inner = self.inner.lock().unwrap();
        if let Some(&id) = inner.map.get(uri) {
            return id;
        }
        let id = inner.next;
        inner.next += 1;
        inner.map.insert(uri.to_owned(), id);
        id
    }

    /// Look up a URID we expect to have interned (for our own event writing).
    pub fn lookup(&self, uri: &[u8]) -> LV2_URID {
        let c = CStr::from_bytes_with_nul(uri).expect("uri must be nul-terminated");
        self.intern(c)
    }
}

unsafe extern "C" fn urid_map_fn(handle: LV2_URID_Map_Handle, uri: *const c_char) -> LV2_URID {
    if handle.is_null() || uri.is_null() {
        return 0;
    }
    let map = unsafe { &*(handle as *const UridMap) };
    let uri = unsafe { CStr::from_ptr(uri) };
    map.intern(uri)
}

/// A loaded, activated LV2 plugin instance.
///
/// Drop order: deactivate + cleanup the instance, then unload the library. The
/// feature array and URID map are kept boxed so their addresses stay valid for
/// the whole instance lifetime.
pub struct Lv2Plugin {
    descriptor: *const LV2_Descriptor,
    handle: LV2_Handle,
    urid: Box<UridMap>,
    // Boxed/owned so the pointers we hand the plugin never move or free.
    _urid_map_feature: Box<LV2_URID_Map>,
    _map_uri: CString,
    _features: Vec<*const LV2_Feature>,
    _feature_storage: Box<[LV2_Feature]>,
    _lib: Library,
    activated: bool,
}

// The instance is created and run on the audio thread; raw pointers are owned
// exclusively here.
unsafe impl Send for Lv2Plugin {}

#[derive(Debug)]
pub enum HostError {
    Load(String),
    NoEntry,
    DescriptorNotFound(String),
    InstantiateFailed,
}

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HostError::Load(e) => write!(f, "failed to load LV2 binary: {e}"),
            HostError::NoEntry => write!(f, "no `lv2_descriptor` symbol in binary"),
            HostError::DescriptorNotFound(uri) => {
                write!(f, "binary has no descriptor for uri '{uri}'")
            }
            HostError::InstantiateFailed => write!(f, "instantiate() returned null"),
        }
    }
}

impl std::error::Error for HostError {}

impl Lv2Plugin {
    /// Load the plugin binary, find the descriptor matching `plugin_uri`,
    /// supply the URID map feature, and instantiate at `sample_rate`.
    pub fn instantiate(
        manifest: &PluginManifest,
        plugin_uri: &str,
        sample_rate: f64,
    ) -> Result<Self, HostError> {
        // SAFETY: loading an arbitrary dylib is inherently unsafe; the caller
        // vouches for the path.
        let lib = unsafe { Library::new(&manifest.binary) }
            .map_err(|e| HostError::Load(format!("{}: {e}", manifest.binary.display())))?;

        let entry: LV2_Descriptor_Function = unsafe {
            let sym = lib
                .get::<LV2_Descriptor_Function>(b"lv2_descriptor\0")
                .map_err(|_| HostError::NoEntry)?;
            *sym
        };

        // Enumerate descriptors until the URI matches (or NULL terminates).
        let descriptor = unsafe {
            let mut i = 0u32;
            loop {
                let desc = entry(i);
                if desc.is_null() {
                    return Err(HostError::DescriptorNotFound(plugin_uri.into()));
                }
                let uri = CStr::from_ptr((*desc).uri).to_string_lossy();
                if uri == plugin_uri {
                    break desc;
                }
                i += 1;
            }
        };

        // Build the URID map feature.
        let urid = UridMap::new();
        let urid_ptr = urid.as_ref() as *const UridMap as *mut c_void;
        let mut urid_map_feature = Box::new(LV2_URID_Map {
            handle: urid_ptr,
            map: Some(urid_map_fn),
        });

        // The feature's URI string must outlive the instance; keep it owned.
        let map_uri = CString::new(&LV2_URID_MAP_URI[..LV2_URID_MAP_URI.len() - 1]).unwrap();

        // Feature structs in a boxed slice (never reallocated), so the pointers
        // we collect into `features` stay valid for the instance's lifetime.
        let feature_storage: Box<[LV2_Feature]> = Box::new([LV2_Feature {
            uri: map_uri.as_ptr(),
            data: urid_map_feature.as_mut() as *mut LV2_URID_Map as *mut c_void,
        }]);
        let mut features: Vec<*const LV2_Feature> =
            feature_storage.iter().map(|f| f as *const _).collect();
        features.push(std::ptr::null()); // NULL terminator

        let bundle = CString::new(manifest.bundle_dir.to_string_lossy().into_owned())
            .map_err(|_| HostError::InstantiateFailed)?;

        let handle = unsafe {
            let instantiate = (*descriptor)
                .instantiate
                .ok_or(HostError::InstantiateFailed)?;
            instantiate(descriptor, sample_rate, bundle.as_ptr(), features.as_ptr())
        };
        if handle.is_null() {
            return Err(HostError::InstantiateFailed);
        }

        let mut plugin = Lv2Plugin {
            descriptor,
            handle,
            urid,
            _urid_map_feature: urid_map_feature,
            _map_uri: map_uri,
            _features: features,
            _feature_storage: feature_storage,
            _lib: lib,
            activated: false,
        };

        // activate() is optional.
        unsafe {
            if let Some(activate) = (*descriptor).activate {
                activate(handle);
            }
        }
        plugin.activated = true;
        Ok(plugin)
    }

    pub fn urid(&self) -> &UridMap {
        &self.urid
    }

    /// Connect a port to a memory location. [main-thread or audio-thread]
    pub fn connect_port(&self, port: u32, data: *mut c_void) {
        unsafe {
            if let Some(connect) = (*self.descriptor).connect_port {
                connect(self.handle, port, data);
            }
        }
    }

    /// Run the plugin for `n` frames. All ports must be connected first.
    pub fn run(&self, n: u32) {
        unsafe {
            if let Some(run) = (*self.descriptor).run {
                run(self.handle, n);
            }
        }
    }
}

impl Drop for Lv2Plugin {
    fn drop(&mut self) {
        unsafe {
            if self.activated {
                if let Some(deactivate) = (*self.descriptor).deactivate {
                    deactivate(self.handle);
                }
                self.activated = false;
            }
            if let Some(cleanup) = (*self.descriptor).cleanup {
                cleanup(self.handle);
            }
        }
        // _lib unloads here.
    }
}
