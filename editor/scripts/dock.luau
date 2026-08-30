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
                    local anim = animlib.clip(model.selected(S))
                    hint = if anim then string.format("0.00 – %.2f s · %s", anim.length, anim.clip) else "no clip"
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

-- The real keyframe timeline: scrub with the ruler, ＋ Key inserts a key at
-- the playhead from the node's current transform (see scripts/anim.luau).
local function timeline(S, k)
    local node = model.selected(S)
    local anim = animlib.clip(node)
    ui.add_space(4)
    if anim == nil then
        ui.horizontal({ height = 30 }, function()
            ui.add_space(12)
            ui.label(node.name .. " has no clip", { size = 12, font = "mono", color = k.faint })
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
        local t, scrubbed = ui.slider(S.anim_time, 0, anim.length, {
            width = lane_w,
            rail = k.sunken, fill = k.accent_fill, knob = k.bg,
        })
        if scrubbed then
            S.anim_time = t
            animlib.apply_playhead(S)
        end
        ui.add_space(6)
        ui.label(string.format("%.2f / %.2f s", S.anim_time, anim.length), { size = 11, font = "mono", color = k.faint })
    end)
    -- Track lanes with key dots (filled before the playhead, hollow after).
    for _, track in { "position", "rotation" } do
        ui.horizontal({ height = 30 }, function()
            ui.add_space(8)
            ui.horizontal({ height = 30, width = name_w - 12 }, function()
                ui.frame({ fill = k.sunken, radius = 15, padding_x = 10 }, function()
                    ui.horizontal({ height = 30, tight = true }, function()
                        ui.dot(if track == "position" then k.accent else k.sage, 7)
                        ui.add_space(4)
                        ui.label(track, { size = 12, font = "mono", color = k.text })
                    end)
                end)
            end)
            local cursor = 0
            for _, key in anim.keys do
                local x = math.max(0, (key.t / math.max(anim.length, 1e-6)) * lane_w - 5)
                local gap = x - cursor
                if gap > 0 then
                    ui.add_space(gap)
                    cursor = x
                end
                local before = key.t <= S.anim_time + 1e-4
                ui.label(if before then "●" else "○", {
                    size = 11,
                    color = if track == "position" then k.accent else k.sage,
                })
                cursor += 14
            end
        end)
    end
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
