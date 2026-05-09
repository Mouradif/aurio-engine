-- Driving 4-bar bassline with a turnaround fill that kicks in every N loops.
--
-- opts:
--   name                 phrase/track display name (also used to derive sub-phrase names)
--   target               instrument id or label this track drives (default "Bass")
--   transpose            semitone offset applied to every pitch (default 0)
--   repeats_before_fill  how many groove loops before the fill plays (default 4)
return function(song, opts)
    opts = opts or {}
    local name      = opts.name      or "Bass"
    local target    = opts.target    or "Bass"
    local transpose = opts.transpose or 0
    local repeats   = opts.repeats_before_fill or 4

    local function bar(root, fifth, offset)
        local notes = {}
        for i = 0, 7 do
            local pitch = (i == 3 or i == 7) and fifth or root
            table.insert(notes, {
                pitch = pitch + transpose,
                start = offset + i * 0.5,
                duration = 0.45,
            })
        end
        return notes
    end

    local function chain(...)
        local out = {}
        for _, list in ipairs({...}) do
            for _, n in ipairs(list) do table.insert(out, n) end
        end
        return out
    end

    -- Progression: Am | Am | F | G
    local groove = chain(
        bar(45, 52, 0),    -- A2, E3
        bar(45, 52, 4),
        bar(41, 48, 8),    -- F2, C3
        bar(43, 50, 12)    -- G2, D3
    )

    -- Fill: same first 3 bars, then a walk-up from G2 back up to the tonic.
    local walk_up = {}
    local steps = { 43, 45, 47, 48, 50, 52, 53, 55 }
    for i, p in ipairs(steps) do
        table.insert(walk_up, {
            pitch = p + transpose,
            start = 12.0 + (i - 1) * 0.5,
            duration = 0.45,
        })
    end
    local fill = chain(
        bar(45, 52, 0),
        bar(45, 52, 4),
        bar(41, 48, 8),
        walk_up
    )

    local groove_name = name .. " Groove"
    local fill_name   = name .. " Fill"
    local loops       = 0  -- captured by hooks + transition condition below

    song:phrase(groove_name, {
        duration = 16.0,
        on_enter = function() loops = 0 end,
        on_loop  = function() loops = loops + 1 end,
        notes    = groove,
    })

    song:phrase(fill_name, {
        duration = 16.0,
        notes    = fill,
    })

    song:track({
        targets = { target },
        initial = groove_name,
        transitions = {
            { from = groove_name, to = fill_name,   condition = function() return loops == repeats - 1 end },
            { from = fill_name,   to = groove_name },
        },
    })
end
