-- Minimal MIDI-control demo.
--
-- Plays a steady drone through a distortion → gain chain. The distortion
-- mix and the final gain are both bound to MIDI CCs so you can sweep them
-- live from a knob/fader.
--
-- Default mapping (works with most generic controllers):
--   CC 53 → distortion mix     (0.0 dry  → 1.0 wet)
--   CC 52 → master gain        (silent  → 0.6)
--   CC 51 → delay time         (5 ms .. 800 ms) — ride it for the Doppler
--
-- Edit the cc numbers below to match your controller. The first available
-- MIDI input port is opened automatically on startup.

song:bpm(60)

song:instrument(0, {
  adsr = { 0.05, 0.1, 0.8, 0.3 },
  oscillators = {
    { wave = "Saw",      gain = 0.4 },
    { wave = "Triangle", gain = 0.3 },
  },
}, { label = "Drone" })

-- Distortion: drive is static, mix is live.
song:effect(10, "Distortion", {
  drive = 4.0,
  mix   = midi_connect("cc53"),
}, { label = "drive" })

-- Master gain bound to a CC, mapped 0.0 .. 0.6 instead of 0.0 .. 1.0
-- so the loudest position isn't unity.
song:effect(11, "Gain", {
  level = midi_connect({ cc = 52, range = { 0.0, 0.6 }, initial = 0.2 }),
}, { label = "master" })

-- Delay time bound to a CC. The raw 0..127 maps into the `range` (in ms).
-- `glide` (seconds) eases the read head toward each new knob value, so riding
-- the knob bends the echo pitch like a tape delay. Without glide the time
-- would just jump (no Doppler). `initial` is the starting time in ms.
song:effect(12, "Delay", {
  time = midi_connect({ cc = 51, range = { 5, 800 }, initial = 200 }),
  feedback = 0.5,
  mix = 0.5,
  glide = 0.08,
}, { label = "delay" })

song:output(99, { label = "main" })

song:connection("Drone",  "drive")
song:connection("drive",  "master")
song:connection("master", "delay")
song:connection("delay",  "main")

-- A long held note so you can hear the live param changes clearly.
song:phrase("Drone Phrase", {
  duration = 6.0,
  notes = {
    { pitch = 48, start = 0, duration = 1.5 },
    { pitch = 55, start = 0, duration = 1.5 },
    { pitch = 46, start = 1.5, duration = 1.5 },
    { pitch = 53, start = 1.5, duration = 1.5 },
    { pitch = 45, start = 3.0, duration = 1.5 },
    { pitch = 53, start = 3.0, duration = 1.5 },
    { pitch = 44, start = 4.5, duration = 1.5 },
    { pitch = 53, start = 4.5, duration = 1.5 },
  },
})

song:track({
  targets = { "Drone" },
  initial = "Drone Phrase",
  transitions = { { from = "Drone Phrase", to = "Drone Phrase" } },
})
