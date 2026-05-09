song:bpm(120)

-- Instruments
song:instrument(0, {
  adsr = { 0.025, 0.1, 1.0, 0.2 },
  oscillators = {
    { wave = "Sine",     gain = 0.4 },
    { wave = "Triangle", gain = 0.2 },
    { wave = "Saw",      gain = 0.1 },
  },
})

song:instrument(1, {
  adsr = { 0.0, 0.1, 0.7, 0.05 },
  oscillators = { { wave = "Sine", gain = 1.0 } },
}, { label = "Bass" })

-- Effects
song:effect(2, "Reverb", { delay = 0.5, feedback = 0.5, mix = 0.8 }, { label = "reverb" })
song:effect(3, "Gain",   { level = 0.3 }, { label = "piano_gain" })
song:effect(4, "Gain",   { level = 0.6 }, { label = "bass_gain" })

-- Outputs
song:output(10, { label = "main" })

-- Connections (numeric ID or label, interchangeable)
song:connection(0,            "reverb")
song:connection("reverb",     "piano_gain")
song:connection("piano_gain", "main")
song:connection("Bass",       "bass_gain")
song:connection("bass_gain",  "main")

-- Phrases
song:phrase("Piano A", {
  duration = 2.0,
  on_enter = function()
    piano_a_count = 0
    print("[hook] piano A entering")
  end,
  on_loop = function()
    piano_a_count = piano_a_count + 1
    print("[hook] piano A loop " .. piano_a_count)
  end,
  on_exit = function()
    print("[hook] piano A leaving after " .. piano_a_count .. " loops")
  end,
  notes = {
    { pitch = 79, start = 0.0, duration = 0.45 },
    { pitch = 84, start = 0.5, duration = 0.45 },
    { pitch = 86, start = 1.0, duration = 0.45 },
    { pitch = 91, start = 1.5, duration = 0.45 },
  },
})

song:phrase("Piano B", {
  on_enter = function()
    piano_b_count = 0
    print("[hook] piano B entering")
  end,
  on_loop = function()
    piano_b_count = piano_b_count + 1
    print("[hook] piano B loop " .. piano_b_count)
  end,
  rtttl = [[
        8a3 8c4 8e4 8f4
    ]],
})

song:phrase("Bass A", {
  duration = 16.0,
  notes = {
    { pitch = 51, start = 0,    duration = 7.5 },
    { pitch = 58, start = 7.5,  duration = 0.5 },
    { pitch = 51, start = 8.0,  duration = 6.0 },
    { pitch = 63, start = 14.0, duration = 2.0 },
  },
})

song:phrase("Bass B", {
  duration = 16.0,
  notes = {
    { pitch = 62, start = 0,    duration = 7.5 },
    { pitch = 57, start = 7.5,  duration = 0.5 },
    { pitch = 50, start = 8.0,  duration = 6.0 },
    { pitch = 50, start = 14.5, duration = 0.5 },
  }
})

song:phrase("Bass C", {
  duration = 16.0,
  notes = {
    { pitch = 51, start = 0,    duration = 6.5 },
    { pitch = 58, start = 6.5,  duration = 1.0 },
    { pitch = 39, start = 7.5,  duration = 0.5 },
    { pitch = 51, start = 8.0,  duration = 1.0 },
    { pitch = 51, start = 9.5,  duration = 1.0 },
    { pitch = 51, start = 11.0,  duration = 0.5 },
    { pitch = 63, start = 14.0, duration = 2.0 },
  }
})

song:phrase("Bass D", {
  duration = 16.0,
  notes = {
    { pitch = 62, start = 0,    duration = 6.5 },
    { pitch = 57, start = 6.5,  duration = 1.0 },
    { pitch = 62, start = 7.5,  duration = 0.5 },
    { pitch = 50, start = 8.0,  duration = 1.0 },
    { pitch = 50, start = 9.5,  duration = 1.0 },
    { pitch = 50, start = 11.0,  duration = 1.0 },
  }
})

-- Tracks
song:track({
  targets = { 0 },
  initial = "Piano A",
  transitions = {
    { from = "Piano A", to = "Piano B", condition = function() return piano_a_count == 7 end },
    { from = "Piano B", to = "Piano A", condition = function() return piano_b_count == 7 end },
  },
})

song:track({
  targets = { "Bass" },
  initial = "Bass A",
  transitions = {
    { from = "Bass A", to = "Bass B" },
    { from = "Bass B", to = "Bass C" },
    { from = "Bass C", to = "Bass D" },
    { from = "Bass D", to = "Bass A" },
  },
})
