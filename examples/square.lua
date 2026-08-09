song:bpm(120)

song:instrument(0, {
  adsr = { 0.0, 0.0, 1.0, 0.0 },
  oscillators = {
    { wave = "Square",   gain = 1.0 },
  },
})

-- Effects
song:effect(1, { kind = "Gain", label = "gain", params = { level = 0.5 } })

-- Outputs
song:output(10, { label = "main" })

-- Connections (numeric ID or label, interchangeable)
song:connection(0, 1)
song:connection(1, 10)

-- Phrases
song:phrase("Piano A", {
  duration = 2.0,
  on_enter = function()
    piano_a_count = 1
    print("[hook] piano A entering")
  end,
  on_loop = function()
    print("[hook] piano A loop " .. piano_a_count)
    piano_a_count = piano_a_count + 1
  end,
  on_exit = function()
    print("[hook] piano A leaving after " .. piano_a_count .. " loops")
  end,
  notes = {
    { pitch = 79, start = 0.0, duration = 2.0 },
  },
})

song:phrase("Piano B", {
  on_enter = function()
    piano_b_count = 1
    print("[hook] piano B entering")
  end,
  on_loop = function()
    print("[hook] piano B loop " .. piano_b_count)
    piano_b_count = piano_b_count + 1
  end,
  rtttl = [[
        2a3
    ]],
})

-- Tracks
song:track({
  targets = { 0 },
  initial = "Piano A",
  transitions = {
    { from = "Piano A", to = "Piano B", condition = function() return piano_a_count == 8 end },
    { from = "Piano B", to = "Piano A", condition = function() return piano_b_count == 8 end },
  },
})
