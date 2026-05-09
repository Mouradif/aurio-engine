-- Conductor track. Plays chord stabs and decides which section comes next.
-- Sets two globals that the follower tracks read:
--   section_state — "intro" | "main" | "alt" | "surprise"
--   main_count    — number of completed Main loops since the last reset
--
-- Section flow:
--   Intro (once) → Main → ... → Alt → Main → ... → Alt ...
--                          ↳ ~20% per loop, detour through Surprise → Main
return function(song)
  -- Re-seed math.random so the surprise timing differs run-to-run.
  -- (Lua 5.4 form: no-arg math.randomseed self-seeds with a random value.)
  math.randomseed()

  -- ---- helpers -----------------------------------------------------------
  local function chord(pitches, start, dur)
    local out = {}
    for _, p in ipairs(pitches) do
      table.insert(out, { pitch = p, start = start, duration = dur })
    end
    return out
  end

  local function append(dest, src)
    for _, x in ipairs(src) do table.insert(dest, x) end
  end

  -- ---- voicings (rooted around C4) ---------------------------------------
  local Cm7    = { 60, 63, 67, 70 }       -- C  Eb G  Bb
  local Fm7    = { 53, 56, 60, 63 }       -- F  Ab C  Eb
  local Gm7    = { 55, 58, 62, 65 }       -- G  Bb D  F
  local Abmaj7 = { 56, 60, 63, 67 }       -- Ab C  Eb G
  local Cm9    = { 60, 63, 67, 70, 74 }   -- C  Eb G  Bb D

  -- ---- section note tables -----------------------------------------------
  -- Intro: 2 bars, just sets the mood with two chord stabs.
  local intro_notes = {}
  append(intro_notes, chord(Cm7, 0, 3.5))
  append(intro_notes, chord(Fm7, 4, 3.5))

  -- Main: i — iv — i — V (over Cm).
  local main_notes = {}
  append(main_notes, chord(Cm7, 0,  3.5))
  append(main_notes, chord(Fm7, 4,  3.5))
  append(main_notes, chord(Cm7, 8,  3.5))
  append(main_notes, chord(Gm7, 12, 3.5))

  -- Alt: VI — V — iv — i (descending feel).
  local alt_notes = {}
  append(alt_notes, chord(Abmaj7, 0,  3.5))
  append(alt_notes, chord(Gm7,    4,  3.5))
  append(alt_notes, chord(Fm7,    8,  3.5))
  append(alt_notes, chord(Cm7,    12, 3.5))

  -- Surprise: 2-bar suspended Cm9.
  local surprise_notes = chord(Cm9, 0, 7.5)

  -- ---- phrases -----------------------------------------------------------
  song:phrase("Chord Intro", {
    duration = 8.0,
    on_enter = function()
      section_state = "intro"
      main_count = 0
      print("[section] intro")
    end,
    notes = intro_notes,
  })

  song:phrase("Chord Main", {
    duration = 16.0,
    on_enter = function()
      section_state = "main"
      main_count = 0
      print("[section] main")
    end,
    on_loop = function()
      main_count = main_count + 1
    end,
    notes = main_notes,
  })

  song:phrase("Chord Alt", {
    duration = 16.0,
    on_enter = function()
      section_state = "alt"
      print("[section] alt")
    end,
    notes = alt_notes,
  })

  song:phrase("Chord Surprise", {
    duration = 8.0,
    on_enter = function()
      section_state = "surprise"
      print("[section] surprise!")
    end,
    notes = surprise_notes,
  })

  -- ---- track / state machine ---------------------------------------------
  -- NOTE: order matters. The Alt rule must come *before* the Surprise rule
  -- so that the deterministic 4-loop exit always wins on the 4th play.
  song:track({
    targets = { "Chord" },
    initial = "Chord Intro",
    transitions = {
      -- Intro plays once and then advances unconditionally.
      { from = "Chord Intro", to = "Chord Main" },

      -- Main: forced exit to Alt after 4 plays (transition is checked before
      -- on_loop, so count == 3 fires on the 4th end).
      { from = "Chord Main", to = "Chord Alt",
        condition = function() return main_count >= 3 end },
      -- Otherwise, ~20% chance per loop to detour through Surprise.
      { from = "Chord Main", to = "Chord Surprise",
        condition = function() return math.random() < 0.20 end },

      -- Alt and Surprise each play once and return to Main.
      { from = "Chord Alt",      to = "Chord Main" },
      { from = "Chord Surprise", to = "Chord Main" },
    },
  })
end
