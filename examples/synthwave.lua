-- Synthwave — a 4-bar loop in A minor that exercises every effect.
--
-- Routing (each line = a separate signal chain into "main"):
--   Kick  → kick_gain                                  → main   (clean, just gain)
--   Snare → snare_crush → snare_verb → snare_gain      → main   (Bitcrusher + Reverb)
--   Hat   → hat_hp      → hat_gain                     → main   (Highpass)
--   Bass  → bass_drive  → bass_lp  → bass_gain         → main   (Distortion + Lowpass)
--   Pad   → pad_trem    → pad_verb → pad_gain          → main   (Tremolo + Reverb)
--   Lead  → lead_drive  → lead_verb → lead_gain        → main   (Distortion + Reverb)

song:bpm(110)

-- =========================================================================
-- Instruments
-- =========================================================================

-- Kick: low sine, fast decay, no sustain — punchy thump.
song:instrument(0, {
  adsr = { 0.0, 0.18, 0.0, 0.05 },
  oscillators = { { wave = "Sine", gain = 1.0 } },
}, { label = "Kick" })

-- Snare: noise body + a touch of triangle for tonal weight.
song:instrument(1, {
  adsr = { 0.0, 0.14, 0.0, 0.08 },
  oscillators = {
    { wave = "Noise",    gain = 0.55 },
    { wave = "Triangle", gain = 0.25 },
  },
}, { label = "Snare" })

-- Hi-hat: pure noise, ultra-short envelope — character comes from the highpass.
song:instrument(2, {
  adsr = { 0.0, 0.04, 0.0, 0.02 },
  oscillators = { { wave = "Noise", gain = 0.6 } },
}, { label = "Hat" })

-- Bass: gritty saw + sub sine.
song:instrument(3, {
  adsr = { 0.0, 0.08, 0.7, 0.05 },
  oscillators = {
    { wave = "Saw",  gain = 0.5 },
    { wave = "Sine", gain = 0.45 },
  },
}, { label = "Bass" })

-- Pad: triangle + sine, slow attack, long release — gets tremolo'd.
song:instrument(4, {
  adsr = { 0.35, 0.3, 0.7, 0.7 },
  oscillators = {
    { wave = "Triangle", gain = 0.22 },
    { wave = "Sine",     gain = 0.28 },
  },
}, { label = "Pad" })

-- Lead: saw + square, quick attack — gets distorted.
song:instrument(5, {
  adsr = { 0.005, 0.15, 0.6, 0.25 },
  oscillators = {
    { wave = "Saw",    gain = 0.4 },
    { wave = "Square", gain = 0.18 },
  },
}, { label = "Lead" })

-- =========================================================================
-- Effects
-- =========================================================================

-- Drum chain
song:effect(20, "Bitcrusher", { bits = 6 },                               { label = "snare_crush" })
song:effect(21, "Reverb",     { delay = 0.22, feedback = 0.25, mix = 0.35 }, { label = "snare_verb"  })
song:effect(22, "Highpass",   { cutoff = 6500 },                          { label = "hat_hp"      })

-- Bass chain
song:effect(30, "Distortion", { drive = 2.5, mix = 0.6 },                 { label = "bass_drive"  })
song:effect(31, "Lowpass",    { cutoff = 1100 },                          { label = "bass_lp"     })

-- Pad chain
song:effect(40, "Tremolo",    { rate = 4.5, depth = 0.55 },               { label = "pad_trem"    })
song:effect(41, "Reverb",     { delay = 0.5, feedback = 0.4, mix = 0.6 }, { label = "pad_verb"    })

-- Lead chain
song:effect(50, "Distortion", { drive = 3.0, mix = 0.5 },                 { label = "lead_drive"  })
song:effect(51, "Reverb",     { delay = 0.32, feedback = 0.3, mix = 0.4 }, { label = "lead_verb"   })

-- Trim gains (one per source chain)
song:effect(60, "Gain", { level = 0.65 }, { label = "kick_gain"  })
song:effect(61, "Gain", { level = 0.45 }, { label = "snare_gain" })
song:effect(62, "Gain", { level = 0.25 }, { label = "hat_gain"   })
song:effect(63, "Gain", { level = 0.5  }, { label = "bass_gain"  })
song:effect(64, "Gain", { level = 0.18 }, { label = "pad_gain"   })
song:effect(65, "Gain", { level = 0.32 }, { label = "lead_gain"  })

-- =========================================================================
-- Output
-- =========================================================================
song:output(99, { label = "main" })

-- =========================================================================
-- Routing
-- =========================================================================
song:connection("Kick",        "kick_gain")
song:connection("kick_gain",   "main")

song:connection("Snare",       "snare_crush")
song:connection("snare_crush", "snare_verb")
song:connection("snare_verb",  "snare_gain")
song:connection("snare_gain",  "main")

song:connection("Hat",         "hat_hp")
song:connection("hat_hp",      "hat_gain")
song:connection("hat_gain",    "main")

song:connection("Bass",        "bass_drive")
song:connection("bass_drive",  "bass_lp")
song:connection("bass_lp",     "bass_gain")
song:connection("bass_gain",   "main")

song:connection("Pad",         "pad_trem")
song:connection("pad_trem",    "pad_verb")
song:connection("pad_verb",    "pad_gain")
song:connection("pad_gain",    "main")

song:connection("Lead",        "lead_drive")
song:connection("lead_drive",  "lead_verb")
song:connection("lead_verb",   "lead_gain")
song:connection("lead_gain",   "main")

-- =========================================================================
-- Phrases
-- =========================================================================

-- Kick: 4-on-the-floor, one hit per beat for 16 beats.
local kick_notes = {}
for i = 0, 15 do
  table.insert(kick_notes, { pitch = 36, start = i, duration = 0.1 })
end
song:phrase("Kick Beat", { duration = 16.0, notes = kick_notes })

-- Snare: backbeat — beats 1 & 3 of each bar (8 hits over 4 bars).
local snare_notes = {}
for i = 0, 7 do
  table.insert(snare_notes, { pitch = 60, start = i * 2 + 1, duration = 0.1 })
end
song:phrase("Snare Beat", { duration = 16.0, notes = snare_notes })

-- Hat: 8th notes — 32 hits over 16 beats.
local hat_notes = {}
for i = 0, 31 do
  table.insert(hat_notes, { pitch = 72, start = i * 0.5, duration = 0.05 })
end
song:phrase("Hat Beat", { duration = 16.0, notes = hat_notes })

-- Bass: driving 8th notes on the chord root, with an octave-up on the last
-- 8th of each bar. Progression: Am | F | C | G.
local bass_pattern = {
  -- root, octave-up, bar offset
  { 45, 57, 0  },  -- A2 / A3
  { 41, 53, 4  },  -- F2 / F3
  { 48, 60, 8  },  -- C3 / C4 (jumps register — natural for this progression)
  { 43, 55, 12 },  -- G2 / G3
}
local bass_notes = {}
for _, p in ipairs(bass_pattern) do
  local root, oct, bar = p[1], p[2], p[3]
  for i = 0, 6 do
    table.insert(bass_notes, { pitch = root, start = bar + i * 0.5, duration = 0.4 })
  end
  table.insert(bass_notes, { pitch = oct, start = bar + 3.5, duration = 0.4 })
end
song:phrase("Bassline", { duration = 16.0, notes = bass_notes })

-- Pad: held 3-note chord per bar (i — VI — III — VII).
local pad_chords = {
  -- bar offset, three pitches
  { 0,  { 57, 60, 64 } },  -- A m  (A3, C4, E4)
  { 4,  { 53, 57, 60 } },  -- F maj (F3, A3, C4)
  { 8,  { 60, 64, 67 } },  -- C maj (C4, E4, G4)
  { 12, { 55, 59, 62 } },  -- G maj (G3, B3, D4)
}
local pad_notes = {}
for _, c in ipairs(pad_chords) do
  for _, p in ipairs(c[2]) do
    table.insert(pad_notes, { pitch = p, start = c[1], duration = 4.0 })
  end
end
song:phrase("Pad Chords", { duration = 16.0, notes = pad_notes })

-- Lead: syncopated A-minor-pentatonic melody over the 4-bar progression.
song:phrase("Lead Melody", {
  duration = 16.0,
  notes = {
    -- over Am
    { pitch = 76, start = 0.0,  duration = 1.5 },  -- E5
    { pitch = 79, start = 1.5,  duration = 0.5 },  -- G5
    { pitch = 81, start = 2.0,  duration = 1.0 },  -- A5
    { pitch = 79, start = 3.0,  duration = 1.0 },  -- G5
    -- over F
    { pitch = 77, start = 4.0,  duration = 1.0 },  -- F5
    { pitch = 81, start = 5.0,  duration = 0.5 },  -- A5
    { pitch = 84, start = 5.5,  duration = 1.5 },  -- C6
    { pitch = 81, start = 7.0,  duration = 1.0 },  -- A5
    -- over C
    { pitch = 79, start = 8.0,  duration = 1.0 },  -- G5
    { pitch = 76, start = 9.0,  duration = 1.0 },  -- E5
    { pitch = 79, start = 10.0, duration = 1.5 },  -- G5
    { pitch = 76, start = 11.5, duration = 0.5 },  -- E5
    -- over G
    { pitch = 74, start = 12.0, duration = 1.0 },  -- D5
    { pitch = 79, start = 13.0, duration = 0.5 },  -- G5
    { pitch = 83, start = 13.5, duration = 0.5 },  -- B5
    { pitch = 86, start = 14.0, duration = 2.0 },  -- D6
  },
})

-- =========================================================================
-- Tracks (one phrase each, looping)
-- =========================================================================
song:track({
  targets = { "Kick" },
  initial = "Kick Beat",
  transitions = { { from = "Kick Beat", to = "Kick Beat" } },
})

song:track({
  targets = { "Snare" },
  initial = "Snare Beat",
  transitions = { { from = "Snare Beat", to = "Snare Beat" } },
})

song:track({
  targets = { "Hat" },
  initial = "Hat Beat",
  transitions = { { from = "Hat Beat", to = "Hat Beat" } },
})

song:track({
  targets = { "Bass" },
  initial = "Bassline",
  transitions = { { from = "Bassline", to = "Bassline" } },
})

song:track({
  targets = { "Pad" },
  initial = "Pad Chords",
  transitions = { { from = "Pad Chords", to = "Pad Chords" } },
})

song:track({
  targets = { "Lead" },
  initial = "Lead Melody",
  transitions = { { from = "Lead Melody", to = "Lead Melody" } },
})
