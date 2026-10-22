-- Sampler: a drum kit built from one sample per key.
--
-- Each drum is a separate WAV assigned to its own root note, with the key
-- range defaulting to just that one note (low/high omitted). Playing c3, d3,
-- e3 then triggers hi-hat, kick, snare respectively — so a "drum pattern" is
-- just a phrase of those three pitches.
--
-- WAV paths resolve relative to this song file's directory, exactly like
-- `require` does for Lua modules — so these live in ./samples/.
--
-- Octave convention: c3 = 72 (RTTTL-style).

song:bpm(100)

-- One sampler, three zones. No low/high → each maps to exactly its root.
song:sampler(0, {
	samples = {
		{ file = "samples/hh.wav", root = "c3" }, -- 72
		{ file = "samples/kick.wav", root = "d3" }, -- 74
		{ file = "samples/snare.wav", root = "e3" }, -- 76
	},
}, { label = "Drums" })

song:effect(10, { kind = "Gain", label = "gain", params = { level = 0.8 } })
song:output(99, { label = "main" })

song:connection("Drums", "gain")
song:connection("gain", "main")

-- A one-bar (4-beat) backbeat groove, looped.
--   kick (d3=74) on beats 1 and 3
--   snare (e3=76) on beats 2 and 4
--   hat (c3=72) on every 8th note
local hh, kick, snare = 72, 74, 76
local pattern = {}

-- Hats: offbeat-inclusive 8th notes.
for i = 0, 7 do
	local hh_velocity = 30
	if i % 2 == 0 then
		hh_velocity = 55
	end
	table.insert(pattern, { pitch = hh, start = i * 0.5, duration = 0.1, velocity = hh_velocity })
	if i % 4 == 0 then
		table.insert(pattern, { pitch = kick, start = i * 0.5, duration = 0.1 })
	else
		if i % 2 == 0 then
			table.insert(pattern, { pitch = snare, start = i * 0.5, duration = 0.1 })
		end
	end
end

local beat = {
	{
		{ pitch = kick, start = 0.0, duration = 0.1 },
		{ pitch = kick, start = 1.5, duration = 0.1 },
		{ pitch = snare, start = 1.5, duration = 0.1 },
	},
}

song:phrase("Groove", { duration = 2.0, notes = beat })

song:track({
	targets = { "Drums" },
	initial = "Groove",
	transitions = {
		{ from = "Groove", to = "Groove" },
	},
})
