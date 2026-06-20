//! LV2 plugin hosting, hand-rolled on top of the raw ABI in [`sys`].
//!
//! Gated behind the `lv2-host` cargo feature. Unlike CLAP, an LV2 plugin isn't
//! fully self-describing through its C ABI: the port layout (counts, types,
//! directions, names, ranges) lives in the bundle's Turtle/RDF manifests, which
//! [`ttl`] parses (via `sophia`). The flow: parse `manifest.ttl` → find the
//! plugin's binary + port descriptors → `dlopen` → `lv2_descriptor` → supply the
//! `urid:map` feature → `instantiate` → `connect_port` each port to our buffers
//! → `activate` → drive `run()` per block from the graph node ([`Lv2Node`]).

pub mod sys;

mod host;
mod node;
mod ttl;

pub use node::{Lv2Error, Lv2Node, ParamInfo};
