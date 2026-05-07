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
local piano_a_notes = {}
local piano_a_pitches = { 79, 84, 86, 91 }
for i = 0, 31 do
    table.insert(piano_a_notes, {
        pitch = piano_a_pitches[(i % 4) + 1],
        start = i * 0.5,
        duration = 0.45,
    })
end

song:phrase("Piano A", {
    duration = 16.0,
    on_enter = function()
        cycle = (cycle or 0) + 1
        print("[hook] starting cycle " .. cycle)
    end,
    on_exit = function()
        print("[hook] piano A leaving on cycle " .. cycle)
    end,
    notes = piano_a_notes,
})

song:phrase("Piano B", {
    rtttl = [[
        8a3 8c3 8e3 8f3
        8a3 8c3 8e3 8f3
        8a3 8c3 8e3 8f3
        8a3 8c3 8e3 8f3
        8a3 8c3 8e3 8f3
        8a3 8c3 8e3 8f3
        8a3 8c3 8e3 8f3
        8a3 8c3 8e3 8f3
    ]],
})

song:phrase("Bass A", {
    duration = 16.0,
    notes = {  -- table = static
        { pitch = 51, start = 0,    duration = 7.5 },
        { pitch = 58, start = 7.5,  duration = 0.5 },
        { pitch = 51, start = 8.0,  duration = 6.0 },
        { pitch = 63, start = 14.0, duration = 2.0 },
    },
})

song:phrase("Bass B", {
    duration = 16.0,
    notes = function()  -- function = re-evaluate each loop
        return {
            { pitch = 62, start = 0,    duration = 7.5 },
            { pitch = 57, start = 7.5,  duration = 0.5 },
            { pitch = 50, start = 8.0,  duration = 6.0 },
            { pitch = 50, start = 14.5, duration = 0.5 },
        }
    end,
})

-- Tracks
song:track({
    targets = { 0 },
    initial = "Piano A",
    transitions = {
        { from = "Piano A", to = "Piano B" },
        { from = "Piano B", to = "Piano A" },
    },
})

song:track({
    targets = { "Bass" },
    initial = "Bass A",
    transitions = {
        { from = "Bass A", to = "Bass B" },
        { from = "Bass B", to = "Bass A" },
    },
})
