//! LV2 Turtle/RDF parsing: read a bundle's `manifest.ttl` and the plugin's
//! `.ttl` to discover the binary path and the full port layout.
//!
//! LV2's C ABI is index-based (`connect_port(instance, index, ptr)`) and tells
//! you nothing about what each port *is*. That information lives only in the
//! RDF, so this module is a hard requirement for hosting. We use `sophia` to
//! parse Turtle into a queryable graph and walk plugin→port→property links.

use std::path::{Path, PathBuf};

use sophia::api::ns::Namespace;
use sophia::api::prelude::*;
use sophia::api::term::SimpleTerm;
use sophia::inmem::graph::LightGraph;

/// What a port carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortKind {
    Audio,
    Control,
    /// An atom:Sequence port (events, e.g. MIDI in).
    AtomSequence,
    /// A port type we don't handle; still connected (to a dummy buffer) so the
    /// plugin doesn't crash, but otherwise ignored.
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortDirection {
    Input,
    Output,
}

/// One discovered port.
#[derive(Clone, Debug)]
pub struct PortDescriptor {
    pub index: u32,
    pub symbol: String,
    pub name: String,
    pub kind: PortKind,
    pub direction: PortDirection,
    /// Whether an atom port advertises MIDI support (`atom:supports midi`).
    pub supports_midi: bool,
    pub default: Option<f32>,
    pub min: Option<f32>,
    pub max: Option<f32>,
}

/// Everything needed to load and wire a plugin, read from the bundle's RDF.
#[derive(Clone, Debug)]
pub struct PluginManifest {
    /// Absolute path to the shared library binary.
    pub binary: PathBuf,
    /// Absolute path to the bundle directory (passed to `instantiate`, with a
    /// trailing separator per the LV2 spec).
    pub bundle_dir: PathBuf,
    pub ports: Vec<PortDescriptor>,
}

#[derive(Debug)]
pub enum TtlError {
    Io(String),
    Parse(String),
    NoManifest,
    PluginNotFound(String),
    NoBinary,
}

impl std::fmt::Display for TtlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TtlError::Io(e) => write!(f, "io error reading bundle: {e}"),
            TtlError::Parse(e) => write!(f, "turtle parse error: {e}"),
            TtlError::NoManifest => write!(f, "bundle has no manifest.ttl"),
            TtlError::PluginNotFound(uri) => write!(f, "plugin uri '{uri}' not in bundle"),
            TtlError::NoBinary => write!(f, "plugin declares no lv2:binary"),
        }
    }
}

impl std::error::Error for TtlError {}

// LV2 namespaces used in queries.
fn lv2() -> Namespace<&'static str> {
    Namespace::new("http://lv2plug.in/ns/lv2core#").unwrap()
}
fn atom() -> Namespace<&'static str> {
    Namespace::new("http://lv2plug.in/ns/ext/atom#").unwrap()
}
fn rdf() -> Namespace<&'static str> {
    Namespace::new("http://www.w3.org/1999/02/22-rdf-syntax-ns#").unwrap()
}

const MIDI_EVENT: &str = "http://lv2plug.in/ns/ext/midi#MidiEvent";

/// Parse `bundle_dir/manifest.ttl` plus the plugin's `seeAlso` TTL, returning
/// the binary path and port layout for the plugin identified by `plugin_uri`.
pub fn load(bundle_dir: &Path, plugin_uri: &str) -> Result<PluginManifest, TtlError> {
    let manifest_path = bundle_dir.join("manifest.ttl");
    if !manifest_path.is_file() {
        return Err(TtlError::NoManifest);
    }

    // LV2 manifests reference bundle files with *relative* IRIs (e.g.
    // `<ADLplug.so>`), which are only valid when resolved against a base IRI.
    // The base is the bundle directory as a `file://` URL (with trailing slash
    // so relative refs resolve into it).
    let base = bundle_dir_to_file_iri(bundle_dir);

    // Parse the manifest, then every `.ttl` it references via rdfs:seeAlso for
    // this plugin, into one combined graph. In practice a plugin's full data is
    // in one seeAlso file, but we also fold in the manifest itself.
    let mut graph: LightGraph = LightGraph::new();
    load_ttl_into(&manifest_path, &base, &mut graph)?;

    // Resolve the plugin subject term. SimpleTerm::Iri holds an
    // IriRef<MownStr>, so validate the string then wrap it as an owned MownStr.
    sophia::api::term::IriRef::new(plugin_uri)
        .map_err(|e| TtlError::Parse(format!("bad plugin uri: {e}")))?;
    let plugin = SimpleTerm::Iri(sophia::api::term::IriRef::new_unchecked(
        sophia::api::MownStr::from(plugin_uri.to_string()),
    ));

    // Find binary + seeAlso targets declared in the manifest. Namespaces are
    // bound to locals so the NsTerms they produce don't outlive a temporary.
    let lv2 = lv2();
    let rdfs = Namespace::new("http://www.w3.org/2000/01/rdf-schema#").unwrap();
    let binary_pred = lv2.get("binary").unwrap();
    let see_also = rdfs.get("seeAlso").unwrap();

    // Pull in referenced TTL files (the bulk of the port data lives here).
    let see_also_files: Vec<PathBuf> = graph
        .triples_matching([&plugin], [&see_also], Any)
        .filter_map(|t| t.ok())
        .filter_map(|t| term_iri_string(t.o()))
        .filter_map(|iri| iri_to_bundle_path(&iri, bundle_dir))
        .collect();
    for f in &see_also_files {
        if f.is_file() {
            load_ttl_into(f, &base, &mut graph)?;
        }
    }

    // binary may be declared in either manifest or the seeAlso file.
    let binary_iri = graph
        .triples_matching([&plugin], [&binary_pred], Any)
        .filter_map(|t| t.ok())
        .find_map(|t| term_iri_string(t.o()))
        .ok_or(TtlError::NoBinary)?;
    let binary = iri_to_bundle_path(&binary_iri, bundle_dir).ok_or(TtlError::NoBinary)?;

    // Confirm the plugin actually exists in the combined graph (has a type).
    let rdf_ns = rdf();
    let rdf_type = rdf_ns.get("type").unwrap();
    let exists = graph
        .triples_matching([&plugin], [&rdf_type], Any)
        .next()
        .is_some();
    if !exists {
        return Err(TtlError::PluginNotFound(plugin_uri.to_string()));
    }

    let ports = read_ports(&graph, &plugin)?;

    // The bundle path passed to instantiate() must end with a separator.
    let mut bundle_with_sep = bundle_dir.to_path_buf();
    bundle_with_sep.push(""); // ensures a trailing separator

    Ok(PluginManifest {
        binary,
        bundle_dir: bundle_with_sep,
        ports,
    })
}

/// Read and parse one `.ttl` file (resolving relative IRIs against `base`),
/// inserting its triples into `graph`.
fn load_ttl_into(path: &Path, base: &str, graph: &mut LightGraph) -> Result<(), TtlError> {
    use sophia::api::parser::TripleParser;
    use sophia::turtle::parser::turtle::TurtleParser;

    let text = std::fs::read_to_string(path).map_err(|e| TtlError::Io(e.to_string()))?;
    let base_iri = sophia::iri::resolve::BaseIriRef::new(base.to_string().into_boxed_str())
        .map_err(|e| TtlError::Parse(format!("bad base iri '{base}': {e}")))?;
    let parser = TurtleParser {
        base: Some(base_iri),
        ..Default::default()
    };
    let parsed: LightGraph = parser
        .parse_str(&text)
        .collect_triples()
        .map_err(|e| TtlError::Parse(e.to_string()))?;
    for t in parsed.triples() {
        let t = t.map_err(|e| TtlError::Parse(e.to_string()))?;
        graph
            .insert(t.s(), t.p(), t.o())
            .map_err(|e| TtlError::Parse(e.to_string()))?;
    }
    Ok(())
}

/// Walk `plugin lv2:port ?port` and read each port's properties.
fn read_ports(graph: &LightGraph, plugin: &SimpleTerm) -> Result<Vec<PortDescriptor>, TtlError> {
    let lv2 = lv2();
    let atom = atom();
    let rdf = rdf();

    let port_pred = lv2.get("port").unwrap();
    let index_pred = lv2.get("index").unwrap();
    let symbol_pred = lv2.get("symbol").unwrap();
    let name_pred = lv2.get("name").unwrap();
    let default_pred = lv2.get("default").unwrap();
    let min_pred = lv2.get("minimum").unwrap();
    let max_pred = lv2.get("maximum").unwrap();
    let type_pred = rdf.get("type").unwrap();
    let supports_pred = atom.get("supports").unwrap();

    const LV2: &str = "http://lv2plug.in/ns/lv2core#";
    const ATOM: &str = "http://lv2plug.in/ns/ext/atom#";

    // Collect the port nodes (blank nodes) linked from the plugin.
    let port_nodes: Vec<SimpleTerm> = graph
        .triples_matching([plugin], [&port_pred], Any)
        .filter_map(|t| t.ok())
        .map(|t| t.o().into_term())
        .collect();

    let mut ports = Vec::with_capacity(port_nodes.len());
    for port in &port_nodes {
        // Gather this port's rdf:type set as IRI strings.
        let types: Vec<String> = graph
            .triples_matching([port], [&type_pred], Any)
            .filter_map(|t| t.ok())
            .filter_map(|t| term_iri_string(t.o()))
            .collect();
        let has = |iri: String| -> bool { types.contains(&iri) };

        let kind = if has(format!("{LV2}AudioPort")) {
            PortKind::Audio
        } else if has(format!("{LV2}ControlPort")) {
            PortKind::Control
        } else if has(format!("{ATOM}AtomPort")) {
            PortKind::AtomSequence
        } else {
            PortKind::Other
        };

        let direction = if has(format!("{LV2}OutputPort")) {
            PortDirection::Output
        } else if has(format!("{LV2}InputPort")) {
            PortDirection::Input
        } else {
            // Default to input; harmless for our buffer wiring.
            PortDirection::Input
        };

        let index = graph
            .triples_matching([port], [&index_pred], Any)
            .filter_map(|t| t.ok())
            .find_map(|t| term_int(t.o()))
            .unwrap_or(u32::MAX);
        let symbol = graph
            .triples_matching([port], [&symbol_pred], Any)
            .filter_map(|t| t.ok())
            .find_map(|t| term_string(t.o()))
            .unwrap_or_default();
        let name = graph
            .triples_matching([port], [&name_pred], Any)
            .filter_map(|t| t.ok())
            .find_map(|t| term_string(t.o()))
            .unwrap_or_else(|| symbol.clone());

        let supports_midi = graph
            .triples_matching([port], [&supports_pred], Any)
            .filter_map(|t| t.ok())
            .any(|t| term_iri_string(t.o()).is_some_and(|s| s == MIDI_EVENT));

        let read_num = |pred| {
            graph
                .triples_matching([port], [pred], Any)
                .filter_map(|t| t.ok())
                .find_map(|t| term_f32(t.o()))
        };

        ports.push(PortDescriptor {
            index,
            symbol,
            name,
            kind,
            direction,
            supports_midi,
            default: read_num(&default_pred),
            min: read_num(&min_pred),
            max: read_num(&max_pred),
        });
    }

    // Drop any port we couldn't assign an index to, and sort by index so the
    // vector position is meaningful for debugging.
    ports.retain(|p| p.index != u32::MAX);
    ports.sort_by_key(|p| p.index);
    Ok(ports)
}

/// Build the base IRI for a bundle: its absolute path as a `file://` URL with a
/// trailing slash, so relative refs like `<ADLplug.so>` resolve into it.
fn bundle_dir_to_file_iri(bundle_dir: &Path) -> String {
    let abs = std::fs::canonicalize(bundle_dir).unwrap_or_else(|_| bundle_dir.to_path_buf());
    let mut s = abs.to_string_lossy().into_owned();
    if !s.ends_with('/') {
        s.push('/');
    }
    format!("file://{s}")
}

/// Resolve a (possibly relative) IRI from the RDF to a path inside the bundle.
/// After base resolution these are absolute `file://` IRIs; we also tolerate a
/// bare relative ref as a fallback.
fn iri_to_bundle_path(iri: &str, bundle_dir: &Path) -> Option<PathBuf> {
    let stripped = iri.strip_prefix("file://").unwrap_or(iri);
    let decoded = percent_decode(stripped);
    let p = Path::new(&decoded);
    if p.is_absolute() {
        Some(p.to_path_buf())
    } else {
        Some(bundle_dir.join(&decoded))
    }
}

/// Minimal percent-decoding for `file://` paths (handles `%20` etc.). Good
/// enough for filesystem paths; not a general-purpose URL decoder.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(b) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// --- term readers (generic over sophia's Term trait) --------------------

fn term_iri_string<T: Term>(t: T) -> Option<String> {
    if t.is_iri() {
        t.iri().map(|i| i.as_str().to_string())
    } else {
        None
    }
}

fn term_string<T: Term>(t: T) -> Option<String> {
    t.lexical_form().map(|s| s.to_string())
}

fn term_f32<T: Term>(t: T) -> Option<f32> {
    t.lexical_form().and_then(|s| s.trim().parse::<f32>().ok())
}

fn term_int<T: Term>(t: T) -> Option<u32> {
    t.lexical_form().and_then(|s| {
        s.trim()
            .parse::<u32>()
            .ok()
            .or_else(|| s.trim().parse::<f64>().ok().map(|f| f as u32))
    })
}
