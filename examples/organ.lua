-- Hammond-style tonewheel organ demo — a slow gospel-ish progression in C.
--
-- The organ is a native instrument: declare it with `drawbars` instead of
-- `adsr`/`oscillators`. Registration is the classic 9-digit drawbar string
-- (16', 5-1/3', 8', 4', 2-2/3', 2', 1-3/5', 1-1/3', 1'), each digit 0-8 —
-- or a list of 9 values where any drawbar can be a modulator or a live
-- midi_connect(...) fader.
--
-- Routing: Organ → tremolo (poor man's Leslie) → reverb → main
--          Bass organ → main (dry, dark registration)

song:bpm(84)

-- Upper manual: the classic "888" jazz registration, but played on a worn
-- B3 — C3 chorus, third-harmonic percussion (which steals the 1' bar and
-- dips the volume, like the real circuit), tube drive, and some age.
song:instrument(0, {
  drawbars = {
    8, 8, 8, 0, 0, 0, 0, 0,
    -- The 1' bar slowly breathes in and out via an LFO (it's silenced
    -- anyway while percussion is on — swap harmonics to hear it).
    { name = "triangle", freq = 0.1, low = 0, high = 6 },
    -- Live alternatives, with a 9-fader MIDI controller:
    --   midi_connect({ cc = 70, initial = 8, range = { 0, 8 } }), ...
  },
  gain = 0.9,           -- swell; bind it live with midi_connect for a pedal
  click = 0.4,
  percussion = { harmonic = 3, decay = "fast", volume = "normal" },
  vibrato = "C3",       -- the six-position switch: V1-V3, C1-C3
  drive = 0.35,         -- preamp growl, responds to the swell gain
  leakage = 0.3,        -- tonewheel crosstalk whine
  wobble = 0.4,         -- per-wheel detune + slow generator flutter
}, { label = "Organ" })

-- Lower manual / pedals: dark 16'+8' only, clean and tight.
song:instrument(1, {
  drawbars = "80 8000 000",
  gain = 0.9,
  click = 0.2,
  vibrato = "V1",
}, { label = "BassOrgan" })

-- A tremolo standing in for the Leslie rotor, then a room.
song:effect(10, { kind = "Tremolo", label = "leslie", params = { rate = 5.7, depth = 0.35 } })
song:effect(11, { kind = "Reverb", label = "room", params = { delay = 0.3, feedback = 0.3, mix = 0.25 } })

song:output(99, { label = "main" })

song:connection("Organ", "leslie")
song:connection("leslie", "room")
song:connection("room", "main")
song:connection("BassOrgan", "main")

-- Chords: C - Am - F - G, one bar each, with gaps between chords so the
-- single-trigger percussion re-arms (it only fires when no key is held).
local chords = {
  { 0,  { 60, 64, 67 } },  -- C
  { 4,  { 57, 60, 64 } },  -- Am
  { 8,  { 57, 60, 65 } },  -- F
  { 12, { 59, 62, 67 } },  -- G
}
local organ_notes = {}
for _, c in ipairs(chords) do
  for _, p in ipairs(c[2]) do
    table.insert(organ_notes, { pitch = p, start = c[1], duration = 3.5 })
  end
end
song:phrase("Organ Chords", { duration = 16.0, notes = organ_notes })

-- Bass: roots in the low register, held.
song:phrase("Organ Bass", {
  duration = 16.0,
  notes = {
    { pitch = 48, start = 0,  duration = 3.8 },  -- C2
    { pitch = 33, start = 4,  duration = 3.8 },  -- A1
    { pitch = 29, start = 8,  duration = 3.8 },  -- F1
    { pitch = 31, start = 12, duration = 3.8 },  -- G1
  },
})

song:track({
  targets = { "Organ" },
  initial = "Organ Chords",
})

song:track({
  targets = { "BassOrgan" },
  initial = "Organ Bass",
})
