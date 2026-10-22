-- Play an aurio instrument from an external MIDI keyboard.
--
-- `song:midi_in(...)` opens a hardware port and forwards its note on/off to
-- whatever it's connected to. Unlike `live = true`, no audio comes off the
-- cable — only notes — so the sound is aurio's, played by your hands.
--
-- A midi_in has no phrases, so it is never a track target; it feeds an
-- instrument through a normal connection instead.
--
--   port      substring of the MIDI port name, case-insensitive.
--             Omit to take the first available input.
--   channel   1..16. Omit to listen on every channel.
--   transpose semitones added to each incoming pitch, for playing an
--             instrument outside the keyboard's own range.

song:bpm(120)

-- The keyboard, on Port 1 of the interface.
song:midi_in(0, { port = "Port 1", channel = 1, label = "keys" })

-- The voice it plays.
song:instrument(1, {
	drawbars = {
		8,
		8,
		8,
		0,
		0,
		0,
		0,
		0,
		{ name = "triangle", freq = 0.1, low = 0, high = 6 },
	},
	gain = 0.9,
	click = 0.4,
	percussion = { harmonic = 3, decay = "fast", volume = "normal" },
	vibrato = "C3",
	drive = 0.35,
	leakage = 0.3,
	wobble = 0.4,
}, { label = "lead" })

song:effect(2, { kind = "Reverb", label = "verb", params = { delay = 0.35, feedback = 0.4, mix = 0.5 } })
song:effect(3, { kind = "Gain", label = "trim", params = { level = 0.4 } })

song:output(10, { label = "main" })

-- Notes flow keys -> lead; audio flows lead -> verb -> trim -> main.
song:connection("keys", "lead")
song:connection("lead", "verb")
song:connection("verb", "trim")
song:connection("trim", "main")
