-- City: a 4-bar driving loop in A minor.
--
-- This song is split across multiple files to demonstrate the import +
-- parameter pattern. Each module under examples/city/ returns a single
-- `function(song, opts)` that registers its own phrases and track on the
-- song instance it's handed.

local setup_audio   = require("city.setup")
local register_bass = require("city.bass")
local register_lead = require("city.lead")

song:bpm(128)

setup_audio(song)

register_bass(song, {
    name                = "City Bass",
    target              = "Bass",
    repeats_before_fill = 4,
})

register_lead(song, {
    name   = "City Lead",
    target = "Lead",
})
