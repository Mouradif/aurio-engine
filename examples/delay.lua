-- The Delay effect — analog/tape-style, with the Doppler pitch-bend you get
-- when the delay time moves.
--
-- Time can be given as:
--   * a number      → milliseconds          (time = 375)
--   * a note string → tempo-synced duration (time = "4." = dotted quarter)
--   * a modulator   → swept time, in ms      (time = { name="sine", ... })
-- and as a single value (both channels) or a {left, right} pair.
--
-- The delay glides its read head toward the target time internally, so moving
-- the time — even a stepped change — resamples the buffered audio and bends its
-- pitch, exactly like riding the time knob on a delay pedal.
--
-- Octave convention: c3 = 72 (RTTTL-style).

song:bpm(120)

song:instrument(0, {
  adsr = { 0.005, 0.08, 0.0, 0.10 },  -- plucky, so echoes are distinct
  oscillators = {
    { wave = "Triangle", gain = 0.5 },
    { wave = "Sine",     gain = 0.3 },
  },
}, { label = "Pluck" })

song:effect(10, { kind = "Gain", label = "gain", params = { level = 0.5 } })

-- A tempo-synced ping-pong delay: left taps a dotted-eighth, right a quarter,
-- so echoes bounce L↔R in a syncopated pattern. feedback gives several repeats.
song:effect(11, {
  kind = "Delay",
  label = "pingpong",
  params = {
    time      = { "8.", "4" },   -- {left, right} as note durations
    feedback  = 0.45,
    mix       = 0.5,
    ping_pong = true,
  },
})

-- A second, deliberately Doppler-y delay: a slow sine sweeps the time between
-- 120 ms and 400 ms. As the read head chases the moving target, the echoes
-- warble up and down in pitch — the tape-delay sound.
--
-- `glide` eases the read head toward the target over a time constant (seconds).
-- It defaults to 0 (no glide, no Doppler) — but here the *modulator* already
-- moves the time smoothly, so you get the warble even at glide=0. A small
-- glide adds extra smoothing and makes stepped/CC time changes bend too.
song:effect(12, {
  kind = "Delay",
  label = "tape",
  params = {
    time     = { name = "sine", freq = 0.1, low = 120, high = 400 },
    feedback = 0.5,
    mix      = 0.45,
    glide    = 0.04,
  },
})

song:output(99, { label = "main" })

song:connection("Pluck",    "gain")
song:connection("gain",     "pingpong")
song:connection("pingpong", "tape")
song:connection("tape",     "main")

-- A sparse riff so the echoes have room to ring out.
song:phrase("Riff", {
  duration = 8.0,
  notes = {
    { pitch = "c3", start = 0.0, duration = 0.3 },
    { pitch = "g3", start = 1.0, duration = 0.3 },
    { pitch = "e3", start = 2.5, duration = 0.3 },
    { pitch = "a3", start = 4.0, duration = 0.3 },
    { pitch = "d3", start = 5.5, duration = 0.3 },
    { pitch = "g3", start = 6.5, duration = 0.3 },
  },
})

song:track({
  targets = { "Pluck" },
  initial = "Riff",
  transitions = { { from = "Riff", to = "Riff" } },
})
