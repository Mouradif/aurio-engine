-- Following an external MIDI clock.
--
--   song:bpm({ midi_source = "clock" })
--
-- takes the tempo from MIDI clock arriving on a `song:midi_in`. The source
-- must be an *input* — clock is received, and a `midi_out` only sends. Naming
-- an output is caught when the song loads.
--
-- How it tracks:
--   * MIDI clock is 24 pulses per quarter note. Aurio averages a whole
--     quarter note between estimates rather than timing pulse-to-pulse, which
--     divides the transport's jitter by 24.
--   * A new tempo is applied to the **next phrase to be scheduled**. Whatever
--     is already playing finishes at the tempo it started at, so a change is
--     never audible as a cut or a retrigger — the cost is that it takes effect
--     one phrase later. Keep phrases short if you want it to track closely.
--   * That threshold is the debounce: a knob sweeping the clock continuously
--     won't reschedule on every tick, only when the tempo has actually moved.
--   * Start/stop (0xFA/0xFC) resets the estimator, so the gap while stopped
--     isn't measured as one enormous beat.
--
-- Nothing already scheduled is ever disturbed, so following a clock can't
-- glitch the audio — it just means the tempo lags by up to one phrase.

-- The starting tempo, used until the first clock estimate arrives (a quarter
-- note after the transport starts).

-- The port carrying clock. Nothing else about it needs declaring: it needs no
-- connection and no listener, because the clock path is built in.
song:midi_in(0, { port = "Port 3", label = "clock" })

-- Follow it.
song:bpm({ midi_source = "clock" })

song:instrument(1, {
  adsr = { 0.005, 0.05, 0.4, 0.1 },
  oscillators = {
    { wave = "Square",   gain = 0.25 },
    { wave = "Triangle", gain = 0.35 },
  },
}, { label = "pulse" })

song:effect(2, { kind = "Gain", label = "trim", params = { level = 0.35 } })
song:output(10, { label = "main" })
song:connection("pulse", "trim")
song:connection("trim", "main")

-- A steady eighth-note pattern makes tempo changes obvious by ear. The phrase
-- is deliberately short (2 beats) so a tempo change is picked up quickly:
-- with a 16-beat phrase the same change would take many seconds to be heard.
song:phrase("Pulse", {
  duration = 2.0,
  on_loop = function()
    print(string.format("[clock] following at %.1f BPM", song:bpm()))
  end,
  notes = {
    { pitch = 72, start = 0.0, duration = 0.2 },
    { pitch = 60, start = 0.5, duration = 0.2 },
    { pitch = 67, start = 1.0, duration = 0.2 },
    { pitch = 60, start = 1.5, duration = 0.2 },
  },
})

song:track({ targets = { "pulse" }, initial = "Pulse" })
