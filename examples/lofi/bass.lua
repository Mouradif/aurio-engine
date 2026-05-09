-- Bass track. Held roots, one note per bar, follows section_state.
local make_follower = require("lofi.follower")

return function(song)
  song:phrase("Bass Intro", { duration = 8.0, notes = {} })  -- silent

  -- Main: Cm — Fm — Cm — Gm
  song:phrase("Bass Main", {
    duration = 16.0,
    notes = {
      { pitch = 36, start = 0,  duration = 3.5 },  -- C2
      { pitch = 41, start = 4,  duration = 3.5 },  -- F2
      { pitch = 36, start = 8,  duration = 3.5 },  -- C2
      { pitch = 43, start = 12, duration = 3.5 },  -- G2
    },
  })

  -- Alt: Ab — G — F — C
  song:phrase("Bass Alt", {
    duration = 16.0,
    notes = {
      { pitch = 44, start = 0,  duration = 3.5 },  -- Ab2
      { pitch = 43, start = 4,  duration = 3.5 },  -- G2
      { pitch = 41, start = 8,  duration = 3.5 },  -- F2
      { pitch = 36, start = 12, duration = 3.5 },  -- C2
    },
  })

  -- Surprise: a single held C.
  song:phrase("Bass Surprise", {
    duration = 8.0,
    notes = { { pitch = 36, start = 0, duration = 7.5 } },
  })

  make_follower(song, {
    target = "Bass",
    phrases = {
      intro    = "Bass Intro",
      main     = "Bass Main",
      alt      = "Bass Alt",
      surprise = "Bass Surprise",
    },
  })
end
