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
  song:effect(20, { kind = "Lowpass", label = "kick_lp", params = { cutoff = 1800 } })
  song:effect(21, { kind = "Bitcrusher", label = "snare_crush", params = { bits = 5 } })
  song:effect(22, { kind = "Highpass", label = "snare_hp", params = { cutoff = 220 } })
  song:effect(23, { kind = "Lowpass", label = "hat_lp", params = { cutoff = 7500 } })
  song:effect(24, { kind = "Distortion", label = "bass_dist", params = { drive = 1.6, mix = 0.4 } })
  song:effect(25, { kind = "Lowpass", label = "bass_lp", params = { cutoff = 700 } })
  song:effect(26, { kind = "Tremolo", label = "chord_trem", params = { rate = 2.0, depth = 0.30 } })
  song:effect(27, { kind = "Lowpass", label = "chord_lp", params = { cutoff = 3200 } })
  song:effect(28, { kind = "Reverb", label = "chord_verb", params = { delay = 0.40, feedback = 0.35, mix = 0.50 } })
  song:effect(29, { kind = "Lowpass", label = "lead_lp", params = { cutoff = 4200 } })
  song:effect(30, { kind = "Reverb", label = "lead_verb", params = { delay = 0.50, feedback = 0.40, mix = 0.55 } })

  song:effect(40, { kind = "Gain", label = "kick_gain", params = { level = 0.55 } })
  song:effect(41, { kind = "Gain", label = "snare_gain", params = { level = 0.32 } })
  song:effect(42, { kind = "Gain", label = "hat_gain", params = { level = 0.16 } })
  song:effect(43, { kind = "Gain", label = "bass_gain", params = { level = 0.40 } })
  song:effect(44, { kind = "Gain", label = "chord_gain", params = { level = 0.20 } })
  song:effect(45, { kind = "Gain", label = "lead_gain", params = { level = 0.28 } })

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
