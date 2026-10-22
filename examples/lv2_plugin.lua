-- LV2 plugin hosting (requires building with `--features lv2-host`).
--
-- An LV2 plugin is an `instrument()` or `effect()` whose config table carries
-- an `lv2` key (the path to the .lv2 bundle *directory*):
--   song:instrument(id, { lv2 = "/path/Bundle.lv2", uri = "...", params = {...} })
--   song:effect(id,     { lv2 = "/path/Bundle.lv2", uri = "...", params = {...} })
-- Through `instrument()` it's a note source; through `effect()` an audio
-- processor. Config keys:
--   lv2    = "/path/to/Bundle.lv2"   -- the bundle *directory* (rel to this file)
--   uri    = "https://..."           -- the plugin URI inside the bundle
--                                       (from the bundle's .ttl, NOT a path)
--   label  = "name"                  -- optional, like other sockets
--   params = { ["Name or symbol"] = value | midi_connect(...) | { name="sine", ... } }
--       Initial values for the plugin's *control* ports, addressed by either
--       the port's human name ("Master volume") or its LV2 symbol
--       ("mastervol"), case-insensitive. A value may be a number, a
--       midi_connect() handle for live MIDI-CC automation, or a modulator table.
--
-- Unlike CLAP, an LV2 bundle is a directory containing `manifest.ttl` and the
-- binary; the port layout is read from the Turtle/RDF. Note events are sent to
-- the plugin's atom-sequence MIDI input port as LV2 Atom events.
--
-- As with CLAP, a failed load warns and falls back (instrument -> silence,
-- effect -> pass-through), so a missing/incompatible plugin never crashes the
-- song. Update the path/uri below to a bundle you actually have.

song:bpm(120)

-- ADLplug: an OPL/OPN FM synth (a MIDI instrument). Its plugin URI comes from
-- the bundle's manifest.ttl. "Master volume" (symbol "mastervol", range 0..10)
-- is bound to MIDI CC 7 here.

song:instrument(0, {
  lv2 = "/Users/mkejji/dev/cpp/ADLplug/ADLplug-arm64.lv2",
  uri = "https://github.com/jpcima/ADLplug",
  label = "FM",
  params = {
    ["mastervol"] = midi_connect{ cc = 7, range = { 0, 10 } },
  },
})

-- A little gain trim after it, using a native effect.
song:effect(10, { kind = "Gain", label = "trim", params = { level = 0.6 } })

song:output(99, { label = "main" })

song:connection("FM",   "trim")
song:connection("trim", "main")

song:phrase("Riff", {
  duration = 4.0,
  notes = {
    { pitch = "c3", start = 0.0, duration = 0.5 },
    { pitch = "e3", start = 1.0, duration = 0.5 },
    { pitch = "g3", start = 2.0, duration = 0.5 },
    { pitch = "c4", start = 3.0, duration = 0.5 },
  },
})

song:track({
  targets = { "FM" },
  initial = "Riff"
})
