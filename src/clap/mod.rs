//! CLAP plugin hosting, hand-rolled on top of the raw ABI in [`sys`].
//!
//! Gated behind the `clap-host` cargo feature so the default build stays free
//! of plugin-hosting machinery. The flow mirrors a minimal CLAP host:
//! load the bundle → init the entry → get the factory → create a plugin by id →
//! activate at the stream's sample rate and max block → start processing, then
//! drive `process()` per block from inside the graph node ([`PluginNode`]).

pub mod sys;

mod host;
mod node;

pub use node::{PluginError, PluginNode};
