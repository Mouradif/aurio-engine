-- Modulating an LV2 effect's parameter (requires `--features lv2-host`).
--
-- A built-in synth feeds x42's tinygain (a simple gain control) whose "gain"
-- control port is driven by a sine LFO modulator. Sweeping the gain in dB makes
-- the volume pulse -> a tremolo, produced entirely by automating the plugin's
-- parameter from Aurio.
--
-- The same binding accepts a midi_connect(...) handle instead of a modulator,
-- to drive the gain live from a MIDI CC knob.
--
-- Plugin: x42 tinygain (#mono). Built for arm64 with:
--   clang -std=c99 -O3 -arch arm64 -I build/compat-include \
--     -I ~/dev/c/lv2/include -o build/tinygain.dylib src/tinygain.c \
--     -shared -dynamiclib -lm
-- (plus the generated build/manifest.ttl and build/tinygain.ttl).

song:bpm(120)

song:instrument(0, {
  adsr = { 0.005, 0.1, 0.6, 0.2 },
  oscillators = {
    { wave = "Saw",      gain = 0.4 },
    { wave = "Square",   gain = 0.2 },
  },
}, { label = "Synth" })

-- tinygain as an effect. Its "gain" port (dB, -20..+20) is modulated by a 5 Hz
-- sine swinging between -18 dB (near silent) and +6 dB (a touch boosted) -> a
-- pronounced tremolo. Swap the modulator for `midi_connect{ cc = 7, ... }` to
-- ride the gain from a controller instead.
song:lv2(1, "/Users/mkejji/dev/cpp/tinyamp.lv2/build", {
  uri = "http://gareus.org/oss/lv2/tinygain#mono",
  instrument = false,
  params = {
    ["gain"] = { name = "sine", freq = 5.0, low = -18.0, high = 6.0 },
  },
}, { label = "Trem" })

song:output(99, { label = "main" })

song:connection("Synth", "Trem")
song:connection("Trem",  "main")

song:phrase("Riff", {
  duration = 4.0,
  notes = {
    { pitch = "c3", start = 0.0, duration = 1.0 },
    { pitch = "e3", start = 1.0, duration = 1.0 },
    { pitch = "g3", start = 2.0, duration = 1.0 },
    { pitch = "c4", start = 3.0, duration = 1.0 },
  },
})

song:track({
  targets = { "Synth" },
  initial = "Riff",
})
