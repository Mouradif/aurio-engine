-- Twilight — a melancholic D-minor piece.
--
-- A solo violin plays 8 bars of intro, then drums and a synth bass drop in
-- for a 4-bar main loop (Dm – Bb – F – C).
--
-- The violin is the conductor: it writes `section` to a global, and the
-- follower tracks (kick, snare, hat, bass) start on silent intro phrases
-- and switch to their main pattern as soon as `section == "main"`.
-- Conductor MUST be registered first so its on_enter fires before any
-- follower reads the global in the same timing-loop iteration.

song:bpm(80)

-- =========================================================================
-- Instruments
-- =========================================================================

-- Violin: saw is the canonical bowed-string approximation; triangle softens
-- the top. Slow-ish attack mimics the bow draw, medium release lets notes
-- bloom into the reverb.
song:instrument(0, {
  adsr = { 0.08, 0.20, 0.75, 0.40 },
  oscillators = {
    { wave = "Saw",      gain = 0.32 },
    { wave = "Triangle", gain = 0.22 },
  },
}, { label = "Violin" })

song:instrument(1, {
  adsr = { 0.0, 0.20, 0.0, 0.08 },
  oscillators = { { wave = "Sine", gain = 1.0 } },
}, { label = "Kick" })

song:instrument(2, {
  adsr = { 0.0, 0.12, 0.0, 0.06 },
  oscillators = {
    { wave = "Noise",    gain = 0.55 },
    { wave = "Triangle", gain = 0.20 },
  },
}, { label = "Snare" })

song:instrument(3, {
  adsr = { 0.0, 0.04, 0.0, 0.02 },
  oscillators = { { wave = "Noise", gain = 0.5 } },
}, { label = "Hat" })

-- Synth bass: saw + sub-sine. Sine carries the fundamental, saw adds bite.
song:instrument(4, {
  adsr = { 0.005, 0.10, 0.7, 0.10 },
  oscillators = {
    { wave = "Saw",  gain = 0.35 },
    { wave = "Sine", gain = 0.40 },
  },
}, { label = "Bass" })

-- =========================================================================
-- Effects
-- =========================================================================

-- Violin chain: tremolo doubles as vibrato (~5.5 Hz, mild depth).
song:effect(10, "Tremolo", { rate = 5.5, depth = 0.25 },                   { label = "violin_vib"  })
song:effect(11, "Lowpass", { cutoff = 4200 },                              { label = "violin_lp"   })
song:effect(12, "Reverb",  { delay = 0.45, feedback = 0.40, mix = 0.55 },  { label = "violin_verb" })
song:effect(13, "Gain",    { level = 0.08 },                               { label = "violin_gain" })

song:effect(20, "Lowpass", { cutoff = 1800 },                              { label = "kick_lp"     })
song:effect(21, "Gain",    { level = 0.45 },                               { label = "kick_gain"   })

song:effect(30, "Bitcrusher", { bits = 5 },                                { label = "snare_crush" })
song:effect(31, "Highpass",   { cutoff = 220 },                            { label = "snare_hp"    })
song:effect(32, "Gain",       { level = 0.32 },                            { label = "snare_gain"  })

song:effect(40, "Lowpass", { cutoff = 7500 },                              { label = "hat_lp"      })
song:effect(41, "Gain",    { level = 0.18 },                               { label = "hat_gain"    })

song:effect(50, "Distortion", { drive = 1.4, mix = 0.30 },                 { label = "bass_dist"   })
song:effect(51, "Lowpass",    { cutoff = 800 },                            { label = "bass_lp"     })
song:effect(52, "Gain",       { level = 0.32 },                            { label = "bass_gain"   })

-- =========================================================================
-- Output + routing
-- =========================================================================
song:output(99, { label = "main" })

song:connection("Violin",      "violin_vib")
song:connection("violin_vib",  "violin_lp")
song:connection("violin_lp",   "violin_verb")
song:connection("violin_verb", "violin_gain")
song:connection("violin_gain", "main")

song:connection("Kick",      "kick_lp")
song:connection("kick_lp",   "kick_gain")
song:connection("kick_gain", "main")

song:connection("Snare",       "snare_crush")
song:connection("snare_crush", "snare_hp")
song:connection("snare_hp",    "snare_gain")
song:connection("snare_gain",  "main")

song:connection("Hat",      "hat_lp")
song:connection("hat_lp",   "hat_gain")
song:connection("hat_gain", "main")

song:connection("Bass",      "bass_dist")
song:connection("bass_dist", "bass_lp")
song:connection("bass_lp",   "bass_gain")
song:connection("bass_gain", "main")

-- =========================================================================
-- Phrases
-- =========================================================================

-- Violin Intro: 8 bars (32 beats) of unaccompanied melody outlining D minor.
song:phrase("Violin Intro", {
  duration = 8.0,
  on_enter = function()
    section = "intro"
    print("[section] intro")
  end,
  notes = {
    -- bar 1
    { pitch = 74, start = 0.0,  duration = 3.5 },  -- D5
    -- bar 2
    { pitch = 77, start = 4.0,  duration = 1.5 },  -- F5
    { pitch = 81, start = 5.5,  duration = 2.5 },  -- A5
  },
})

-- Violin Main: 4-bar melody over Dm – Bb – F – C.
song:phrase("Violin Main", {
  duration = 16.0,
  on_enter = function()
    section = "main"
    print("[section] main")
  end,
  notes = {
    -- bar 1 over Dm
    { pitch = 81, start = 0.0,  duration = 1.5 },  -- A5
    { pitch = 77, start = 1.5,  duration = 1.0 },  -- F5
    { pitch = 74, start = 2.5,  duration = 1.5 },  -- D5
    -- bar 2 over Bb
    { pitch = 74, start = 4.0,  duration = 1.0 },  -- D5
    { pitch = 77, start = 5.0,  duration = 1.0 },  -- F5
    { pitch = 82, start = 6.0,  duration = 2.0 },  -- Bb5
    -- bar 3 over F
    { pitch = 84, start = 8.0,  duration = 1.0 },  -- C6
    { pitch = 81, start = 9.0,  duration = 1.5 },  -- A5
    { pitch = 77, start = 10.5, duration = 1.5 },  -- F5
    -- bar 4 over C
    { pitch = 76, start = 12.0, duration = 1.0 },  -- E5
    { pitch = 79, start = 13.0, duration = 1.0 },  -- G5
    { pitch = 76, start = 14.0, duration = 1.0 },  -- E5
    { pitch = 74, start = 15.0, duration = 1.0 },  -- D5
  },
})

-- Followers: silent 32-beat intros aligned with the violin solo, then
-- their main patterns kick in once the conductor flips `section`.

song:phrase("Kick Intro", { duration = 8.0, notes = {} })
local kick_notes = {}
for bar = 0, 3 do
  local b = bar * 4
  table.insert(kick_notes, { pitch = 36, start = b,       duration = 0.1 })
  table.insert(kick_notes, { pitch = 36, start = b + 2.5, duration = 0.1 })
end
song:phrase("Kick Main", { duration = 16.0, notes = kick_notes })

song:phrase("Snare Intro", { duration = 8.0, notes = {} })
local snare_notes = {}
for i = 0, 7 do
  table.insert(snare_notes, { pitch = 60, start = i * 2 + 1, duration = 0.1 })
end
song:phrase("Snare Main", { duration = 16.0, notes = snare_notes })

song:phrase("Hat Intro", { duration = 8.0, notes = {} })
local hat_notes = {}
for i = 0, 15 do
  table.insert(hat_notes, { pitch = 72, start = i + 0.5, duration = 0.05 })
end
song:phrase("Hat Main", { duration = 16.0, notes = hat_notes })

song:phrase("Bass Intro", { duration = 8.0, notes = {} })
-- 8th-note pulse on the chord root, with an octave-up on the last 8th of
-- each bar for movement. Roots: D2, Bb2, F2, C2.
local bass_pattern = {}
local roots = { { 38, 0 }, { 46, 4 }, { 41, 8 }, { 36, 12 } }
for _, p in ipairs(roots) do
  local root, bar = p[1], p[2]
  for i = 0, 6 do
    table.insert(bass_pattern, { pitch = root, start = bar + i * 0.5, duration = 0.4 })
  end
  table.insert(bass_pattern, { pitch = root + 12, start = bar + 3.5, duration = 0.4 })
end
song:phrase("Bass Main", { duration = 16.0, notes = bass_pattern })

-- =========================================================================
-- Tracks
-- =========================================================================

-- Conductor: Intro plays once, then Main loops forever.
song:track({
  targets = { "Violin" },
  initial = "Violin Intro",
  transitions = {
    { from = "Violin Intro", to = "Violin Main" },
    { from = "Violin Main",  to = "Violin Main" },
  },
})

-- Each follower starts silent and waits for the conductor to flip `section`.
local function follower(target, intro_name, main_name)
  song:track({
    targets = { target },
    initial = intro_name,
    transitions = {
      { from = intro_name, to = main_name,
        condition = function() return section == "main" end },
      { from = main_name,  to = main_name },
    },
  })
end

follower("Kick",  "Kick Intro",  "Kick Main")
follower("Snare", "Snare Intro", "Snare Main")
follower("Hat",   "Hat Intro",   "Hat Main")
follower("Bass",  "Bass Intro",  "Bass Main")
