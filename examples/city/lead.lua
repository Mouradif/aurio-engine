-- Syncopated 4-bar lead with a call/response structure: hook ↔ answer.
--
-- opts:
--   name       phrase/track display name (also used to derive sub-phrase names)
--   target     instrument id or label this track drives (default "Lead")
--   transpose  semitone offset applied to every pitch (default 0)
return function(song, opts)
    opts = opts or {}
    local name      = opts.name      or "Lead"
    local target    = opts.target    or "Lead"
    local transpose = opts.transpose or 0

    local function n(pitch, start, duration)
        return { pitch = pitch + transpose, start = start, duration = duration }
    end

    -- Bars 1-2: punchy A-minor pentatonic hook.
    -- Bars 3-4: drops down a register, lands on C.
    local hook = {
        n(76, 0.0,  1.0),   -- E5
        n(79, 1.0,  0.5),   -- G5
        n(81, 1.5,  1.0),   -- A5
        n(79, 2.5,  0.5),   -- G5
        n(76, 3.0,  1.0),   -- E5

        n(72, 4.0,  1.5),   -- C5
        n(74, 5.5,  0.5),   -- D5
        n(76, 6.0,  1.0),   -- E5
        n(72, 7.0,  1.0),   -- C5

        n(72, 8.0,  0.5),   -- C5
        n(77, 8.5,  0.5),   -- F5
        n(81, 9.0,  1.0),   -- A5
        n(79, 10.0, 1.5),   -- G5
        n(77, 11.5, 0.5),   -- F5

        n(74, 12.0, 1.0),   -- D5
        n(76, 13.0, 1.0),   -- E5
        n(72, 14.0, 2.0),   -- C5
    }

    -- Sparser answer phrase — long held notes give the hook room to breathe.
    local answer = {
        n(64, 0.0,  2.0),   -- E4
        n(67, 2.0,  1.0),   -- G4
        n(69, 3.0,  1.0),   -- A4
        n(72, 4.0,  3.0),   -- C5
        n(76, 7.0,  1.0),   -- E5
        n(74, 8.0,  2.0),   -- D5
        n(72, 10.0, 2.0),   -- C5
        n(67, 12.0, 4.0),   -- G4 hold
    }

    local hook_name   = name .. " Hook"
    local answer_name = name .. " Answer"

    song:phrase(hook_name,   { duration = 16.0, notes = hook })
    song:phrase(answer_name, { duration = 16.0, notes = answer })

    song:track({
        targets = { target },
        initial = hook_name,
        transitions = {
            { from = hook_name,   to = answer_name },
            { from = answer_name, to = hook_name   },
        },
    })
end
