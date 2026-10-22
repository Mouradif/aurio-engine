-- CLAP plugin hosting (requires building with `--features clap-host`).
--
-- A CLAP plugin is just an `instrument()` or `effect()` whose config table
-- carries a `clap` key (the path to the .clap bundle):
--   song:instrument(id, { clap = "/path/Foo.clap", id = "...", params = {...} })
--   song:effect(id,     { clap = "/path/Foo.clap", id = "...", params = {...} })
-- Calling it through `instrument()` makes it a note source; through `effect()`
-- an audio processor. Config keys:
--   clap   = "/path/to/Foo.clap"   -- the bundle path (relative to this file)
--   id     = "com.vendor.plugin"   -- the plugin id *inside* the bundle
--   label  = "name"                -- optional, like other sockets
--   params = { ["Param Name"] = value | midi_connect(...) | { name="sine", ... } }
--       Initial parameter values, addressed by the plugin's own parameter
--       names (case-insensitive). A value may be a number, a midi_connect()
--       handle to drive it live from a MIDI CC, or a modulator table.
--
-- If a plugin fails to load (missing file, wrong arch, unknown id), Aurio warns
-- and keeps playing: an instrument plugin becomes silence, an effect plugin a
-- transparent pass-through. So a missing plugin never takes the whole song down.
--
-- Paths are resolved relative to this song file's directory (or pass an absolute
-- path). Update the paths/ids below to plugins you actually have installed.

song:bpm(120)

-- A CLAP *instrument*: notes routed to it (via a track) drive the plugin, and
-- its stereo output flows downstream.
--
-- To bind a param, you need its exact name as the plugin reports it.
-- `params = { ["Exact Name"] = midi_connect{ cc = 74, range = { lo, hi } } }`
-- entry below. (Param values are the plugin's plain values, so pick the range
-- to match what the dump shows.)
song:instrument(0, {
  clap = "/Library/Audio/Plug-Ins/CLAP/Surge XT.clap",
  id = "org.surge-synth-team.surge-xt",
  label = "Synth",
})

-- A CLAP *effect*: it processes the audio passing through it.
song:effect(1, {
  clap = "/Library/Audio/Plug-Ins/CLAP/Surge XT Effects.clap",
  id = "org.surge-synth-team.surge-xt-fx",
  label = "FX",
})

song:output(99, { label = "main" })

song:connection("Synth", "FX")
song:connection("FX",    "main")

-- Drive the plugin instrument with a simple phrase.
song:phrase("Riff", {
  duration = 4.0,
  notes = {
    { pitch = "c3", start = 0.0, duration = 0.5 },
    { pitch = "e3", start = 1.0, duration = 0.5 },
    { pitch = "g3", start = 2.0, duration = 0.5 },
    { pitch = "b3", start = 3.0, duration = 0.5 },
  },
})

song:track({
  targets = { "Synth" },
  initial = "Riff",
  transitions = { { from = "Riff", to = "Riff" } },
})
