-- Play a Kaossilator Pro+ from a MINILAB3 keyboard.
--
-- The Kaossilator has no note input: its sound comes from the touchpad, so a
-- keyboard can only drive it by translating notes into pad Control Change.
-- That translation is written here in Lua rather than built into aurio —
-- `midi_on` registers a live listener on a `midi_in`, and `midi_emit` sends
-- through a `midi_out`. Any device pair can be bridged this way.
--
--   midi_on(source, filter, handler)
--     source   a midi_in label or socket id
--     filter   { event = "note_on" | "note_off" | "cc",
--                channel = 1..16, cc = 0..127, note = 0..127 }
--              every field but `event` is optional and matched exactly
--     handler  called with one table: { note, velocity, cc, value, channel }
--
--   midi_emit(target, message)
--     target   a midi_out label or socket id
--     message  { cc = N, value = V } or { note = N, velocity = V, on = bool }
--              `channel` (1..16) overrides the node's channel
--
-- Handlers run on the MIDI thread that owns the source port, never the audio
-- thread: a slow handler delays MIDI on that port but can't cause a dropout.
-- Lua globals persist across calls, so a handler can keep state (see the
-- note counter below).

song:bpm(120)

-- The keyboard. No connection to an instrument: it exists purely as an event
-- source for the listeners below.
song:midi_in(0, { port = "Port 2", label = "keys" })

-- The Kaossilator. Channel 2 — the unit ships listening there, and a
-- wrong-channel message is discarded silently, which looks exactly like a
-- dead cable. Suspect the channel before the CC numbers.
song:midi_out(1, { port = "Port 3", channel = 2, label = "kaoss" })

-- Pad CC map, confirmed by echoing the device's own output back at it.
local CC_X    = 12
local CC_Y    = 13
local CC_GATE = 92

local Y_VAL = 0

-- Note on: set the pad position, then raise the gate.
--
-- Order matters. A real finger establishes X/Y *before* the gate rises, and
-- the hardware expects the same: gate-first can sound the previous position
-- for a moment. X is sent twice to match what the pad itself transmits.
midi_on("keys", { event = "note_on" }, function(m)
  actual_note = (m.note - 48) * 6
  midi_emit("kaoss", { cc = CC_X, value = actual_note })
  midi_emit("kaoss", { cc = CC_Y, value = Y_VAL })
  midi_emit("kaoss", { cc = CC_GATE, value = 127 })
end)

-- Note off: drop the gate. Position is left alone — the next note sets it.
midi_on("keys", { event = "note_off" }, function(m)
  midi_emit("kaoss", { cc = CC_GATE, value = 0 })
end)

-- Mod wheel (CC 1) sweeps the pad's Y axis.
midi_on("keys", { event = "cc", cc = 1 }, function(m)
  Y_VAL = m.value
  midi_emit("kaoss", { cc = CC_Y, value = Y_VAL })
end)
