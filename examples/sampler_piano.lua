-- Sampler: one sample, pitched across the whole keyboard.
--
-- The most basic sampler form: load a single WAV, declare its root note, and
-- every played note is that same recording pitch-shifted relative to the root.
-- Here "piano_c3.wav" is rooted at c3 and (in the single-file form) mapped
-- across all 128 MIDI notes, so playing higher notes speeds the sample up and
-- lower notes slow it down.
--
-- Note on octaves: this engine uses the RTTTL octave convention where c3 = 72.

song:bpm(96)

-- A single-sample sampler. With no `samples` list, `file`/`root` declare one
-- zone covering the entire range. `root` defaults to "c3" if omitted.
song:sampler(0, {
  file = "samples/piano_c3.wav",
  root = "c3",
}, { label = "Piano" })

-- A little reverb to taste, then an auto-panner, then out.
song:effect(10, "Reverb", { delay = 0.35, feedback = 0.30, mix = 0.35 }, { label = "verb" })
song:effect(11, "Gain",   { level = 0.7 },                               { label = "gain" })

-- Pan is a stereo effect: amount -1 (full left) .. +1 (full right). Here the
-- amount is driven by a sine LFO instead of a constant, so the piano slowly
-- sweeps left↔right once every 4 seconds (freq = 0.25 Hz). Any param can take
-- a modulator table like this — sine / triangle / ramp / square.
song:effect(12, "Pan", {
  amount = { name = "sine", freq = 0.25, low = -1, high = 1 },
}, { label = "autopan" })

song:output(99, { label = "main" })

song:connection("Piano",   "verb")
song:connection("verb",    "gain")
song:connection("gain",    "autopan")
song:connection("autopan", "main")

-- An arpeggiated phrase that wanders well above and below the c3 root so the
-- pitch-shifting is clearly audible. `pitch` accepts either a note-name string
-- like "c3"/"f#2" or a raw MIDI number — here we use names.
song:phrase("Arp", {
  duration = 8.0,
  notes = {
    { pitch = "c3", start = 0.0, duration = 0.9 },  -- root (unshifted)
    { pitch = "e3", start = 1.0, duration = 0.9 },
    { pitch = "g3", start = 2.0, duration = 0.9 },
    { pitch = "c4", start = 3.0, duration = 0.9 },  -- one octave up
    { pitch = "g3", start = 4.0, duration = 0.9 },
    { pitch = "g2", start = 5.0, duration = 0.9 },
    { pitch = "c2", start = 6.0, duration = 0.9 },  -- one octave down
    { pitch = "c3", start = 7.0, duration = 0.9 },
  },
})

song:track({
  targets = { "Piano" },
  initial = "Arp",
  transitions = {
    { from = "Arp", to = "Arp" },
  },
})
