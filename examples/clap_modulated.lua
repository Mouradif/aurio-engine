-- Modulating a CLAP plugin's parameter with an LFO (requires `--features
-- clap-host`). This is the CLAP counterpart to examples/lv2_modulated.lua —
-- the exact same modulator-table syntax drives a plugin param once per block,
-- whether the plugin is LV2 or CLAP.
--
-- Here a sine LFO sweeps Surge XT's filter cutoff for a classic filter-wobble.
--
-- IMPORTANT — find the real param name first:
--   CLAP plugins expose their parameters by name, and Surge has *hundreds*.
--   Run once with the dump enabled to list them, then copy the exact cutoff
--   name into `params` below:
--
--     AURIO_DUMP_PARAMS=1 cargo run --features clap-host -- examples/clap_modulated.lua 2>&1 | grep -i cutoff
--
--   Surge's cutoff is typically named like "A Filter 1 Cutoff" / "Filter 1
--   Cutoff" depending on version. If the name below doesn't match, Aurio prints
--   "[plugin] socket 0: ... has no param named '...'" and you'll see the right
--   one in the dump. Param values are the plugin's own plain values, so set the
--   modulator low/high to span the range the dump shows for that param.

song:bpm(120)

-- Surge XT as a CLAP instrument, with its scene volume swept by a sine LFO for
-- a tremolo. The same modulator-table syntax works for any plugin param (and
-- for LV2 — see lv2_modulated.lua); we modulate "A Volume" here because it's
-- unambiguously audible. (Modulating, say, a filter cutoff also works, but in
-- Surge's default patch the filter routing/type must be set up first for the
-- cutoff to matter — volume needs no such setup.)
--
-- Run with AURIO_DUMP_PARAMS=1 to list Surge's parameter names + ranges.
song:instrument(0, {
  clap = "/Library/Audio/Plug-Ins/CLAP/Surge XT.clap",
  id = "org.surge-synth-team.surge-xt",
  label = "Surge",
  params = {
    -- A 9 Hz sine swinging the scene volume (one way to get tremolo).
    ["A Volume"] = { name = "sine", freq = 9.0, low = 0.4, high = 0.9 },
  },
})

song:output(99, { label = "main" })

song:connection("Surge", "main")

-- Hold longer notes so the filter sweep is audible across each note.
song:phrase("Riff", {
  duration = 4.0,
  notes = {
    { pitch = "c2", start = 0.0, duration = 2.0 },
    { pitch = "g2", start = 2.0, duration = 2.0 },
  },
})

song:track({
  targets = { "Surge" },
  initial = "Riff",
})
