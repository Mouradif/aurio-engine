-- Helper for tracks that follow the conductor's `section_state` global.
--
-- Given a target instrument and a {intro, main, alt, surprise} -> phrase-name
-- table, builds a track whose transitions are driven entirely by reads of
-- the global. Whatever section the conductor moves to, this track moves to
-- the same one on its next phrase boundary.
--
-- opts:
--   target   instrument id or label this track drives
--   phrases  table mapping section name -> phrase name, e.g.
--              { intro = "Kick Intro", main = "Kick Main", ... }
return function(song, opts)
  local target  = opts.target
  local phrases = opts.phrases
  local sections = { "intro", "main", "alt", "surprise" }

  -- Wrap the closure in a factory so each transition captures its own
  -- target_section (defensive against any per-iteration scoping surprises).
  local function condition_for(target_section)
    return function() return section_state == target_section end
  end

  local transitions = {}
  for _, from in ipairs(sections) do
    for _, to in ipairs(sections) do
      if from ~= to then
        table.insert(transitions, {
          from = phrases[from],
          to = phrases[to],
          condition = condition_for(to),
        })
      end
    end
  end

  song:track({
    targets = { target },
    initial = phrases.intro,
    transitions = transitions,
  })
end
