-- Instruments, effects, output, and routing for the lo-fi song.
return function(song)
  -- =========================================================================
  -- Instruments
  -- =========================================================================
  song:instrument(0, {
    adsr = { 0.0, 0.20, 0.0, 0.08 },
    oscillators = { { wave = "Sine", gain = 1.0 } },
  }, { label = "Kick" })

  song:instrument(1, {
    adsr = { 0.0, 0.12, 0.0, 0.06 },
    oscillators = {
      { wave = "Noise",    gain = 0.55 },
      { wave = "Triangle", gain = 0.20 },
    },
  }, { label = "Snare" })

  song:instrument(2, {
    adsr = { 0.0, 0.04, 0.0, 0.02 },
    oscillators = { { wave = "Noise", gain = 0.5 } },
  }, { label = "Hat" })

  song:instrument(3, {
    adsr = { 0.005, 0.10, 0.7, 0.10 },
    oscillators = {
      { wave = "Sine",     gain = 0.55 },
      { wave = "Triangle", gain = 0.30 },
    },
  }, { label = "Bass" })

  -- "Rhodes-y" pad: triangle + sine, mellow envelope.
  song:instrument(4, {
    adsr = { 0.02, 0.25, 0.55, 0.45 },
    oscillators = {
      { wave = "Triangle", gain = 0.28 },
      { wave = "Sine",     gain = 0.28 },
    },
  }, { label = "Chord" })

  song:instrument(5, {
    adsr = { 0.05, 0.20, 0.6, 0.40 },
    oscillators = {
      { wave = "Sine",     gain = 0.45 },
      { wave = "Triangle", gain = 0.25 },
    },
  }, { label = "Lead" })

  -- =========================================================================
  -- Effects
  -- =========================================================================
  song:effect(20, "Lowpass",    { cutoff = 1800 },                            { label = "kick_lp"     })
  song:effect(21, "Bitcrusher", { bits = 5 },                                 { label = "snare_crush" })
  song:effect(22, "Highpass",   { cutoff = 220 },                             { label = "snare_hp"    })
  song:effect(23, "Lowpass",    { cutoff = 7500 },                            { label = "hat_lp"      })
  song:effect(24, "Distortion", { drive = 1.6, mix = 0.4 },                   { label = "bass_dist"   })
  song:effect(25, "Lowpass",    { cutoff = 700 },                             { label = "bass_lp"     })
  song:effect(26, "Tremolo",    { rate = 2.0, depth = 0.30 },                 { label = "chord_trem"  })
  song:effect(27, "Lowpass",    { cutoff = 3200 },                            { label = "chord_lp"    })
  song:effect(28, "Reverb",     { delay = 0.40, feedback = 0.35, mix = 0.50 }, { label = "chord_verb"  })
  song:effect(29, "Lowpass",    { cutoff = 4200 },                            { label = "lead_lp"     })
  song:effect(30, "Reverb",     { delay = 0.50, feedback = 0.40, mix = 0.55 }, { label = "lead_verb"   })

  song:effect(40, "Gain", { level = 0.55 }, { label = "kick_gain"  })
  song:effect(41, "Gain", { level = 0.32 }, { label = "snare_gain" })
  song:effect(42, "Gain", { level = 0.16 }, { label = "hat_gain"   })
  song:effect(43, "Gain", { level = 0.40 }, { label = "bass_gain"  })
  song:effect(44, "Gain", { level = 0.20 }, { label = "chord_gain" })
  song:effect(45, "Gain", { level = 0.28 }, { label = "lead_gain"  })

  -- =========================================================================
  -- Output
  -- =========================================================================
  song:output(99, { label = "main" })

  -- =========================================================================
  -- Routing
  -- =========================================================================
  song:connection("Kick",        "kick_lp")
  song:connection("kick_lp",     "kick_gain")
  song:connection("kick_gain",   "main")

  song:connection("Snare",       "snare_crush")
  song:connection("snare_crush", "snare_hp")
  song:connection("snare_hp",    "snare_gain")
  song:connection("snare_gain",  "main")

  song:connection("Hat",         "hat_lp")
  song:connection("hat_lp",      "hat_gain")
  song:connection("hat_gain",    "main")

  song:connection("Bass",        "bass_dist")
  song:connection("bass_dist",   "bass_lp")
  song:connection("bass_lp",     "bass_gain")
  song:connection("bass_gain",   "main")

  song:connection("Chord",       "chord_trem")
  song:connection("chord_trem",  "chord_lp")
  song:connection("chord_lp",    "chord_verb")
  song:connection("chord_verb",  "chord_gain")
  song:connection("chord_gain",  "main")

  song:connection("Lead",        "lead_lp")
  song:connection("lead_lp",     "lead_verb")
  song:connection("lead_verb",   "lead_gain")
  song:connection("lead_gain",   "main")
end
