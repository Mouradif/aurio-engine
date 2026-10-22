-- Funk — a 2-bar E-dorian groove (Em7 | A7) with a lead that answers it.
--
-- Drums and bass loop a 2-bar pattern forever. The lead track sits out for
-- 4 bars, plays a 2-bar hook, then goes back to resting, so the hook comes
-- around every 6 bars over the same groove.
--
-- Routing:
--   Kick  → kick_gain                          → main
--   Snare → snare_hp → snare_verb → snare_gain → main
--   Hat   → hat_hp   → hat_gain                 → main
--   Bass  → bass_wah → bass_drive → bass_gain  → main   (swept lowpass = wah)
--   Lead  → lead_lp  → lead_delay → lead_gain  → main   (dotted-8th delay)

song:bpm(104)

-- =========================================================================
-- Instruments
-- =========================================================================

song:instrument(0, {
  adsr = { 0.0, 0.16, 0.0, 0.04 },
  oscillators = { { wave = "Sine", gain = 1.0 } },
}, { label = "Kick" })

song:instrument(1, {
  adsr = { 0.0, 0.12, 0.0, 0.06 },
  oscillators = {
    { wave = "Noise",    gain = 0.6 },
    { wave = "Triangle", gain = 0.2 },
  },
}, { label = "Snare" })

song:instrument(2, {
  adsr = { 0.0, 0.03, 0.0, 0.02 },
  oscillators = { { wave = "Noise", gain = 0.6 } },
}, { label = "Hat" })

-- Bass: plucky square + saw with a sub sine; short decay keeps it bouncy.
song:instrument(3, {
  adsr = { 0.0, 0.12, 0.45, 0.04 },
  oscillators = {
    { wave = "Square", gain = 0.3 },
    { wave = "Saw",    gain = 0.3 },
    { wave = "Sine",   gain = 0.5 },
  },
}, { label = "Bass" })

-- Lead: bright saw + square, snappy attack.
song:instrument(4, {
  adsr = { 0.005, 0.1, 0.65, 0.12 },
  oscillators = {
    { wave = "Saw",    gain = 0.35 },
    { wave = "Square", gain = 0.25 },
  },
}, { label = "Lead" })

-- =========================================================================
-- Effects
-- =========================================================================

song:effect(20, { kind = "Highpass", label = "snare_hp",   params = { cutoff = 250 } })
song:effect(21, { kind = "Reverb",   label = "snare_verb", params = { delay = 0.12, feedback = 0.2, mix = 0.2 } })
song:effect(22, { kind = "Highpass", label = "hat_hp",     params = { cutoff = 7500 } })

-- Wah: lowpass cutoff swept once per bar (104 BPM / 4 beats / 60 s ≈ 0.433 Hz).
song:effect(30, { kind = "Lowpass", label = "bass_wah",
  params = { cutoff = { name = "sine", freq = 0.4333, low = 350, high = 1800 } } })
song:effect(31, { kind = "Distortion", label = "bass_drive", params = { drive = 1.8, mix = 0.35 } })

song:effect(40, { kind = "Lowpass", label = "lead_lp",    params = { cutoff = 4500 } })
song:effect(41, { kind = "Delay",   label = "lead_delay", params = { time = "8.", feedback = 0.3, mix = 0.25 } })

-- Trims keep the summed peak comfortably under full scale.
song:effect(60, { kind = "Gain", label = "kick_gain",  params = { level = 0.4  } })
song:effect(61, { kind = "Gain", label = "snare_gain", params = { level = 0.25 } })
song:effect(62, { kind = "Gain", label = "hat_gain",   params = { level = 0.15 } })
song:effect(63, { kind = "Gain", label = "bass_gain",  params = { level = 0.28 } })
song:effect(64, { kind = "Gain", label = "lead_gain",  params = { level = 0.18 } })

song:output(99, { label = "main" })

-- =========================================================================
-- Routing
-- =========================================================================
song:connection("Kick",       "kick_gain")
song:connection("kick_gain",  "main")

song:connection("Snare",      "snare_hp")
song:connection("snare_hp",   "snare_verb")
song:connection("snare_verb", "snare_gain")
song:connection("snare_gain", "main")

song:connection("Hat",        "hat_hp")
song:connection("hat_hp",     "hat_gain")
song:connection("hat_gain",   "main")

song:connection("Bass",       "bass_wah")
song:connection("bass_wah",   "bass_drive")
song:connection("bass_drive", "bass_gain")
song:connection("bass_gain",  "main")

song:connection("Lead",       "lead_lp")
song:connection("lead_lp",    "lead_delay")
song:connection("lead_delay", "lead_gain")
song:connection("lead_gain",  "main")

-- =========================================================================
-- Phrases (written on a 16th-note grid: step 0..31 over 2 bars)
-- =========================================================================

local S = 0.25 -- one 16th note, in beats

-- { step, velocity } → notes at a fixed pitch and duration.
local function hits(pitch, duration, list)
  local notes = {}
  for _, h in ipairs(list) do
    table.insert(notes, { pitch = pitch, start = h[1] * S, duration = duration, velocity = h[2] })
  end
  return notes
end

-- Kick: syncopated, dodges the backbeat, pickup into bar 1 at the end.
song:phrase("Kick Groove", { duration = 8.0, notes = hits(33, 0.1, {
  { 0, 120 }, { 3, 95 }, { 8, 110 }, { 10, 100 }, { 13, 90 },
  { 16, 120 }, { 19, 95 }, { 22, 105 }, { 26, 100 }, { 31, 85 },
}) })

-- Snare: hard backbeat on 2 and 4, quiet ghost notes in between.
song:phrase("Snare Groove", { duration = 8.0, notes = hits(60, 0.1, {
  { 4, 120 }, { 7, 35 }, { 9, 40 }, { 12, 120 }, { 15, 35 },
  { 18, 30 }, { 20, 120 }, { 23, 40 }, { 25, 35 }, { 28, 120 }, { 30, 45 }, { 31, 55 },
}) })

-- Hat: straight 16ths, accented 8ths, softer offbeat 16ths.
local hat = {}
for step = 0, 31 do
  local vel = (step % 4 == 2) and 110 or ((step % 2 == 0) and 85 or 45)
  table.insert(hat, { step, vel })
end
song:phrase("Hat Groove", { duration = 8.0, notes = hits(72, 0.05, hat) })

-- Bass: { step, length in steps, pitch, velocity }. Em7 | A7, octave pops,
-- chromatic walk-ups into each bar.
local bass_line = {
  -- Em7
  { 0,  2, 40, 120 }, -- E2
  { 3,  1, 40, 75  }, -- E2 (ghost)
  { 4,  1, 52, 125 }, -- E3 pop
  { 6,  1, 43, 95  }, -- G2
  { 7,  1, 45, 100 }, -- A2
  { 8,  2, 47, 115 }, -- B2
  { 10, 1, 50, 95  }, -- D3
  { 11, 1, 52, 120 }, -- E3 pop
  { 12, 1, 40, 105 }, -- E2
  { 14, 1, 43, 95  }, -- G2
  { 15, 1, 44, 100 }, -- G#2 → A
  -- A7
  { 16, 2, 45, 120 }, -- A2
  { 19, 1, 45, 75  }, -- A2 (ghost)
  { 20, 1, 57, 125 }, -- A3 pop
  { 22, 1, 43, 95  }, -- G2
  { 23, 1, 40, 100 }, -- E2
  { 24, 2, 43, 110 }, -- G2
  { 26, 1, 45, 100 }, -- A2
  { 27, 1, 49, 110 }, -- C#3
  { 28, 1, 52, 120 }, -- E3
  { 29, 1, 50, 95  }, -- D3
  { 30, 1, 47, 95  }, -- B2
  { 31, 1, 39, 105 }, -- D#2 → E
}
local bass_notes = {}
for _, n in ipairs(bass_line) do
  table.insert(bass_notes, {
    pitch = n[3], start = n[1] * S, duration = n[2] * S * 0.85, velocity = n[4],
  })
end
song:phrase("Bass Groove", { duration = 8.0, notes = bass_notes })

-- Lead: silent for 4 bars, then a 2-bar syncopated E-dorian hook.
song:phrase("Lead Rest", { duration = 16.0, notes = {} })

song:phrase("Lead Hook", {
  duration = 8.0,
  notes = {
    -- over Em7
    { pitch = 76, start = 0.5,  duration = 0.2  }, -- E5
    { pitch = 79, start = 0.75, duration = 0.2  }, -- G5
    { pitch = 81, start = 1.0,  duration = 0.45 }, -- A5
    { pitch = 83, start = 1.75, duration = 0.7  }, -- B5 (pushed)
    { pitch = 81, start = 2.75, duration = 0.2  }, -- A5
    { pitch = 79, start = 3.0,  duration = 0.2  }, -- G5
    { pitch = 76, start = 3.25, duration = 0.45 }, -- E5
    { pitch = 74, start = 3.75, duration = 0.2  }, -- D5
    -- over A7
    { pitch = 76, start = 4.0,  duration = 0.45 }, -- E5
    { pitch = 78, start = 4.75, duration = 0.2  }, -- F#5
    { pitch = 79, start = 5.0,  duration = 0.2  }, -- G5
    { pitch = 81, start = 5.25, duration = 0.45 }, -- A5
    { pitch = 85, start = 5.75, duration = 0.2  }, -- C#6
    { pitch = 83, start = 6.0,  duration = 0.45 }, -- B5
    { pitch = 81, start = 6.5,  duration = 0.2  }, -- A5
    { pitch = 79, start = 6.75, duration = 0.2  }, -- G5
    { pitch = 76, start = 7.0,  duration = 0.9  }, -- E5
  },
})

-- =========================================================================
-- Tracks
-- =========================================================================
local function loop(target, phrase)
  song:track({
    targets = { target },
    initial = phrase,
    transitions = { { from = phrase, to = phrase } },
  })
end

loop("Kick",  "Kick Groove")
loop("Snare", "Snare Groove")
loop("Hat",   "Hat Groove")
loop("Bass",  "Bass Groove")

song:track({
  targets = { "Lead" },
  initial = "Lead Rest",
  transitions = {
    { from = "Lead Rest", to = "Lead Hook" },
    { from = "Lead Hook", to = "Lead Rest" },
  },
})
