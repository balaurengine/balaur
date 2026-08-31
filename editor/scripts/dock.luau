-- Bottom dock (150 px, 212 px for Timeline): Output / Assets / Timeline.
local theme = require("scripts/theme")
local model = require("scripts/model")
local animlib = require("scripts/anim")

local M = {}

local function tab_row(S, k)
    ui.horizontal({ height = 34 }, function()
        ui.spacing(6, 0)
        ui.add_space(6)
        for _, tab in { "output", "assets", "timeline" } do
            local active = S.dock == tab
            local label = string.upper(string.sub(tab, 1, 1)) .. string.sub(tab, 2)
            if ui.pill(label, {
                height = 24, size = 11.5, strong = true,
                fill = if active then k.accent_fill else k.sunken,
                color = if active then k.on_accent else k.dim,
            }) then
                S.dock = tab
            end
        end
        if not S.compact then
            ui.right(function()
                ui.add_space(8)
                local hint
                if S.dock == "assets" then
                    hint = S.game_root
                elseif S.dock == "timeline" then
                    local node = model.selected(S)
                    local def = animlib.definition(S)
                    if def == nil then
                        hint = "no clip"
                    else
                        local name = animlib.clip_name(node)
                        hint = string.format("0.00 – %.2f s · %s · %s",
                            animlib.length(def),
                            if name == "" then "unnamed" else name,
                            animlib.library_path(node) or "inline")
                    end
                else
                    hint = `balaur-core {S.version}`
                end
                ui.label(hint, { size = 11, font = "mono", color = k.faint })
            end)
        end
    end)
    ui.separator(k.line_soft)
end

local function output(S, k)
    ui.scroll("output", nil, function()
        ui.spacing(4, 2)
        for _, entry in log.recent(60) do
            ui.horizontal(nil, function()
                ui.add_space(6)
                local color = k.dim
                if entry.level == "warn" then
                    color = k.accent
                elseif entry.level == "error" then
                    color = k.accent
                elseif entry.tag == "env" then
                    color = k.sage
                end
                ui.label(string.format("%6.1fs", entry.time), { size = 11.5, font = "mono", color = k.faint })
                ui.add_space(6)
                local tag = string.sub(entry.tag, 1, 9)
                ui.label(tag .. string.rep(" ", math.max(0, 9 - #tag)), { size = 11.5, font = "mono", strong = true, color = color })
                ui.add_space(6)
                ui.label(entry.message, { size = 11.5, font = "mono", color = k.dim })
            end)
        end
    end)
end

local function assets(S, k)
    ui.add_space(8)
    ui.horizontal(nil, function()
        ui.spacing(12, 0)
        ui.add_space(12)
        local function card(icon, name, tint, on_click)
            ui.vertical(function()
                if ui.pill(icon, {
                    height = 66, min_width = 96, size = 20,
                    fill = tint, color = k.dim,
                    tooltip = name,
                }) and on_click ~= nil then
                    on_click()
                end
                ui.label(string.sub(name, 1, 14), { size = 11, font = "mono", color = k.dim })
            end)
        end
        local count = 0
        for _, f in fs.list(S.game_root) do
            if count >= 8 then
                break
            end
            if f.is_dir then
                card("▸", f.name, k.sunken)
                count += 1
            end
        end
        for _, f in fs.list(S.game_root .. "/scripts") do
            if count >= 8 then
                break
            end
            local rel = "scripts/" .. f.name
            card("‹›", f.name, k.sage_soft, function()
                for i, node in S.doc do
                    if node.script == rel then
                        S.select(i)
                        S.set_persona("script")
                        break
                    end
                end
            end)
            count += 1
        end
        for _, f in fs.list(S.game_root .. "/scenes") do
            if count >= 8 then
                break
            end
            local rel = "scenes/" .. f.name
            card("◇", f.name, k.accent_soft, function()
                if S.scene_rel ~= rel then
                    model.open_scene(S, rel)
                end
            end)
            count += 1
        end
    end)
end

-- The keyframe timeline over the engine's clip: scrub with the ruler (which
-- seeks the engine's player, so what the viewport shows is the engine's own
-- pose), ＋ Key snapshots the node's transform into the clip at the playhead.
-- The data is an `animation_clip` asset; scripts/anim.luau is the adapter.
local function timeline(S, k)
    local node = model.selected(S)
    local def = animlib.definition(S)
    ui.add_space(4)
    if def == nil then
        ui.horizontal({ height = 30 }, function()
            ui.add_space(12)
            local missing = if animlib.component(node) == nil
                then node.name .. " has no clip"
                else node.name .. ": clip '" .. animlib.clip_name(node) .. "' is not in its library"
            ui.label(missing, { size = 12, font = "mono", color = k.faint })
            ui.add_space(12)
            if ui.pill("＋ Create clip", {
                height = 24, size = 11.5, strong = true,
                fill = k.accent_fill, color = k.on_accent,
            }) then
                animlib.ensure_clip(S)
            end
        end)
        return
    end
    local length = animlib.length(def)
    local name_w = 186
    local lane_w = math.max(120, ui.available_width() - name_w - 60)
    -- Ruler row: playhead scrubber + key controls. The name column and the
    -- lane share their origin with the track rows below so keys line up
    -- with the scrubber.
    ui.horizontal({ height = 26 }, function()
        ui.add_space(8)
        ui.horizontal({ height = 26, width = name_w - 12 }, function()
            if ui.pill("＋ Key", {
                height = 22, size = 11, strong = true, tooltip = "Insert keyframe at playhead (or press K)",
                fill = k.accent_soft, color = k.accent,
            }) then
                animlib.add_key(S, S.anim_time)
            end
            if ui.pill("− Key", {
                height = 22, size = 11, strong = true, tooltip = "Delete keyframe at playhead",
                fill = k.sunken, color = k.dim,
            }) then
                animlib.delete_key_at(S, S.anim_time)
            end
        end)
        local t, scrubbed = ui.slider(S.anim_time, 0, length, {
            width = lane_w,
            rail = k.sunken, fill = k.accent_fill, knob = k.bg,
        })
        if scrubbed then
            animlib.seek(S, t)
        end
        ui.add_space(6)
        ui.label(string.format("%.2f / %.2f s", S.anim_time, length), { size = 11, font = "mono", color = k.faint })
    end)
    -- One lane per track the clip actually has, with key dots (filled before
    -- the playhead, hollow after). A component track and a method track show
    -- up here the same way a transform track does, because the clip is the
    -- engine's and the engine has all three.
    ui.scroll("timeline-tracks", nil, function()
        for index, track in animlib.tracks(def) do
            local label = animlib.track_label(track)
            local color = if index % 2 == 1 then k.accent else k.sage
            ui.horizontal({ height = 30 }, function()
                ui.add_space(8)
                ui.horizontal({ height = 30, width = name_w - 12 }, function()
                    ui.frame({ fill = k.sunken, radius = 15, padding_x = 10 }, function()
                        ui.horizontal({ height = 30, tight = true }, function()
                            ui.dot(color, 7)
                            ui.add_space(4)
                            ui.label(label, { size = 12, font = "mono", color = k.text })
                        end)
                    end)
                end)
                local cursor = 0
                for _, key in (track.keys or {}) do
                    local x = math.max(0, ((key.t or 0) / length) * lane_w - 5)
                    local gap = x - cursor
                    if gap > 0 then
                        ui.add_space(gap)
                        cursor = x
                    end
                    local before = (key.t or 0) <= S.anim_time + 1e-4
                    ui.label(if before then "●" else "○", { size = 11, color = color })
                    cursor += 14
                end
            end)
        end
    end)
end

function M.draw(S)
    local k = theme.current
    local height = if S.dock == "timeline" then 212 else 150
    ui.bottom_panel("dock", { height = height, fill = k.panel }, function()
        tab_row(S, k)
        if S.dock == "assets" then
            assets(S, k)
        elseif S.dock == "timeline" then
            timeline(S, k)
        else
            output(S, k)
        end
    end)
end

return M
