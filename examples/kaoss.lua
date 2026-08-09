-- Proof of concept: driving a Kaossilator Pro+ from aurio.
--
-- The Kaossilator is not a keyboard. Note on/off does nothing to it — its
-- sound comes from the touchpad, and MIDI's job is to simulate a finger.
-- So it is driven by Control Change rather than by phrases, and it is *not*
-- a track target.
--
-- The pad only sounds while the touch gate is held, so CC 92 has to be on
-- for anything at all to come out. Here a slow square wave taps the pad on
-- and off while X and Y sweep underneath it — audible proof the whole chain
-- works, and a decent drone in its own right.
--
-- CC map, confirmed by echoing the unit's own output back at it:
--   12  pad X            92  touch on/off (the gate)
--   13  pad Y            94  volume
--   90  gate arp on/off  91  gate arp speed
--   93  gate arp time
--
-- If nothing sounds, suspect the channel before the CC numbers — a mismatched
-- channel is discarded silently and is indistinguishable from a wrong map.
-- `cargo run --example kaoss_echo` prints what the pad actually transmits.
--
-- A control's value is a modulator table (swept) or a bare number (parked):
--   name  sine | triangle | ramp | square
--   freq  cycles per second — 0.2 is one sweep every five seconds
--   low   / high   the 0..127 range the sweep covers
--
-- Nothing here connects to an output: the audio comes out of the
-- Kaossilator's own jacks, not through aurio.

song:bpm(120)

song:midi_out(0, {
  port = "Port 3",
  -- The Kaossilator ships listening on channel 2, not 1. Sending to the
  -- wrong channel is silently ignored and looks exactly like a bad CC map.
  channel = 2,
  label = "kaoss",
  controls = {
    -- Volume, parked. Set this before anything else so the first sweep
    -- doesn't arrive at whatever level the last session left behind.
    [94] = 100,

    -- The touch gate: on for half a cycle, off for the other half. At 0.5 Hz
    -- that's a one-second note every two seconds.
    [92] = { name = "square", freq = 0.5, low = 0, high = 127 },

    -- Pad position. X moves faster than Y, so the pair traces a slow figure
    -- across the pad instead of a straight diagonal.
    [12] = { name = "sine",     freq = 0.2,  low = 10, high = 118 },
    [13] = { name = "triangle", freq = 0.07, low = 30, high = 100 },
  },
})

-- Uncomment to let the gate arpeggiator chop the drone instead of the
-- square-wave gate above (set [92] = 127 to hold the pad down continuously).
-- song:midi_out(1, {
--   port = "Port 2",
--   channel = 1,
--   label = "kaoss_arp",
--   controls = {
--     [92] = 127,                                                   -- pad held
--     [90] = 127,                                                   -- arp on
--     [91] = { name = "ramp", freq = 0.05, low = 40, high = 110 },  -- arp speed
--     [93] = 64,                                                    -- arp time
--   },
-- })
