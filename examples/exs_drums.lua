-- Sampler: a drum kit from an EXS24 library.
--
-- A drum EXS maps one recording per key and turns pitch tracking off, so
-- every key is a different drum played at its recorded pitch. Logic's kits
-- follow the General MIDI drum map, so the note numbers below work across
-- most of them; swap `library` for another kit and the groove carries over.
--
-- To see what a kit puts on each key:
--
--   cargo run --example exs_info -- "/path/to/Kit.exs"

song:bpm(98)

song:sampler(0, {
	library = "/Library/Application Support/Logic/Sampler Instruments/03 Drums & Percussion/02 Electronic Drum Kits/Cory's LinnDrum Kit.exs",
}, { label = "Kit" })

song:effect(10, { kind = "Reverb", label = "room", params = { delay = 0.12, feedback = 0.2, mix = 0.12 } })
song:effect(11, { kind = "Gain", label = "gain", params = { level = 0.8 } })
song:output(99, { label = "main" })

song:connection("Kit", "room")
song:connection("room", "gain")
song:connection("gain", "main")

-- General MIDI drum map.
local kick = 36
local snare = 38
local clap = 39
local hat = 42
local open_hat = 46

-- One bar of 16ths: kick on 1 and the "and" of 3, snare + clap on 2 and 4,
-- hats on every 8th with ghosted offbeats, an open hat to lead into the loop.
local notes = {}
local function hit(pitch, step, velocity)
	table.insert(notes, { pitch = pitch, start = step * 0.25, duration = 0.2, velocity = velocity })
end

for step = 0, 15 do
	if step % 2 == 0 and step ~= 14 then
		hit(hat, step, step % 4 == 0 and 90 or 55)
	end
end
hit(open_hat, 14, 80)

hit(kick, 0, 120)
hit(kick, 6, 95)
hit(kick, 10, 110)

hit(snare, 4, 115)
hit(clap, 4, 90)
hit(snare, 12, 115)
hit(clap, 12, 90)
hit(snare, 15, 40) -- ghost note into the next bar

song:phrase("Groove", { duration = 4.0, notes = notes })

song:track({
	targets = { "Kit" },
	initial = "Groove",
})
