-- Jam along with your song using a plugged-in instrument.
--
-- A `live` key on `song:instrument(...)` makes it an *audio input* node rather
-- than a note-driven one: it streams whatever your interface picks up straight
-- into the graph. It takes no phrases (there's nothing to sequence — you're the
-- sequencer), but it composes with effects exactly like any other instrument.
--
-- Guitar → distortion → ping-pong delay → reverb → main, over a backing bass.

song:bpm(96)

-- The latency knob. This sets the device callback period for BOTH the output
-- stream and every live input, and is by far the biggest lever on how immediate
-- your playing feels. At 48 kHz, round-trip is roughly 3 periods:
--   512 → ~32 ms (mushy)   128 → ~8 ms (default, tight)   64 → ~4 ms
-- Go as low as your machine allows without dropouts; back off if you hear them.
song:buffer_size(128)

-- The live input. Every field is optional except `live` itself:
--   live     = true, or a device-name substring as a shorthand for `device`
--   device   = substring match on the input device name (default: system input)
--   channels = 1-based input jacks; one = mono (centered), two = stereo L/R
--   gain     = input trim; a number, a modulator, or midi_connect(...)
--   buffer   = jitter ring-buffer size in frames; derived from the device
--              period by default. NOT the latency knob (that's buffer_size
--              above) — only raise it if the console reports overruns.
song:instrument(0, {
	live = true,
	device = "Scarlett",
	channels = { 1 }, -- guitar in jack 1; use { 1, 2 } for a stereo source
	gain = 0.9,
	-- Ride the input level from a pedal or fader:
	-- gain = midi_connect({ cc = 7, initial = 0.9, range = { 0, 2 } }),
}, { label = "Bass" })

song:effect(10, { kind = "Tremolo", label = "trem", params = { rate = 6.0, depth = 1 } })
song:output(99, { label = "main" })

song:connection("Bass", "trem")
song:connection("trem", "main")

-- A simple i-VI-III-VII vamp in A minor for the backing bass.
song:phrase("Vamp", {
	duration = 16.0,
	notes = {
		{ pitch = "a1", start = 0.0, duration = 3.6 },
		{ pitch = "f1", start = 4.0, duration = 3.6 },
		{ pitch = "c2", start = 8.0, duration = 3.6 },
		{ pitch = "g1", start = 12.0, duration = 3.6 },
	},
})

-- song:track({
--   targets = { "Bass" },
--   initial = "Vamp",
-- })

-- The live input needs no track at all — it plays as soon as it's connected.
-- Declaring one with `mode = "live"` is optional, and just makes the intent
-- explicit in the song file. Pairing it with `initial`/`transitions` is an
-- error, since a live input has no phrases.
song:track({
	targets = { "Bass" },
	mode = "live",
})
