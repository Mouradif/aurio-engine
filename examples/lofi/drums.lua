-- Drum tracks (kick, snare, hat). Each follows the conductor's section_state.
local make_follower = require("lofi.follower")

return function(song)
  -- =========================================================================
  -- Kick
  -- =========================================================================
  song:phrase("Kick Intro", { duration = 8.0, notes = {} })  -- silent

  -- Main / Alt: same pattern — downbeat of each bar plus the "and-of-3"
  -- (a classic syncopated lo-fi feel).
  local kick_pattern = {}
  for bar = 0, 3 do
    local b = bar * 4
    table.insert(kick_pattern, { pitch = 36, start = b,       duration = 0.1 })
    table.insert(kick_pattern, { pitch = 36, start = b + 2.5, duration = 0.1 })
  end
  song:phrase("Kick Main", { duration = 16.0, notes = kick_pattern })
  song:phrase("Kick Alt",  { duration = 16.0, notes = kick_pattern })

  -- Surprise: half-time — just two kicks across 2 bars.
  song:phrase("Kick Surprise", {
    duration = 8.0,
    notes = {
      { pitch = 36, start = 0, duration = 0.1 },
      { pitch = 36, start = 4, duration = 0.1 },
    },
  })

  make_follower(song, {
    target = "Kick",
    phrases = {
      intro    = "Kick Intro",
      main     = "Kick Main",
      alt      = "Kick Alt",
      surprise = "Kick Surprise",
    },
  })

  -- =========================================================================
  -- Snare
  -- =========================================================================
  song:phrase("Snare Intro", { duration = 8.0, notes = {} })  -- silent

  -- Backbeat: beats 1 & 3 of each bar (8 hits over 4 bars).
  local snare_pattern = {}
  for i = 0, 7 do
    table.insert(snare_pattern, { pitch = 60, start = i * 2 + 1, duration = 0.1 })
  end
  song:phrase("Snare Main", { duration = 16.0, notes = snare_pattern })
  song:phrase("Snare Alt",  { duration = 16.0, notes = snare_pattern })

  song:phrase("Snare Surprise", {
    duration = 8.0,
    notes = {
      { pitch = 60, start = 2, duration = 0.1 },
      { pitch = 60, start = 6, duration = 0.1 },
    },
  })

  make_follower(song, {
    target = "Snare",
    phrases = {
      intro    = "Snare Intro",
      main     = "Snare Main",
      alt      = "Snare Alt",
      surprise = "Snare Surprise",
    },
  })

  -- =========================================================================
  -- Hat
  -- =========================================================================
  -- Intro hat is the only intro element that's audible — sparse offbeats.
  local hat_intro = {}
  for i = 0, 7 do
    table.insert(hat_intro, { pitch = 72, start = i + 0.5, duration = 0.05 })
  end
  song:phrase("Hat Intro", { duration = 8.0, notes = hat_intro })

  -- Main / Alt: offbeat 8th notes throughout.
  local hat_pattern = {}
  for i = 0, 15 do
    table.insert(hat_pattern, { pitch = 72, start = i + 0.5, duration = 0.05 })
  end
  song:phrase("Hat Main", { duration = 16.0, notes = hat_pattern })
  song:phrase("Hat Alt",  { duration = 16.0, notes = hat_pattern })

  -- Surprise: nearly silent.
  song:phrase("Hat Surprise", {
    duration = 8.0,
    notes = {
      { pitch = 72, start = 1.5, duration = 0.05 },
      { pitch = 72, start = 5.5, duration = 0.05 },
    },
  })

  make_follower(song, {
    target = "Hat",
    phrases = {
      intro    = "Hat Intro",
      main     = "Hat Main",
      alt      = "Hat Alt",
      surprise = "Hat Surprise",
    },
  })
end
