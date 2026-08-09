-- Changing tempo while the song plays.
--
-- `song:bpm(n)` sets the tempo and `song:bpm()` reads it. The same call works
-- while the song is being declared and while it plays, so a phrase hook or a
-- `midi_on` handler can change tempo mid-song.
--
-- When a change takes effect: at the **next phrase boundary**. Notes already
-- scheduled keep the tempo they were scheduled at, so a tempo change never
-- retimes notes that are already on their way to the audio thread. With long
-- phrases, expect to wait for the current one to finish.
--
-- Range is 20..400 BPM; anything outside is clamped with a warning.
--
-- Not affected by a live tempo change: an effect declared with a beat-relative
-- delay time (`time = { beats = 1 }`) converts to milliseconds once, when the
-- graph is built. It keeps the tempo the song started at.

song:bpm(100)

song:instrument(0, {
  adsr = { 0.01, 0.08, 0.6, 0.15 },
  oscillators = {
    { wave = "Triangle", gain = 0.5 },
    { wave = "Sine",     gain = 0.3 },
  },
})

song:effect(1, { kind = "Gain", label = "trim", params = { level = 0.3 } })
song:output(10, { label = "main" })
song:connection(0, "trim")
song:connection("trim", "main")

-- Each loop of this phrase nudges the tempo up, then snaps back at the top.
-- The hook runs on the timing thread, between phrases — exactly where a tempo
-- change lands cleanly.
loops = 0
song:phrase("Riff", {
  duration = 2.0,
  on_enter = function()
    print(string.format("[tempo] starting at %.0f BPM", song:bpm()))
  end,
  on_loop = function()
    loops = loops + 1
    local next_bpm = song:bpm() + 10
    if next_bpm > 180 then
      next_bpm = 100
      print("[tempo] wrapping back to 100")
    end
    song:bpm(next_bpm)
    print(string.format("[tempo] loop %d -> %.0f BPM", loops, song:bpm()))
  end,
  notes = {
    { pitch = 62, start = 0.0, duration = 0.4 },
    { pitch = 65, start = 0.5, duration = 0.4 },
    { pitch = 69, start = 1.0, duration = 0.4 },
    { pitch = 74, start = 1.5, duration = 0.4 },
  },
})

song:track({ targets = { 0 }, initial = "Riff" })

-- A knob can drive it too. Uncomment with a controller on Port 2:
--
-- song:midi_in(2, { port = "Port 2", label = "keys" })
-- midi_on("keys", { event = "cc", cc = 1 }, function(m)
--   -- Mod wheel 0..127 across a musical range.
--   song:bpm(60 + (m.value / 127) * 120)
-- end)
