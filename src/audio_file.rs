//! Decoding of the uncompressed audio formats a sampler library can point at:
//! WAV (via hound), AIFF/AIFC and CAF (hand-rolled, both big-endian).
//!
//! Everything here runs at load time, never in the audio callback.
//!
//! A consolidated library keeps every recording end-to-end in one large file
//! (Logic's EXS factory content ships 50 MB+ CAFs) and each zone names the
//! frame range it owns. So the entry point takes an optional frame range and
//! reads only that slice, instead of pulling the whole file in once per zone.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use crate::frame::Frame;

/// Decoded PCM: stereo frames plus the rate they were recorded at.
pub struct AudioData {
    pub data: Vec<Frame>,
    pub source_rate: f32,
}

/// Frame range to decode: `start` is a frame index, `len` a frame count.
/// `None` means the whole file.
pub type FrameRange = Option<(u64, u64)>;

/// Decode `path`, optionally only the frames in `range`. The format is chosen
/// by extension, falling back to sniffing the leading magic bytes so a file
/// with a wrong or missing extension still loads.
pub fn load<P: AsRef<Path>>(path: P, range: FrameRange) -> Result<AudioData, String> {
    let path = path.as_ref();
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "wav" | "wave" => load_wav(path, range),
        "aif" | "aiff" | "aifc" => load_aiff(path, range),
        "caf" => load_caf(path, range),
        _ => match &magic(path)? {
            b"RIFF" => load_wav(path, range),
            b"FORM" => load_aiff(path, range),
            b"caff" => load_caf(path, range),
            _ => Err(format!("unsupported audio format: '{}'", path.display())),
        },
    }
}

/// First four bytes of a file, for format sniffing.
fn magic(path: &Path) -> Result<[u8; 4], String> {
    let mut f = File::open(path).map_err(|e| format!("failed to open '{}': {}", path.display(), e))?;
    let mut m = [0u8; 4];
    f.read_exact(&mut m)
        .map_err(|e| format!("reading '{}': {}", path.display(), e))?;
    Ok(m)
}

/// Interleaved samples → stereo frames. Mono is widened to equal L/R, stereo
/// passes through, and anything wider is averaged down into both channels.
fn to_frames(samples: Vec<f32>, channels: usize) -> Vec<Frame> {
    match channels {
        0 | 1 => samples.into_iter().map(Frame::mono).collect(),
        2 => samples
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| Frame::new(c[0], c[1]))
            .collect(),
        n => samples
            .chunks_exact(n)
            .map(|c| Frame::mono(c.iter().sum::<f32>() / n as f32))
            .collect(),
    }
}

/// Clamp a requested frame range to what the file actually holds, returning
/// the byte offset to seek to and the number of frames to read.
fn clamp_range(range: FrameRange, total_frames: u64, bytes_per_frame: u64) -> (u64, u64) {
    match range {
        Some((start, len)) => {
            let start = start.min(total_frames);
            let len = len.min(total_frames - start);
            (start * bytes_per_frame, len)
        }
        None => (0, total_frames),
    }
}

fn load_wav(path: &Path, range: FrameRange) -> Result<AudioData, String> {
    let mut reader = hound::WavReader::open(path)
        .map_err(|e| format!("failed to open '{}': {}", path.display(), e))?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;

    // hound seeks by frame, so a range costs only the frames we ask for.
    let total = reader.len() as u64 / channels as u64;
    let (start, len) = match range {
        Some((s, l)) => {
            let s = s.min(total);
            (s, l.min(total - s))
        }
        None => (0, total),
    };
    if start > 0 {
        reader
            .seek(start as u32)
            .map_err(|e| format!("seeking '{}': {}", path.display(), e))?;
    }
    let wanted = (len as usize) * channels;

    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .take(wanted)
            .collect::<Result<_, _>>()
            .map_err(|e| format!("reading '{}': {}", path.display(), e))?,
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .take(wanted)
                .map(|s| s.map(|v| v as f32 / max))
                .collect::<Result<_, _>>()
                .map_err(|e| format!("reading '{}': {}", path.display(), e))?
        }
    };

    Ok(AudioData {
        data: to_frames(samples, channels),
        source_rate: spec.sample_rate as f32,
    })
}

/// Read `count` frames of big-endian integer PCM at `bits` depth, normalized
/// to -1.0..=1.0. Widths of 8, 16, 24 and 32 bits are handled.
fn read_be_pcm<R: Read>(
    r: &mut R,
    count: usize,
    bits: u16,
    what: &str,
) -> Result<Vec<f32>, String> {
    let width = (bits as usize).div_ceil(8).max(1);
    let mut raw = vec![0u8; count * width];
    // The last block of a file can be short; a partial read is not an error.
    let got = read_upto(r, &mut raw).map_err(|e| format!("reading {}: {}", what, e))?;
    raw.truncate(got - got % width);

    let scale = 1.0 / (1i64 << (width * 8 - 1)) as f32;
    Ok(raw
        .chunks_exact(width)
        .map(|c| {
            // Sign-extend the big-endian sample into an i32, then normalize.
            let mut v: i32 = if c[0] & 0x80 != 0 { -1 } else { 0 };
            for &b in c {
                v = (v << 8) | b as i32;
            }
            // 8-bit PCM in AIFF is signed, matching the wider widths.
            v as f32 * scale
        })
        .collect())
}

/// Read until the buffer is full or the source ends, returning bytes read.
fn read_upto<R: Read>(r: &mut R, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

/// Decode an 80-bit IEEE 754 extended float, the AIFF sample-rate encoding.
fn extended80(b: &[u8; 10]) -> f64 {
    let sign = if b[0] & 0x80 != 0 { -1.0 } else { 1.0 };
    let exp = (((b[0] & 0x7f) as i32) << 8) | b[1] as i32;
    let mut mantissa = 0u64;
    for &x in &b[2..10] {
        mantissa = (mantissa << 8) | x as u64;
    }
    if exp == 0 && mantissa == 0 {
        return 0.0;
    }
    // The explicit-integer-bit mantissa is scaled by 2^(exp-16383-63).
    sign * mantissa as f64 * 2f64.powi(exp - 16383 - 63)
}

fn load_aiff(path: &Path, range: FrameRange) -> Result<AudioData, String> {
    let mut f = BufReader::new(
        File::open(path).map_err(|e| format!("failed to open '{}': {}", path.display(), e))?,
    );
    let disp = path.display().to_string();

    let mut hdr = [0u8; 12];
    f.read_exact(&mut hdr)
        .map_err(|e| format!("reading '{}': {}", disp, e))?;
    if &hdr[0..4] != b"FORM" || (&hdr[8..12] != b"AIFF" && &hdr[8..12] != b"AIFC") {
        return Err(format!("'{}' is not an AIFF file", disp));
    }

    let mut channels = 0usize;
    let mut bits = 16u16;
    let mut rate = 44100f32;
    let mut frames_in_file = 0u64;
    let mut compression = *b"NONE";

    // Walk the chunk list until the sound data is reached; COMM always
    // precedes SSND in a well-formed file, so the format is known by then.
    loop {
        let mut ch = [0u8; 8];
        if read_upto(&mut f, &mut ch).map_err(|e| format!("reading '{}': {}", disp, e))? < 8 {
            return Err(format!("'{}' has no SSND chunk", disp));
        }
        let id = [ch[0], ch[1], ch[2], ch[3]];
        let size = u32::from_be_bytes([ch[4], ch[5], ch[6], ch[7]]) as u64;

        match &id {
            b"COMM" => {
                let mut c = vec![0u8; size as usize];
                f.read_exact(&mut c)
                    .map_err(|e| format!("reading '{}': {}", disp, e))?;
                if c.len() < 18 {
                    return Err(format!("'{}' has a short COMM chunk", disp));
                }
                channels = u16::from_be_bytes([c[0], c[1]]) as usize;
                frames_in_file = u32::from_be_bytes([c[2], c[3], c[4], c[5]]) as u64;
                bits = u16::from_be_bytes([c[6], c[7]]);
                let mut ext = [0u8; 10];
                ext.copy_from_slice(&c[8..18]);
                rate = extended80(&ext) as f32;
                // AIFC names a codec after the rate field; only uncompressed
                // flavours are supported here.
                if c.len() >= 22 {
                    compression = [c[18], c[19], c[20], c[21]];
                }
            }
            b"SSND" => {
                let mut pre = [0u8; 8];
                f.read_exact(&mut pre)
                    .map_err(|e| format!("reading '{}': {}", disp, e))?;
                let offset = u32::from_be_bytes([pre[0], pre[1], pre[2], pre[3]]) as u64;
                if channels == 0 {
                    return Err(format!("'{}' has SSND before COMM", disp));
                }
                if !matches!(&compression, b"NONE" | b"sowt" | b"twos" | b"in24" | b"in32") {
                    return Err(format!(
                        "'{}' uses unsupported AIFC compression '{}'",
                        disp,
                        String::from_utf8_lossy(&compression)
                    ));
                }
                let bpf = (bits as usize).div_ceil(8) as u64 * channels as u64;
                let (skip, len) = clamp_range(range, frames_in_file, bpf);
                f.seek(SeekFrom::Current((offset + skip) as i64))
                    .map_err(|e| format!("seeking '{}': {}", disp, e))?;
                let mut samples =
                    read_be_pcm(&mut f, len as usize * channels, bits, &disp)?;
                // 'sowt' is little-endian PCM in an AIFC wrapper: the decode
                // above read it big-endian, so swap each sample's bytes back.
                if &compression == b"sowt" {
                    samples = reread_as_le(path, offset + skip, len, channels, bits, &disp)?;
                }
                return Ok(AudioData {
                    data: to_frames(samples, channels),
                    source_rate: rate,
                });
            }
            _ => {
                // Chunks are word-aligned: an odd size carries a pad byte.
                let skip = size + (size & 1);
                f.seek(SeekFrom::Current(skip as i64))
                    .map_err(|e| format!("seeking '{}': {}", disp, e))?;
            }
        }
    }
}

/// Re-read a 'sowt' (little-endian) AIFC body. Rare enough that a second pass
/// is cheaper than threading endianness through the common path.
fn reread_as_le(
    path: &Path,
    data_start_from_ssnd: u64,
    frames: u64,
    channels: usize,
    bits: u16,
    disp: &str,
) -> Result<Vec<f32>, String> {
    let mut f = BufReader::new(File::open(path).map_err(|e| format!("reopening '{}': {}", disp, e))?);
    // Locate SSND again, then apply the same offset the caller used.
    let mut pos = 12u64;
    f.seek(SeekFrom::Start(12))
        .map_err(|e| format!("seeking '{}': {}", disp, e))?;
    loop {
        let mut ch = [0u8; 8];
        if read_upto(&mut f, &mut ch).map_err(|e| format!("reading '{}': {}", disp, e))? < 8 {
            return Err(format!("'{}' has no SSND chunk", disp));
        }
        let size = u32::from_be_bytes([ch[4], ch[5], ch[6], ch[7]]) as u64;
        pos += 8;
        if &ch[0..4] == b"SSND" {
            f.seek(SeekFrom::Start(pos + 8 + data_start_from_ssnd))
                .map_err(|e| format!("seeking '{}': {}", disp, e))?;
            break;
        }
        let skip = size + (size & 1);
        pos += skip;
        f.seek(SeekFrom::Start(pos))
            .map_err(|e| format!("seeking '{}': {}", disp, e))?;
    }

    let width = (bits as usize).div_ceil(8).max(1);
    let mut raw = vec![0u8; frames as usize * channels * width];
    let got = read_upto(&mut f, &mut raw).map_err(|e| format!("reading '{}': {}", disp, e))?;
    raw.truncate(got - got % width);
    let scale = 1.0 / (1i64 << (width * 8 - 1)) as f32;
    Ok(raw
        .chunks_exact(width)
        .map(|c| {
            let mut v: i32 = if c[width - 1] & 0x80 != 0 { -1 } else { 0 };
            for &b in c.iter().rev() {
                v = (v << 8) | b as i32;
            }
            v as f32 * scale
        })
        .collect())
}

fn load_caf(path: &Path, range: FrameRange) -> Result<AudioData, String> {
    let mut f = BufReader::new(
        File::open(path).map_err(|e| format!("failed to open '{}': {}", path.display(), e))?,
    );
    let disp = path.display().to_string();

    let mut hdr = [0u8; 8];
    f.read_exact(&mut hdr)
        .map_err(|e| format!("reading '{}': {}", disp, e))?;
    if &hdr[0..4] != b"caff" {
        return Err(format!("'{}' is not a CAF file", disp));
    }

    let mut rate = 44100f32;
    let mut channels = 0usize;
    let mut bits = 16u16;
    let mut fmt = *b"lpcm";
    let mut fmt_flags = 0u32;
    let mut bytes_per_packet = 0u32;

    loop {
        let mut ch = [0u8; 12];
        if read_upto(&mut f, &mut ch).map_err(|e| format!("reading '{}': {}", disp, e))? < 12 {
            return Err(format!("'{}' has no data chunk", disp));
        }
        let id = [ch[0], ch[1], ch[2], ch[3]];
        let size = i64::from_be_bytes([
            ch[4], ch[5], ch[6], ch[7], ch[8], ch[9], ch[10], ch[11],
        ]);

        match &id {
            b"desc" => {
                let mut c = [0u8; 32];
                f.read_exact(&mut c)
                    .map_err(|e| format!("reading '{}': {}", disp, e))?;
                rate = f64::from_be_bytes(c[0..8].try_into().unwrap()) as f32;
                fmt = [c[8], c[9], c[10], c[11]];
                fmt_flags = u32::from_be_bytes(c[12..16].try_into().unwrap());
                bytes_per_packet = u32::from_be_bytes(c[16..20].try_into().unwrap());
                channels = u32::from_be_bytes(c[24..28].try_into().unwrap()) as usize;
                bits = u32::from_be_bytes(c[28..32].try_into().unwrap()) as u16;
            }
            b"data" => {
                if &fmt != b"lpcm" {
                    return Err(format!(
                        "'{}' uses unsupported CAF format '{}'; only uncompressed lpcm is supported",
                        disp,
                        String::from_utf8_lossy(&fmt)
                    ));
                }
                if channels == 0 {
                    return Err(format!("'{}' has a data chunk before desc", disp));
                }
                let bpf = if bytes_per_packet > 0 {
                    bytes_per_packet as u64
                } else {
                    (bits as usize).div_ceil(8) as u64 * channels as u64
                };
                // PCM begins after the data chunk's 4-byte edit count.
                let origin = f
                    .stream_position()
                    .map_err(|e| format!("seeking '{}': {}", disp, e))?
                    + 4;
                // A size of -1 means "runs to end of file".
                let total_frames = if size < 0 {
                    let end = f
                        .seek(SeekFrom::End(0))
                        .map_err(|e| format!("seeking '{}': {}", disp, e))?;
                    end.saturating_sub(origin) / bpf
                } else {
                    (size as u64).saturating_sub(4) / bpf
                };

                let (skip, len) = clamp_range(range, total_frames, bpf);
                f.seek(SeekFrom::Start(origin + skip))
                    .map_err(|e| format!("seeking '{}': {}", disp, e))?;

                // CAF lpcm is big-endian unless the little-endian flag is set.
                let little_endian = fmt_flags & 2 != 0;
                let is_float = fmt_flags & 1 != 0;
                let samples = if is_float {
                    read_caf_float(&mut f, len as usize * channels, bits, little_endian, &disp)?
                } else if little_endian {
                    read_le_pcm(&mut f, len as usize * channels, bits, &disp)?
                } else {
                    read_be_pcm(&mut f, len as usize * channels, bits, &disp)?
                };

                return Ok(AudioData {
                    data: to_frames(samples, channels),
                    source_rate: rate,
                });
            }
            _ => {
                if size < 0 {
                    return Err(format!("'{}' has a malformed chunk size", disp));
                }
                f.seek(SeekFrom::Current(size))
                    .map_err(|e| format!("seeking '{}': {}", disp, e))?;
            }
        }
    }
}

fn read_le_pcm<R: Read>(r: &mut R, count: usize, bits: u16, what: &str) -> Result<Vec<f32>, String> {
    let width = (bits as usize).div_ceil(8).max(1);
    let mut raw = vec![0u8; count * width];
    let got = read_upto(r, &mut raw).map_err(|e| format!("reading {}: {}", what, e))?;
    raw.truncate(got - got % width);
    let scale = 1.0 / (1i64 << (width * 8 - 1)) as f32;
    Ok(raw
        .chunks_exact(width)
        .map(|c| {
            let mut v: i32 = if c[width - 1] & 0x80 != 0 { -1 } else { 0 };
            for &b in c.iter().rev() {
                v = (v << 8) | b as i32;
            }
            v as f32 * scale
        })
        .collect())
}

fn read_caf_float<R: Read>(
    r: &mut R,
    count: usize,
    bits: u16,
    little_endian: bool,
    what: &str,
) -> Result<Vec<f32>, String> {
    let width = (bits as usize / 8).max(4);
    let mut raw = vec![0u8; count * width];
    let got = read_upto(r, &mut raw).map_err(|e| format!("reading {}: {}", what, e))?;
    raw.truncate(got - got % width);
    Ok(raw
        .chunks_exact(width)
        .map(|c| {
            if width == 8 {
                let a: [u8; 8] = c.try_into().unwrap();
                (if little_endian {
                    f64::from_le_bytes(a)
                } else {
                    f64::from_be_bytes(a)
                }) as f32
            } else {
                let a: [u8; 4] = c[0..4].try_into().unwrap();
                if little_endian {
                    f32::from_le_bytes(a)
                } else {
                    f32::from_be_bytes(a)
                }
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("aurio_af_{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d.join(name)
    }

    /// A mono 16-bit AIFF whose samples ramp, so a decoded range can be
    /// checked against the frame index it should have come from.
    fn write_aiff(path: &Path, frames: u32) {
        let mut d = Vec::new();
        let body_len = 4 + 8 + 18 + 8 + 8 + frames as usize * 2;
        d.extend_from_slice(b"FORM");
        d.extend_from_slice(&(body_len as u32).to_be_bytes());
        d.extend_from_slice(b"AIFF");

        d.extend_from_slice(b"COMM");
        d.extend_from_slice(&18u32.to_be_bytes());
        d.extend_from_slice(&1u16.to_be_bytes()); // channels
        d.extend_from_slice(&frames.to_be_bytes());
        d.extend_from_slice(&16u16.to_be_bytes()); // bits
        // 44100 as an 80-bit extended float.
        d.extend_from_slice(&[0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0]);

        d.extend_from_slice(b"SSND");
        d.extend_from_slice(&(frames * 2 + 8).to_be_bytes());
        d.extend_from_slice(&0u32.to_be_bytes()); // offset
        d.extend_from_slice(&0u32.to_be_bytes()); // block size
        for i in 0..frames {
            d.extend_from_slice(&((i as i16).wrapping_mul(16)).to_be_bytes());
        }
        std::fs::write(path, d).unwrap();
    }

    /// A mono 16-bit CAF with the same ramp.
    fn write_caf(path: &Path, frames: u32) {
        let mut d = Vec::new();
        d.extend_from_slice(b"caff");
        d.extend_from_slice(&1u16.to_be_bytes());
        d.extend_from_slice(&0u16.to_be_bytes());

        d.extend_from_slice(b"desc");
        d.extend_from_slice(&32i64.to_be_bytes());
        d.extend_from_slice(&44100f64.to_be_bytes());
        d.extend_from_slice(b"lpcm");
        d.extend_from_slice(&0u32.to_be_bytes()); // flags: big-endian int
        d.extend_from_slice(&2u32.to_be_bytes()); // bytes per packet
        d.extend_from_slice(&1u32.to_be_bytes()); // frames per packet
        d.extend_from_slice(&1u32.to_be_bytes()); // channels
        d.extend_from_slice(&16u32.to_be_bytes()); // bits

        d.extend_from_slice(b"data");
        d.extend_from_slice(&((frames as i64) * 2 + 4).to_be_bytes());
        d.extend_from_slice(&0u32.to_be_bytes()); // edit count
        for i in 0..frames {
            d.extend_from_slice(&((i as i16).wrapping_mul(16)).to_be_bytes());
        }
        std::fs::write(path, d).unwrap();
    }

    #[test]
    fn decodes_aiff() {
        let p = tmp("t.aif");
        write_aiff(&p, 100);
        let d = load(&p, None).unwrap();
        assert_eq!(d.source_rate, 44100.0);
        assert_eq!(d.data.len(), 100);
        // Mono is widened to both channels.
        assert_eq!(d.data[10].l, d.data[10].r);
        assert!((d.data[10].l - (160.0 / 32768.0)).abs() < 1e-6);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn decodes_caf() {
        let p = tmp("t.caf");
        write_caf(&p, 100);
        let d = load(&p, None).unwrap();
        assert_eq!(d.source_rate, 44100.0);
        assert_eq!(d.data.len(), 100);
        assert!((d.data[10].l - (160.0 / 32768.0)).abs() < 1e-6);
        let _ = std::fs::remove_file(&p);
    }

    /// The consolidated-library case: a range must return exactly the frames
    /// at that offset, since zones index into one shared file.
    #[test]
    fn reads_only_the_requested_range() {
        for (name, writer) in [
            ("r.aif", write_aiff as fn(&Path, u32)),
            ("r.caf", write_caf as fn(&Path, u32)),
        ] {
            let p = tmp(name);
            writer(&p, 1000);
            let d = load(&p, Some((500, 10))).unwrap();
            assert_eq!(d.data.len(), 10, "{name}");
            // Frame 500 of the ramp, not frame 0.
            assert!(
                (d.data[0].l - (500i16.wrapping_mul(16) as f32 / 32768.0)).abs() < 1e-6,
                "{name} started at the wrong frame"
            );
            let _ = std::fs::remove_file(&p);
        }
    }

    /// A range running past the end is clamped rather than failing.
    #[test]
    fn clamps_a_range_past_the_end() {
        let p = tmp("c.caf");
        write_caf(&p, 100);
        let d = load(&p, Some((90, 500))).unwrap();
        assert_eq!(d.data.len(), 10);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn rejects_unknown_format() {
        let p = tmp("bogus.xyz");
        std::fs::write(&p, b"not audio at all").unwrap();
        assert!(load(&p, None).is_err());
        let _ = std::fs::remove_file(&p);
    }
}
