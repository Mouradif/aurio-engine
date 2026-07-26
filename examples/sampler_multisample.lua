-- Sampler: a multisampled instrument with key zones.
--
-- When you have separate recordings of an instrument at different pitches, you
-- map each one over a key range so any given note is only pitched a little
-- (within ±a few semitones of the nearest recorded root) instead of stretched
-- across octaves. This keeps the timbre natural.
--
--   piano_c3.wav  rooted at c3, covering c#2 .. c4
--   piano_c5.wav  rooted at c5, covering c#4 .. c6
--
-- A played note picks the first zone whose range contains it, then pitches
-- that zone's sample relative to its root. The two ranges meet seamlessly:
-- c4 is the top of the lower zone, c#4 the bottom of the upper one. (The
-- recordings are c3 and c5 — a real C4 wasn't available — so each zone covers
-- a wider span and pitches a bit further, but the principle is the same.)
--
-- Octave convention: c3 = 72, c4 = 84, c5 = 96 (RTTTL-style).

song:bpm(88)

song:sampler(0, {
  samples = {
    -- low zone: c3 recording, covering c#2 .. c4.
    { file = "samples/piano_c3.wav", root = "c3", low = "c#2", high = "c4" },
    -- high zone: c5 recording, covering c#4 .. c6.
    { file = "samples/piano_c5.wav", root = "c5", low = "c#4", high = "c6" },
  },
}, { label = "Piano" })

song:effect(10, { kind = "Reverb", label = "verb", params = { delay = 0.40, feedback = 0.35, mix = 0.40 } })
song:effect(11, { kind = "Gain", label = "gain", params = { level = 0.7 } })
song:output(99, { label = "main" })

song:connection("Piano", "verb")
song:connection("verb",  "gain")
song:connection("gain",  "main")

-- A phrase that walks up through both zones, crossing the c4/c#4 boundary so
-- you can hear the sample switch happen mid-line. Pitches span c#2 (61) up to
-- c6 (108).
song:phrase("Walk", {
  duration = 8.0,
  notes = {
    { pitch = 61, start = 0.0, duration = 0.8 },  -- c#2  (low zone, bottom)
    { pitch = 72, start = 1.0, duration = 0.8 },  -- c3   (low zone root)
    { pitch = 84, start = 2.0, duration = 0.8 },  -- c4   (low zone, top)
    { pitch = 85, start = 3.0, duration = 0.8 },  -- c#4  (high zone, bottom)
    { pitch = 96, start = 4.0, duration = 0.8 },  -- c5   (high zone root)
    { pitch = 108, start = 5.0, duration = 0.8 }, -- c6   (high zone, top)
    { pitch = 96, start = 6.0, duration = 0.8 },  -- c5
    { pitch = 72, start = 7.0, duration = 0.8 },  -- c3
  },
})

song:track({
  targets = { "Piano" },
  initial = "Walk",
  transitions = {
    { from = "Walk", to = "Walk" },
  },
})
