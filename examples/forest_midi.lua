-- forest.lua, but the bass is played by an external synth over MIDI.
--
-- The melody still runs through aurio's own instrument 0; only the bass track
-- changed target. Its phrases are untouched — `song:midi_out(...)` receives the
-- same note events a `song:instrument(...)` would, so any phrase can drive
-- hardware instead of a built-in voice.
--
-- Wiring assumed here: a U6MIDI Pro with the minikorg on Port 1. The `port`
-- field is a case-insensitive substring of the MIDI port name, so "Port 1"
-- matches "U6MIDI Pro Port 1".
--
-- Notes on the MIDI-out node:
--   * It makes no sound of its own, so it needs no connection to an output.
--     The audio you hear from it comes out of the synth's own jacks.
--   * `channel` is 1..16 (1 if omitted).
--   * Held notes are released on shutdown, so Ctrl-C won't leave a drone.

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

-- The bass, on the minikorg. Add `transpose = 12` if the phrases sit below
-- the synth's range — the pitches below reach down to MIDI 39.
song:midi_out(1, { port = "Port 1", channel = 1, label = "korg" })
song:midi_out(5, { port = "Port 2", channel = 1, label = "arturia" })

-- Effects (the bass gain lives on the synth now, so only the piano needs one)
song:effect(2, { kind = "Reverb", label = "reverb", params = { delay = 0.5, feedback = 0.5, mix = 0.8 } })
song:effect(3, { kind = "Gain", label = "piano_gain", params = { level = 0.3 } })

-- Outputs
song:output(10, { label = "main" })

-- Connections
song:connection(0,            "reverb")
song:connection("reverb",     "piano_gain")
song:connection("piano_gain", "main")

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
    { pitch = 79, start = 0.0, duration = 0.45 },
    { pitch = 84, start = 0.5, duration = 0.45 },
    { pitch = 86, start = 1.0, duration = 0.45 },
    { pitch = 91, start = 1.5, duration = 0.45 },
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
  targets = { "arturia" },
  initial = "Piano A",
  transitions = {
    { from = "Piano A", to = "Piano B", condition = function() return piano_a_count == 8 end },
    { from = "Piano B", to = "Piano A", condition = function() return piano_b_count == 8 end },
  },
})

-- Same track as forest.lua, pointed at the MIDI port instead of a synth voice.
song:track({
  targets = { "korg" },
  initial = "Bass A",
  transitions = {
    { from = "Bass A", to = "Bass B" },
    { from = "Bass B", to = "Bass C" },
    { from = "Bass C", to = "Bass D" },
    { from = "Bass D", to = "Bass A" },
  },
})
