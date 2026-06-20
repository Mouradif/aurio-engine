-- LV2 *effect* on a built-in synth (requires `--features lv2-host`).
--
-- This demonstrates that any node type composes through Aurio's graph: a native
-- built-in synth feeds its audio into an LV2 effect plugin, which processes it
-- and passes it to the output. (A CLAP synth → LV2 effect chain works exactly
-- the same way — just swap the instrument node.)
--
-- The plugin here is x42's "No Delay Line" (nodelay.lv2), a simple mono delay.
-- Build it for arm64 with:
--   cd ~/dev/cpp/nodelay.lv2 && clang -std=c99 -O3 -fPIC -arch arm64 \
--     -DHAVE_LV2_1_18_6 -I ~/dev/c/lv2/include -o build/nodelay.dylib \
--     nodelay.c -shared -dynamiclib -lm
-- (plus the generated build/manifest.ttl and build/nodelay.ttl).

song:bpm(120)

-- A plain built-in synth — no plugins involved in producing the sound.
song:instrument(0, {
  adsr = { 0.005, 0.1, 0.4, 0.2 },
  oscillators = {
    { wave = "Saw",      gain = 0.4 },
    { wave = "Triangle", gain = 0.3 },
  },
}, { label = "Synth" })

-- An LV2 effect: nodelay processes the synth's audio. `instrument = false`
-- makes it a Processor node (audio in -> audio out). Its "delay" control port
-- (in samples) is set here; "report_latency" picks the operating mode.
song:lv2(1, "/Users/mkejji/dev/cpp/nodelay.lv2/build", {
  uri = "http://gareus.org/oss/lv2/nodelay",
  instrument = false,
  params = {
    ["delay"] = 12000,   -- ~0.25s at 48kHz, audibly delayed
  },
}, { label = "Delay" })

song:output(99, { label = "main" })

song:connection("Synth", "main")
song:connection("Synth", "Delay")
song:connection("Delay", "main")

song:phrase("Riff", {
  duration = 4.0,
  notes = {
    { pitch = "c3", start = 0.0, duration = 0.4 },
    { pitch = "e3", start = 1.0, duration = 0.4 },
    { pitch = "g3", start = 2.0, duration = 0.4 },
    { pitch = "c4", start = 3.0, duration = 0.4 },
  },
})

song:track({
  targets = { "Synth" },
  initial = "Riff",
  transitions = { { from = "Riff", to = "Riff" } },
})
