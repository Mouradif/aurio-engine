//! Reader for EXS24 sampler instruments, the `.exs` files Logic and GarageBand
//! ship their sampled instruments in.
//!
//! # Format
//!
//! An EXS file is a flat list of fixed-size chunks. Each chunk opens with an
//! 84-byte header:
//!
//! ```text
//!   0..4    signature; the low nibble of byte 3 is the chunk type, and its
//!           0x80 bit marks a "JBOS" file
//!   4..8    body size in bytes, not counting this header; the 0x8000 bit is
//!           a flag rather than part of the length
//!   8..12   chunk id
//!  12..16   unused
//!  16..20   magic: "TBOS" little-endian, "JBOS" big-endian
//!  20..84   64-byte NUL-padded name
//! ```
//!
//! The chunk types that matter here are 0x01 (zone), 0x02 (group) and 0x03
//! (sample). A zone maps a key and velocity range onto a frame range of one
//! sample; a sample names a file, either alongside the instrument or inside a
//! consolidated multi-gigabyte blob shared by every zone.
//!
//! Note the two ways libraries reference audio, both of which appear in
//! Apple's factory content: one file per recording (a directory of AIFFs), or
//! a single consolidated CAF that every zone indexes into by frame offset.

use std::path::{Path, PathBuf};

/// One playable region of an EXS instrument.
pub struct ExsZone {
    /// Index into [`ExsInstrument::samples`].
    pub sample: usize,
    pub root: u8,
    pub low: u8,
    pub high: u8,
    pub vel_low: u8,
    pub vel_high: u8,
    /// Frame offset of this zone's audio within its sample file.
    pub start: u64,
    /// Frame count, or 0 when the zone runs to the end of the file.
    pub length: u64,
    /// Cents of detune, from the zone's fine-tune byte.
    pub fine_tune: i8,
    /// Per-zone volume in dB, as stored.
    pub volume: i8,
    pub pitch: bool,
}

/// One audio file an instrument's zones draw from.
pub struct ExsSample {
    pub name: String,
    pub file_name: String,
    /// Directory recorded in the file, used as one candidate when resolving.
    pub dir: String,
    pub frames: u64,
    pub rate: u32,
    pub bits: u16,
}

pub struct ExsInstrument {
    pub name: String,
    pub zones: Vec<ExsZone>,
    pub samples: Vec<ExsSample>,
}

const HEADER: usize = 84;

/// Read a NUL-terminated string out of a fixed-width field.
fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

fn u32_at(b: &[u8], off: usize, be: bool) -> u32 {
    if off + 4 > b.len() {
        return 0;
    }
    let a: [u8; 4] = b[off..off + 4].try_into().unwrap();
    if be {
        u32::from_be_bytes(a)
    } else {
        u32::from_le_bytes(a)
    }
}

impl ExsInstrument {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let path = path.as_ref();
        let data = std::fs::read(path)
            .map_err(|e| format!("failed to read '{}': {}", path.display(), e))?;
        Self::parse(&data, path)
    }

    fn parse(data: &[u8], path: &Path) -> Result<Self, String> {
        if data.len() < HEADER {
            return Err(format!("'{}' is too short to be an EXS file", path.display()));
        }
        // The magic at 16 confirms we are looking at an EXS instrument.
        // "JBOS" files set 0x80 in the signature's flag byte, but that only
        // affects fields we don't read: chunk sizes and the body fields below
        // are little-endian in both flavours.
        let magic = &data[16..20];
        if magic != b"TBOS" && magic != b"JBOS" && magic != b"SOBT" && magic != b"SOBJ" {
            return Err(format!(
                "'{}' is not an EXS instrument (bad magic {:?})",
                path.display(),
                String::from_utf8_lossy(magic)
            ));
        }

        let mut name = String::new();
        let mut zones = Vec::new();
        let mut samples = Vec::new();

        let mut off = 0usize;
        while off + HEADER <= data.len() {
            // The size field's 0x8000 bit is a flag, not part of the length.
            let raw = u32::from_le_bytes(data[off + 4..off + 8].try_into().unwrap());
            let size = if raw & 0x8000 != 0 {
                (raw & 0x7FFF) as usize
            } else {
                raw as usize
            };
            let chunk_type = data[off + 3] & 0x0F;
            let chunk_name = cstr(&data[off + 20..off + HEADER]);
            let body_start = off + HEADER;
            let body_end = (body_start + size).min(data.len());
            let body = &data[body_start..body_end];

            match chunk_type {
                0x00 => {
                    if name.is_empty() {
                        name = chunk_name;
                    }
                }
                0x01 => {
                    if let Some(z) = parse_zone(body) {
                        zones.push(z);
                    }
                }
                0x03 => samples.push(parse_sample(body, chunk_name)),
                _ => {}
            }

            let step = size + HEADER;
            if step == 0 {
                break;
            }
            off += step;
        }

        if zones.is_empty() {
            return Err(format!("'{}' contains no zones", path.display()));
        }
        if samples.is_empty() {
            return Err(format!("'{}' contains no samples", path.display()));
        }
        // Drop zones pointing past the sample table rather than failing the
        // whole instrument; stale indices show up in real factory content.
        let n = samples.len();
        zones.retain(|z| z.sample < n);
        if zones.is_empty() {
            return Err(format!(
                "'{}' has no zones referencing a valid sample",
                path.display()
            ));
        }

        if name.is_empty() {
            name = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
        }

        Ok(Self {
            name,
            zones,
            samples,
        })
    }
}

/// Zone body layout, all little-endian regardless of the file's flag byte
/// (the flag governs the chunk header, not these fields):
///
/// ```text
///   0   flags: 0x08 enables pitch tracking
///   1   root key            4  fine tune (signed cents)
///   5   volume              6  key range low
///   7   key range high      9  velocity low
///  10   velocity high      12  start frame (u32)
///  16   end frame (u32)    20  loop start (u32)
///  84   group index (i32)  92  sample index (i32)
/// ```
fn parse_zone(b: &[u8]) -> Option<ExsZone> {
    if b.len() < 96 {
        return None;
    }
    let start = u32_at(b, 12, false) as u64;
    let end = u32_at(b, 16, false) as u64;
    let sample = i32::from_le_bytes(b[92..96].try_into().unwrap());
    if sample < 0 {
        return None;
    }

    Some(ExsZone {
        sample: sample as usize,
        root: b[1] & 0x7F,
        low: b[6] & 0x7F,
        high: b[7] & 0x7F,
        vel_low: b[9] & 0x7F,
        vel_high: b[10] & 0x7F,
        start,
        // `end` is inclusive of the final frame in Logic's own writer.
        length: end.saturating_sub(start),
        fine_tune: b[4] as i8,
        volume: b[5] as i8,
        // Bit 3 is the pitch-tracking flag. Cleared, the zone plays at its
        // recorded pitch whatever key triggers it, which is how drum and
        // percussion maps are stored.
        pitch: b[0] & 0x08 != 0,
    })
}

/// Sample body layout: frame count and format up front, then two fixed-width
/// string fields, the containing directory and the file name.
fn parse_sample(b: &[u8], name: String) -> ExsSample {
    let frames = u32_at(b, 4, false) as u64;
    let rate = u32_at(b, 8, false);
    let bits = u32_at(b, 12, false) as u16;
    let dir = if b.len() > 80 {
        cstr(&b[80..b.len().min(80 + 256)])
    } else {
        String::new()
    };
    let file_name = if b.len() > 336 {
        cstr(&b[336..b.len().min(336 + 256)])
    } else {
        String::new()
    };

    ExsSample {
        file_name: if file_name.is_empty() {
            name.clone()
        } else {
            file_name
        },
        name,
        dir,
        frames,
        rate: if rate == 0 { 44100 } else { rate },
        bits: if bits == 0 { 16 } else { bits },
    }
}

impl ExsSample {
    /// Find this sample's audio on disk.
    ///
    /// The path recorded in the file is where the library was authored, which
    /// is right for Apple's factory content and wrong for anything that has
    /// been moved or copied between machines. So the recorded directory is
    /// only the first candidate: the instrument's own directory and the usual
    /// neighbours are tried too, and finally a bounded search nearby.
    pub fn resolve(&self, exs_path: &Path, extra_roots: &[PathBuf]) -> Option<PathBuf> {
        let file = &self.file_name;
        if file.is_empty() {
            return None;
        }

        let mut candidates: Vec<PathBuf> = Vec::new();
        if !self.dir.is_empty() {
            candidates.push(Path::new(&self.dir).join(file));
            // Some of Apple's own libraries record a directory with trailing
            // whitespace ("…/African Kit "), which no longer matches the
            // directory on disk. Retry without it before falling back.
            let trimmed = self.dir.trim_end();
            if trimmed != self.dir {
                candidates.push(Path::new(trimmed).join(file));
            }
        }
        for root in extra_roots {
            candidates.push(root.join(file));
        }
        if let Some(parent) = exs_path.parent() {
            candidates.push(parent.join(file));
            // Logic's layout puts audio in a sibling of the instrument
            // directory, so check the conventional neighbours by name.
            for sib in ["Samples", "samples", "Audio", "audio"] {
                candidates.push(parent.join(sib).join(file));
            }
            if let Some(up) = parent.parent() {
                candidates.push(up.join(file));
                for sib in ["Samples", "samples", "Audio", "audio"] {
                    candidates.push(up.join(sib).join(file));
                }
            }
            // Logic keeps audio in a tree parallel to the instruments, under
            // "EXS Factory Samples" rather than "Sampler Instruments", with
            // the category subdirectory repeated. Apple's own libraries record
            // their build machine's paths, so this mapping is what actually
            // finds the factory content.
            candidates.extend(parallel_sample_dirs(parent).map(|d| d.join(file)));
        }

        for c in &candidates {
            if c.is_file() {
                return Some(c.clone());
            }
        }

        // Last resort: walk a couple of levels down from the recorded
        // directory and the instrument's own, looking for the file by name.
        let mut roots: Vec<PathBuf> = Vec::new();
        if !self.dir.is_empty() {
            roots.push(PathBuf::from(&self.dir));
        }
        if let Some(parent) = exs_path.parent() {
            roots.push(parent.to_path_buf());
            if let Some(up) = parent.parent() {
                roots.push(up.to_path_buf());
            }
        }
        for root in roots {
            if let Some(hit) = find_nearby(&root, file, 2) {
                return Some(hit);
            }
        }
        None
    }
}

/// Candidate audio directories in the tree parallel to an instrument's own.
///
/// Given `.../Sampler Instruments/Japanese Traditional`, this yields
/// `.../EXS Factory Samples/Japanese Traditional` and the tree root, so a
/// library keeps working wherever the collection as a whole was installed.
fn parallel_sample_dirs(instrument_dir: &Path) -> impl Iterator<Item = PathBuf> + use<> {
    const AUDIO_DIRS: [&str; 2] = ["EXS Factory Samples", "Sampler Files"];
    let mut out = Vec::new();
    // Walk up looking for the "Sampler Instruments" level, keeping the
    // category path below it so it can be reattached on the audio side.
    let mut suffix: Vec<&std::ffi::OsStr> = Vec::new();
    let mut cur = Some(instrument_dir);
    while let Some(dir) = cur {
        if dir.file_name().map(|n| n == "Sampler Instruments").unwrap_or(false)
            && let Some(base) = dir.parent()
        {
            for audio in AUDIO_DIRS {
                let mut p = base.join(audio);
                for part in suffix.iter().rev() {
                    p.push(part);
                }
                out.push(p);
                out.push(base.join(audio));
            }
            break;
        }
        if let Some(n) = dir.file_name() {
            suffix.push(n);
        }
        cur = dir.parent();
    }
    out.into_iter()
}

/// Search `dir` for a file named `target`, descending at most `depth` levels.
fn find_nearby(dir: &Path, target: &str, depth: usize) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut subdirs = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_file() => {
                if p.file_name().map(|n| n == target).unwrap_or(false) {
                    return Some(p);
                }
            }
            Ok(t) if t.is_dir() => subdirs.push(p),
            _ => {}
        }
    }
    if depth == 0 {
        return None;
    }
    for sub in subdirs {
        if let Some(hit) = find_nearby(&sub, target, depth - 1) {
            return Some(hit);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an EXS chunk: an 84-byte header (signature with the type in the
    /// low nibble of byte 3, size, id, magic, name) followed by the body.
    fn chunk(ctype: u8, name: &str, body: &[u8]) -> Vec<u8> {
        let mut c = vec![0u8; HEADER];
        c[0] = 0x01;
        c[1] = 0x01;
        c[3] = ctype;
        c[4..8].copy_from_slice(&(body.len() as u32).to_le_bytes());
        c[16..20].copy_from_slice(b"TBOS");
        let n = name.as_bytes();
        let len = n.len().min(63);
        c[20..20 + len].copy_from_slice(&n[..len]);
        c.extend_from_slice(body);
        c
    }

    /// `keys` is the (root, low, high) mapping and `vel` the velocity range.
    fn zone_body(keys: (u8, u8, u8), vel: (u8, u8), start: u32, end: u32, s: i32) -> Vec<u8> {
        let (root, low, high) = keys;
        let (vlo, vhi) = vel;
        let mut b = vec![0u8; 96];
        b[0] = 0x08; // pitch tracking on
        b[1] = root;
        b[6] = low;
        b[7] = high;
        b[9] = vlo;
        b[10] = vhi;
        b[12..16].copy_from_slice(&start.to_le_bytes());
        b[16..20].copy_from_slice(&end.to_le_bytes());
        b[92..96].copy_from_slice(&s.to_le_bytes());
        b
    }

    fn sample_body(dir: &str, file: &str, frames: u32) -> Vec<u8> {
        let mut b = vec![0u8; 592];
        b[4..8].copy_from_slice(&frames.to_le_bytes());
        b[8..12].copy_from_slice(&44100u32.to_le_bytes());
        b[12..16].copy_from_slice(&16u32.to_le_bytes());
        b[80..80 + dir.len()].copy_from_slice(dir.as_bytes());
        b[336..336 + file.len()].copy_from_slice(file.as_bytes());
        b
    }

    fn instrument(zones: Vec<Vec<u8>>, samples: Vec<Vec<u8>>) -> Vec<u8> {
        let mut d = chunk(0x00, "Test Inst", &[0u8; 80]);
        for z in zones {
            d.extend(chunk(0x01, "Zone", &z));
        }
        for s in samples {
            d.extend(chunk(0x03, "Sample", &s));
        }
        d
    }

    #[test]
    fn parses_zones_and_samples() {
        let d = instrument(
            vec![zone_body((60, 48, 72), (0, 90), 0, 1000, 0)],
            vec![sample_body("/audio", "kick.aif", 1000)],
        );
        let inst = ExsInstrument::parse(&d, Path::new("x.exs")).unwrap();
        assert_eq!(inst.zones.len(), 1);
        assert_eq!(inst.samples.len(), 1);

        let z = &inst.zones[0];
        assert_eq!((z.root, z.low, z.high), (60, 48, 72));
        assert_eq!((z.vel_low, z.vel_high), (0, 90));
        assert_eq!((z.start, z.length), (0, 1000));
        assert!(z.pitch);

        let s = &inst.samples[0];
        assert_eq!(s.file_name, "kick.aif");
        assert_eq!(s.dir, "/audio");
        assert_eq!((s.frames, s.rate, s.bits), (1000, 44100, 16));
    }

    /// Velocity layers: one key, several recordings chosen by how hard it is
    /// struck. The ranges must survive parsing intact.
    #[test]
    fn keeps_velocity_layers_distinct() {
        let d = instrument(
            vec![
                zone_body((60, 60, 60), (0, 63), 0, 100, 0),
                zone_body((60, 60, 60), (64, 127), 200, 300, 1),
            ],
            vec![
                sample_body("/a", "soft.aif", 100),
                sample_body("/a", "hard.aif", 100),
            ],
        );
        let inst = ExsInstrument::parse(&d, Path::new("x.exs")).unwrap();
        assert_eq!(inst.zones.len(), 2);
        assert_eq!((inst.zones[0].vel_low, inst.zones[0].vel_high), (0, 63));
        assert_eq!((inst.zones[1].vel_low, inst.zones[1].vel_high), (64, 127));
        // The second zone reads a later slice of its file.
        assert_eq!(inst.zones[1].start, 200);
        assert_eq!(inst.zones[1].length, 100);
    }

    /// Zone bit 3 clear means the recording plays at its own pitch whatever
    /// key triggers it, as drum maps are stored.
    #[test]
    fn reads_pitch_tracking_flag() {
        let mut off = zone_body((60, 0, 127), (0, 127), 0, 10, 0);
        off[0] = 0x00;
        let d = instrument(vec![off], vec![sample_body("/a", "s.aif", 10)]);
        let inst = ExsInstrument::parse(&d, Path::new("x.exs")).unwrap();
        assert!(!inst.zones[0].pitch);
    }

    /// A zone naming a sample that isn't in the table is dropped rather than
    /// failing the instrument; stale indices occur in real libraries.
    #[test]
    fn drops_zones_with_dangling_sample_index() {
        let d = instrument(
            vec![
                zone_body((60, 0, 127), (0, 127), 0, 10, 0),
                zone_body((62, 0, 127), (0, 127), 0, 10, 9),
            ],
            vec![sample_body("/a", "s.aif", 10)],
        );
        let inst = ExsInstrument::parse(&d, Path::new("x.exs")).unwrap();
        assert_eq!(inst.zones.len(), 1);
        assert_eq!(inst.zones[0].root, 60);
    }

    #[test]
    fn rejects_non_exs_data() {
        let mut d = instrument(vec![], vec![]);
        d[16..20].copy_from_slice(b"XXXX");
        assert!(ExsInstrument::parse(&d, Path::new("x.exs")).is_err());
        assert!(ExsInstrument::parse(&[0u8; 4], Path::new("x.exs")).is_err());
    }

    #[test]
    fn resolves_sample_beside_the_instrument() {
        let dir = std::env::temp_dir().join(format!("aurio_exs_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let audio = dir.join("found.aif");
        std::fs::write(&audio, b"stub").unwrap();

        // The recorded directory does not exist, so resolution has to fall
        // back to the instrument's own directory.
        let s = ExsSample {
            name: "s".into(),
            file_name: "found.aif".into(),
            dir: "/nonexistent/authoring/path".into(),
            frames: 1,
            rate: 44100,
            bits: 16,
        };
        let got = s.resolve(&dir.join("inst.exs"), &[]);
        assert_eq!(got.as_deref(), Some(audio.as_path()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_reports_missing_audio() {
        let s = ExsSample {
            name: "s".into(),
            file_name: "definitely_absent_xyz.aif".into(),
            dir: "/nonexistent".into(),
            frames: 1,
            rate: 44100,
            bits: 16,
        };
        assert!(s.resolve(Path::new("/nonexistent/inst.exs"), &[]).is_none());
    }
}
