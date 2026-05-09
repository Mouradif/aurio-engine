-- Registers the lead + bass instruments, an effects chain, and the output.
-- All modules in this song look up their target instrument by the labels
-- declared here, so changing them in one place is enough.
return function(song)
    song:instrument(0, {
        adsr = { 0.005, 0.15, 0.7, 0.25 },
        oscillators = {
            { wave = "Saw",    gain = 0.30 },
            { wave = "Square", gain = 0.15 },
            { wave = "Sine",   gain = 0.25 },
        },
    }, { label = "Lead" })

    song:instrument(1, {
        adsr = { 0.0, 0.08, 0.6, 0.05 },
        oscillators = {
            { wave = "Saw",  gain = 0.55 },
            { wave = "Sine", gain = 0.45 },
        },
    }, { label = "Bass" })

    song:effect(10, "Reverb", { delay = 0.35, feedback = 0.3, mix = 0.45 }, { label = "lead_verb" })
    song:effect(11, "Gain",   { level = 0.35 }, { label = "lead_gain" })
    song:effect(12, "Gain",   { level = 0.55 }, { label = "bass_gain" })
    song:output(20, { label = "main" })

    song:connection("Lead",      "lead_verb")
    song:connection("lead_verb", "lead_gain")
    song:connection("lead_gain", "main")
    song:connection("Bass",      "bass_gain")
    song:connection("bass_gain", "main")
end
