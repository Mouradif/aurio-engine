-- CLAP plugin hosting (requires building with `--features clap-host`).
--
-- `song:plugin(id, "/path/to/Foo.clap", opts)` loads a CLAP bundle and uses it
-- as a graph node. opts:
--   id          = "com.vendor.plugin"  -- the plugin id *inside* the bundle
--   instrument  = true | false         -- true: a note source; false: an audio
--                                          effect (default true)
--   label       = "name"               -- optional, like other sockets
--   params      = { ["Param Name"] = value | midi_connect(...) }
--       Initial parameter values, addressed by the plugin's own parameter
--       names (case-insensitive). A value may be a number, or a midi_connect()
--       handle to drive that param live from a MIDI CC.
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
-- To bind a param, you need its exact name as the plugin reports it. Run once
-- with `AURIO_DUMP_PARAMS=1` to print every param name + range, then add a
-- `params = { ["Exact Name"] = midi_connect{ cc = 74, range = { lo, hi } } }`
-- entry below. (Param values are the plugin's plain values, so pick the range
-- to match what the dump shows.)
song:plugin(0, "/Library/Audio/Plug-Ins/CLAP/Surge XT.clap", {
  id = "org.surge-synth-team.surge-xt",
  instrument = true,
}, { label = "Synth" })

-- A CLAP *effect*: it processes the audio passing through it.
song:plugin(1, "/Library/Audio/Plug-Ins/CLAP/Surge XT Effects.clap", {
  id = "org.surge-synth-team.surge-xt-fx",
  instrument = false,
}, { label = "FX" })

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
