-- Sampler: loading an EXS24 instrument.
--
-- Logic and GarageBand ship their sampled instruments as `.exs` files. An EXS
-- is a map, not audio: it describes zones (a key range, a velocity range, a
-- root note) and points at the recordings they play. Handing one to the
-- sampler pulls in the whole multisample in a line, instead of writing out a
-- `samples` list by hand.
--
--   song:sampler(0, { library = "/path/to/Instrument.exs" }, { label = "..." })
--
-- Two layouts appear in Apple's factory content, and both work here:
--
--   * one file per recording, a directory of AIFFs next to the instrument;
--   * one consolidated CAF holding every recording end to end, with each zone
--     naming the frame range it owns. These run to tens of megabytes, so the
--     loader decodes each file once and shares it between zones.
--
-- Sample paths inside an EXS point at wherever the library was authored. That
-- is right for factory content and wrong for anything copied between
-- machines, so the loader also looks beside the instrument before giving up.
-- A recording it cannot find is skipped with a warning and the rest still
-- plays.
--
-- Velocity matters more here than with a hand-built multisample: these
-- libraries record each note several times at different playing strengths and
-- pick between them by how hard the key is struck. The phrase below leans on
-- that, so vary `velocity` to hear a different recording rather than the same
-- one turned up.
--
-- Octave convention: c3 = 72, c4 = 84 (RTTTL-style).

song:bpm(96)

-- Point this at any .exs on your system. This one is Logic factory content
-- backed by a single consolidated CAF.
song:sampler(0, {
	library = "/Library/Application Support/Logic/Sampler Instruments/08 Guitars/Acoustic Guitar.exs",
}, { label = "Guitar" })

song:effect(10, { kind = "Reverb", label = "verb", params = { delay = 0.28, feedback = 0.30, mix = 0.25 } })
song:effect(11, { kind = "Gain", label = "gain", params = { level = 0.8 } })
song:output(99, { label = "main" })

song:connection("Guitar", "verb")
song:connection("verb", "gain")
song:connection("gain", "main")

-- An arpeggiated chord, played soft then hard so the velocity layers switch.
song:phrase("Picked", {
	duration = 2.0,
	notes = {
		{ pitch = 23, start = 0.0, duration = 1.8, velocity = 45 },
		{ pitch = 30, start = 0.04, duration = 1.8, velocity = 56 },
		{ pitch = 35, start = 0.08, duration = 1.8, velocity = 64 },
		{ pitch = 38, start = 0.12, duration = 1.8, velocity = 73 },

		{ pitch = 30, start = 1.0, duration = 1.8, velocity = 45 },
		{ pitch = 37, start = 1.01, duration = 1.8, velocity = 56 },
		{ pitch = 42, start = 1.02, duration = 1.8, velocity = 64 },
		{ pitch = 45, start = 1.03, duration = 1.8, velocity = 72 },
	},
})

song:track({
	targets = { "Guitar" },
	initial = "Picked",
})
