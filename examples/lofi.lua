-- Lo-fi chill — a multi-section beat that wanders between Main and Alt,
-- with a rare random Surprise section that occasionally appears and
-- returns straight back to Main.
--
-- Section flow:
--     Intro (once) → Main → ... → Alt → Main → ... → Alt → ...
--                            ↳ ~20% per loop, detour through Surprise → Main
--
-- The chord track is the *conductor*: it owns the section counter and the
-- random roll, and writes its decision to a shared `section_state` global.
-- The other tracks are *followers* — their transitions just match whatever
-- section the conductor moved to. This keeps every track in lockstep even
-- though they each have an independent state machine.
--
-- Registration order is load-bearing: the conductor MUST be registered
-- first so its on_enter runs before any follower's transition condition
-- reads section_state in the same timing-loop iteration.

local setup_audio     = require("lofi.setup")
local register_chords = require("lofi.chords")    -- conductor (must be first)
local register_drums  = require("lofi.drums")
local register_bass   = require("lofi.bass")
local register_lead   = require("lofi.lead")

song:bpm(78)

setup_audio(song)

register_chords(song)
register_drums(song)
register_bass(song)
register_lead(song)
