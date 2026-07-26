-- Stereo, Pan, and modulated params.
--
-- Aurio processes audio in stereo end-to-end. This song shows the two new
-- building blocks:
--
--   * the `Pan` effect — amount -1 (full left) .. +1 (full right)
--   * modulated params — any effect/synth param can be a constant OR a
--     modulator table { name=..., freq=, low=, high= }, evaluated per sample
--     on the audio thread (no Lua called per frame). Shapes: sine, triangle,
--     ramp, square.
--
-- Two voices, hard-panned and oppositely auto-panning, with a tremolo-ish
-- gain wobble driven by a triangle LFO — so you get clear stereo movement.
--
-- Octave convention: c3 = 72 (RTTTL-style).

song:bpm(90)

-- Two simple synths.
song:instrument(0, {
  adsr = { 0.01, 0.15, 0.6, 0.30 },
  -- Oscillator gain itself can be modulated: this saw pulses with a 6 Hz
  -- triangle LFO between 0.15 and 0.4.
  oscillators = {
    { wave = "Saw",      gain = { name = "triangle", freq = 6, low = 0.15, high = 0.4 } },
    { wave = "Triangle", gain = 0.2 },
  },
}, { label = "VoiceA" })

song:instrument(1, {
  adsr = { 0.01, 0.15, 0.6, 0.30 },
  oscillators = {
    { wave = "Square",   gain = 0.18 },
    { wave = "Sine",     gain = 0.25 },
  },
}, { label = "VoiceB" })

-- Voice A sweeps left↔right with a sine LFO (full cycle every 2s).
song:effect(10, {
  kind = "Pan",
  label = "panA",
  params = {
    amount = { name = "sine", freq = 0.5, low = -1, high = 1 },
  },
})

-- Voice B uses a square LFO to jump hard left / hard right every beat-ish,
-- creating a ping-pong against Voice A's smooth sweep.
song:effect(11, {
  kind = "Pan",
  label = "panB",
  params = {
    amount = { name = "square", freq = 1.5, low = -0.9, high = 0.9 },
  },
})

song:effect(20, { kind = "Gain", label = "gainA", params = { level = 0.5 } })
song:effect(21, { kind = "Gain", label = "gainB", params = { level = 0.5 } })

-- Every effect param accepts a modulator now, including filter cutoff. This
-- low-pass on Voice A sweeps its cutoff from 300 Hz up to 3500 Hz and back
-- with a slow triangle LFO — the classic filter-sweep sound.
song:effect(30, {
  kind = "Lowpass",
  label = "sweepA",
  params = {
    cutoff = { name = "triangle", freq = 0.2, low = 300, high = 3500 },
  },
})

-- Reverb's delay is modulatable too. A *short* delay (1–6 ms) swept by a slow
-- sine, with high feedback and a 50/50 mix, is a flanger — the moving read-tap
-- creates the sweeping "jet plane" comb filtering. (delay is in seconds.)
song:effect(31, {
  kind = "Reverb",
  label = "flangeB",
  params = {
    delay    = { name = "sine", freq = 0.15, low = 0.001, high = 0.006 },
    feedback = 0.7,
    mix      = 0.5,
  },
})

song:output(99, { label = "main" })

song:connection("VoiceA", "gainA")
song:connection("gainA",  "sweepA")
song:connection("sweepA", "panA")
song:connection("panA",   "main")

song:connection("VoiceB",  "gainB")
song:connection("gainB",   "flangeB")
song:connection("flangeB", "panB")
song:connection("panB",    "main")

-- Two interlocking phrases a third apart.
song:phrase("A", {
  duration = 4.0,
  notes = {
    { pitch = "c3", start = 0.0, duration = 0.9 },
    { pitch = "e3", start = 1.0, duration = 0.9 },
    { pitch = "g3", start = 2.0, duration = 0.9 },
    { pitch = "e3", start = 3.0, duration = 0.9 },
  },
})

song:phrase("B", {
  duration = 4.0,
  notes = {
    { pitch = "g2", start = 0.5, duration = 0.9 },
    { pitch = "b2", start = 1.5, duration = 0.9 },
    { pitch = "d3", start = 2.5, duration = 0.9 },
    { pitch = "b2", start = 3.5, duration = 0.9 },
  },
})

song:track({ targets = { "VoiceA" }, initial = "A", transitions = { { from = "A", to = "A" } } })
song:track({ targets = { "VoiceB" }, initial = "B", transitions = { { from = "B", to = "B" } } })
