-- Lead melody. Sparse, floating phrases that follow section_state.
local make_follower = require("lofi.follower")

return function(song)
  song:phrase("Lead Intro", { duration = 8.0, notes = {} })  -- silent

  -- Main: a sparse 4-bar melody — bar 3 is intentionally silent so the
  -- bass and chords can breathe.
  song:phrase("Lead Main", {
    duration = 16.0,
    notes = {
      { pitch = 79, start = 1.5,  duration = 1.0 },   -- G5
      { pitch = 75, start = 4.0,  duration = 2.5 },   -- Eb5
      -- bar 3 silent
      { pitch = 70, start = 12.0, duration = 1.0 },   -- Bb4
      { pitch = 72, start = 13.0, duration = 1.0 },   -- C5
      { pitch = 74, start = 14.0, duration = 2.0 },   -- D5
    },
  })

  -- Alt: descending melodic shape.
  song:phrase("Lead Alt", {
    duration = 16.0,
    notes = {
      { pitch = 84, start = 0.0,  duration = 2.0 },   -- C6
      { pitch = 82, start = 4.0,  duration = 1.0 },   -- Bb5
      { pitch = 80, start = 5.0,  duration = 1.0 },   -- Ab5
      { pitch = 79, start = 8.0,  duration = 2.0 },   -- G5
      { pitch = 75, start = 12.0, duration = 4.0 },   -- Eb5
    },
  })

  -- Surprise: a single held G5 — suspended in the reverb tail.
  song:phrase("Lead Surprise", {
    duration = 8.0,
    notes = {
      { pitch = 79, start = 0, duration = 7.0 },
    },
  })

  make_follower(song, {
    target = "Lead",
    phrases = {
      intro    = "Lead Intro",
      main     = "Lead Main",
      alt      = "Lead Alt",
      surprise = "Lead Surprise",
    },
  })
end
