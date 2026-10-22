use crate::frame::Frame;
use crate::instrument::Instrument;
use crate::midi::NoteEvent;
use crate::param::Param;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::{HeapCons, HeapRb};

/// Ring-buffer capacity as a multiple of the device buffer size, used when Lua
/// doesn't set `buffer` explicitly.
///
/// The ring buffer absorbs jitter between the input and output callbacks, which
/// are scheduled independently. Its useful size is therefore a function of the
/// callback period, not an absolute frame count: four periods covers a late
/// callback on either side while keeping the standing backlog small. Whatever
/// backlog it holds is added directly to the latency you feel while playing, so
/// `render_block` actively trims it (see `max_backlog`).
pub const BUFFER_PERIODS: usize = 4;

/// Floor for the ring buffer, for the case of a very small device buffer where
/// `BUFFER_PERIODS` periods would leave no slack at all.
pub const MIN_BUFFER_FRAMES: usize = 256;

/// Frames that must be dropped before the overrun warning prints. A brief fill
/// while the two streams settle at startup is normal; this keeps the console
/// quiet unless the mismatch is real and sustained.
const OVERRUN_REPORT_FRAMES: u64 = 4800; // ~100 ms at 48 kHz

/// Stale frames that must be skipped before the backlog-trim notice prints.
const STALE_REPORT_FRAMES: u64 = 48_000; // ~1 s at 48 kHz

/// How a live audio input was declared in Lua. Resolved into a `LiveInput`
/// (and its cpal input stream) when the audio graph is built.
pub struct LiveInputSpec {
    /// Substring match against the device name; `None` uses the default input.
    pub device: Option<String>,
    /// 1-based input channel indices. One entry = mono (centered), two =
    /// stereo L/R. `None` takes the first one or two channels the device has.
    pub channels: Option<Vec<u16>>,
    /// Ring-buffer capacity in frames. `None` derives it from the device buffer
    /// size (`BUFFER_PERIODS` periods), which is almost always what you want —
    /// set it only to work around sustained overruns.
    ///
    /// The gain lives outside this spec: it's a `Param` built by the caller,
    /// since it can be modulated or MIDI-bound.
    pub buffer_frames: Option<usize>,
    /// Device buffer size (callback period) in frames, matching the output
    /// stream so both run on the same period.
    pub device_buffer: u32,
}

impl Default for LiveInputSpec {
    fn default() -> Self {
        Self {
            device: None,
            channels: None,
            buffer_frames: None,
            device_buffer: 128,
        }
    }
}

/// A graph source fed by a live audio input device rather than by note events.
///
/// The cpal *input* callback runs on its own thread and pushes frames into an
/// SPSC ring buffer; `render_block` (on the audio thread) pops them. The two
/// streams are not sample-locked, so the buffer absorbs their drift: when it
/// runs dry we emit silence rather than block, and when it overfills the
/// producer drops frames. Both are logged once so a persistent mismatch is
/// visible without spamming the console.
pub struct LiveInput {
    id: usize,
    consumer: HeapCons<Frame>,
    /// Input trim. Smoothed, so a live MIDI fader doesn't click.
    gain: Param,
    /// The input stream, kept alive for as long as this node exists. cpal stops
    /// the stream on drop, so this must not be dropped early.
    _stream: cpal::Stream,
    /// Backlog (in frames, beyond the current block) tolerated before stale
    /// input is discarded to hold latency down. Derived from `buffer_frames`.
    max_backlog: usize,
    /// Running count of stale frames skipped, and whether we've said so.
    dropped_stale: u64,
    warned_backlog: bool,
    /// Set once the buffer has been observed empty, to keep the warning to one
    /// line instead of one per callback.
    warned_underrun: bool,
}

impl LiveInput {
    /// Open the input device described by `spec` and start capturing.
    ///
    /// `sample_rate` is the *output* stream's rate; we ask the input device for
    /// the same rate so no resampling is needed. If the device can't provide
    /// it, that's an error — resampling live input is out of scope here.
    ///
    /// `gain` is built by the caller (from Lua) so this module stays free of
    /// the scripting layer; it may be a constant or a modulator.
    pub fn open(
        id: usize,
        spec: &LiveInputSpec,
        gain: Param,
        sample_rate: f32,
    ) -> Result<Self, String> {
        let host = cpal::default_host();

        let device = match &spec.device {
            Some(pattern) => {
                let needle = pattern.to_lowercase();
                let mut found = None;
                let devices = host
                    .input_devices()
                    .map_err(|e| format!("cannot enumerate input devices: {e}"))?;
                for d in devices {
                    if let Ok(desc) = d.description()
                        && desc.name().to_lowercase().contains(&needle)
                    {
                        found = Some(d);
                        break;
                    }
                }
                found.ok_or_else(|| {
                    format!(
                        "no input device matching '{}' (available: {})",
                        pattern,
                        available_input_names(&host).join(", ")
                    )
                })?
            }
            None => host
                .default_input_device()
                .ok_or_else(|| "no default input device".to_string())?,
        };

        let device_name = device
            .description()
            .map(|d| d.name().to_string())
            .unwrap_or_else(|_| "?".to_string());
        let default_config = device
            .default_input_config()
            .map_err(|e| format!("input device '{device_name}' has no default config: {e}"))?;
        let device_channels = default_config.channels();

        // Resolve which channels to read. Lua uses 1-based indices to match the
        // labels printed on an interface's front panel.
        let picks: Vec<u16> = match &spec.channels {
            Some(list) => {
                if list.is_empty() || list.len() > 2 {
                    return Err(format!(
                        "live input 'channels' must name 1 (mono) or 2 (stereo) channels, got {}",
                        list.len()
                    ));
                }
                for &ch in list {
                    if ch == 0 || ch > device_channels {
                        return Err(format!(
                            "live input channel {} is out of range for '{}' ({} input channel(s), 1-based)",
                            ch, device_name, device_channels
                        ));
                    }
                }
                list.iter().map(|c| c - 1).collect()
            }
            // Default: stereo if the device offers it, otherwise mono.
            None => {
                if device_channels >= 2 {
                    vec![0, 1]
                } else {
                    vec![0]
                }
            }
        };

        // Clamp the requested period to what this input device supports. When
        // input and output are the same physical device (the common jamming
        // case), CoreAudio treats buffer size as a device-level property, so
        // both streams end up on the same period anyway.
        let device_buffer = match default_config.buffer_size() {
            cpal::SupportedBufferSize::Range { min, max } => {
                let clamped = spec.device_buffer.clamp(*min, *max);
                if clamped != spec.device_buffer {
                    eprintln!(
                        "[live] input buffer size {} out of range {}..={} on '{}', using {}",
                        spec.device_buffer, min, max, device_name, clamped
                    );
                }
                clamped
            }
            cpal::SupportedBufferSize::Unknown => spec.device_buffer,
        };

        // Match the output rate so input and output share a timebase, and the
        // output's period so neither side is systematically ahead.
        let config = cpal::StreamConfig {
            channels: device_channels,
            sample_rate: sample_rate as u32,
            buffer_size: cpal::BufferSize::Fixed(device_buffer),
        };

        // Size the ring buffer from the callback period unless Lua overrode it.
        let buffer_frames = spec
            .buffer_frames
            .unwrap_or((device_buffer as usize * BUFFER_PERIODS).max(MIN_BUFFER_FRAMES));

        let rb = HeapRb::<Frame>::new(buffer_frames);
        let (mut producer, consumer) = rb.split();

        let stereo = picks.len() == 2;
        let (left_ch, right_ch) = (picks[0], *picks.last().unwrap());
        let n_dev = device_channels as usize;
        let mut warned_overrun = false;
        let mut dropped: u64 = 0;

        let stream = device
            .build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    // De-interleave the picked channels into stereo frames. A
                    // single picked channel lands equally in both (mono →
                    // center) so a `Pan` effect downstream still has something
                    // to work with. Gain is *not* applied here — it's a live
                    // param owned by the node, applied in `render_block`.
                    for frame in data.chunks_exact(n_dev) {
                        let f = if stereo {
                            Frame::new(frame[left_ch as usize], frame[right_ch as usize])
                        } else {
                            Frame::mono(frame[left_ch as usize])
                        };
                        // A full buffer means the consumer isn't keeping up.
                        // We drop the frame here and let `render_block` trim
                        // the standing backlog from the read side (the producer
                        // half of an SPSC ring can't pop).
                        if producer.try_push(f).is_err() {
                            dropped = dropped.saturating_add(1);
                            if dropped > OVERRUN_REPORT_FRAMES && !warned_overrun {
                                warned_overrun = true;
                                eprintln!(
                                    "[live] input overrunning ({dropped} frames dropped) — \
                                     raise `buffer` above {buffer_frames}"
                                );
                            }
                        }
                    }
                },
                |err| eprintln!("[live] input stream error: {err}"),
                None,
            )
            .map_err(|e| format!("cannot open input stream on '{device_name}': {e}"))?;

        stream
            .play()
            .map_err(|e| format!("cannot start input stream on '{device_name}': {e}"))?;

        // One period of standing backlog is enough to ride out normal jitter;
        // beyond that it's pure added delay, so `render_block` trims it.
        let max_backlog = device_buffer as usize;

        let channel_label: Vec<String> = picks.iter().map(|c| (c + 1).to_string()).collect();
        println!(
            "[live] socket {}: '{}' ch {} @ {} Hz, {} frame period, \
             ~{:.1} ms input latency ({} frame ring)",
            id,
            device_name,
            channel_label.join("+"),
            sample_rate,
            device_buffer,
            (device_buffer as usize + max_backlog) as f32 * 1000.0 / sample_rate,
            buffer_frames
        );

        Ok(Self {
            id,
            consumer,
            gain,
            _stream: stream,
            max_backlog,
            dropped_stale: 0,
            warned_backlog: false,
            warned_underrun: false,
        })
    }

    /// Input trim, bindable from Lua via `midi_connect`.
    pub const PARAM_GAIN: u8 = 0;

    /// Frames currently waiting in the ring buffer. This is the latency the
    /// buffer is adding right now, in frames — useful for diagnostics.
    pub fn backlog(&self) -> usize {
        self.consumer.occupied_len()
    }
}

fn available_input_names(host: &cpal::Host) -> Vec<String> {
    match host.input_devices() {
        Ok(devices) => devices
            .filter_map(|d| d.description().ok().map(|desc| desc.name().to_string()))
            .collect(),
        Err(_) => Vec::new(),
    }
}

impl Instrument for LiveInput {
    fn id(&self) -> usize {
        self.id
    }

    /// Live input ignores note events entirely — the sound comes from the wire,
    /// not from the score. A track pointed at a live input is a declaration
    /// error, caught at load time in `project.rs`.
    fn process_event(&mut self, _event: &NoteEvent) {}

    /// Single-frame path, kept for trait completeness. The graph always calls
    /// `render_block`, which is where the backlog trimming lives; delegating
    /// keeps both paths on one latency policy.
    fn next_sample(&mut self, sample_rate: f32) -> Frame {
        let mut one = [Frame::SILENCE; 1];
        self.render_block(&mut one, sample_rate);
        one[0]
    }

    /// Pop a whole block at once, then apply the trim per frame so a moving
    /// gain stays smooth. Any shortfall is filled with silence: the input
    /// stream hasn't delivered yet (normal for the first few callbacks, and
    /// after an xrun).
    fn render_block(&mut self, out: &mut [Frame], sample_rate: f32) {
        // Whatever sits in the ring buffer *is* latency: it's signal the player
        // already produced that we haven't emitted yet. If a backlog builds up
        // (the input stream ran ahead, or a callback was late), discard the
        // stale excess so the delay we add stays bounded near `max_backlog`
        // instead of drifting toward the buffer's full depth. One glitch beats
        // a permanently laggy monitor path.
        let backlog = self.consumer.occupied_len();
        let target = self.max_backlog + out.len();
        if backlog > target {
            let excess = backlog - target;
            self.consumer.skip(excess);
            self.dropped_stale = self.dropped_stale.saturating_add(excess as u64);
            if self.dropped_stale > STALE_REPORT_FRAMES && !self.warned_backlog {
                self.warned_backlog = true;
                eprintln!(
                    "[live] socket {}: trimming input backlog to stay low-latency \
                     ({} frames skipped so far)",
                    self.id, self.dropped_stale
                );
            }
        }

        let popped = self.consumer.pop_slice(out);
        if popped < out.len() {
            out[popped..].fill(Frame::SILENCE);
            if !self.warned_underrun {
                self.warned_underrun = true;
                eprintln!(
                    "[live] socket {}: input underrun (filling with silence; this is normal at startup)",
                    self.id
                );
            }
        }
        for slot in out.iter_mut() {
            *slot = *slot * self.gain.tick(sample_rate);
        }
    }

    fn set_param(&mut self, idx: u8, value: f32) {
        if idx == Self::PARAM_GAIN {
            self.gain.set_target(value);
        }
    }
}
