-- Bottom dock (150 px, 212 px for Timeline and Debugger): Output / Assets /
-- Timeline / Debugger.
local theme = require("scripts/theme")
local model = require("scripts/model")
local animlib = require("scripts/anim")

local M = {}

local function tab_row(S, k)
    ui.horizontal({ height = 34 }, function()
        ui.spacing(6, 0)
        ui.add_space(6)
        for _, tab in { "output", "assets", "timeline", "debugger" } do
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
        -- Output owns the right side of its own tab row: filter, search and
        -- clear are the controls a log is unusable without.
        if S.dock == "output" then
            ui.right(function()
                ui.add_space(8)
                if ui.pill("clear", {
                    height = 22, size = 11, strong = true, tooltip = "Empty the log buffer",
                    fill = k.sunken, color = k.dim,
                }) then
                    log.clear()
                end
                for _, level in { "error", "warn", "all" } do
                    local active = (S.log_level or "all") == level
                    if ui.pill(level, {
                        height = 22, size = 11, strong = true,
                        tooltip = `Show {level} and above`,
                        fill = if active then k.accent_soft else k.sunken,
                        color = if active then k.accent else k.dim,
                    }) then
                        S.log_level = level
                    end
                end
                if S.compact then
                    -- No field on screen, so no invisible filter hiding rows.
                    S.log_query = ""
                else
                    S.log_query = ui.text_field("log-search", "filter…", {
                        size = 11, font = "mono", width = 130, height = 22,
                        fill = k.sunken, stroke = k.line_soft, color = k.text,
                    })
                end
            end)
        elseif not S.compact then
            ui.right(function()
                ui.add_space(8)
                local hint
                if S.dock == "assets" then
                    -- A project-relative path: the absolute one is this
                    -- machine's, and lands in every screenshot.
                    hint = `res:// · {S.game_name}`
                elseif S.dock == "debugger" then
                    hint = if S.paused ~= nil then S.paused.reason else "F5 run · F10 over · F11 into · ⇧F11 out"
                else
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
                end
                ui.label(hint, { size = 11, font = "mono", color = k.faint })
            end)
        end
    end)
    ui.separator(k.line_soft)
end

-- How loud an entry is, so `error` and `warn` stop looking identical.
local SEVERITY = { error = 3, warn = 2, info = 1, debug = 0, trace = 0 }
local FLOOR = { all = 0, warn = 2, error = 3 }

local function output(S, k)
    local floor = FLOOR[S.log_level or "all"] or 0
    local query = string.lower(S.log_query or "")
    local shown, hidden = 0, 0
    -- The buffer holds 500; asking for 60 threw away most of the session.
    local entries = log.recent(500)
    ui.scroll("output", { stick_to_bottom = true }, function()
        ui.spacing(4, 2)
        for _, entry in entries do
            local level = SEVERITY[entry.level] or 1
            local matches = query == ""
                or string.find(string.lower(entry.message), query, 1, true) ~= nil
                or string.find(string.lower(entry.tag), query, 1, true) ~= nil
            if level < floor or not matches then
                hidden += 1
                continue
            end
            shown += 1
            ui.horizontal(nil, function()
                ui.add_space(6)
                local color, marker = k.dim, " "
                if entry.level == "error" then
                    color, marker = k.accent, "✕"
                elseif entry.level == "warn" then
                    color, marker = k.accent, "!"
                elseif entry.tag == "env" then
                    color = k.sage
                end
                ui.label(marker, { size = 11.5, font = "mono", strong = true, color = color })
                ui.label(string.format("%6.1fs", entry.time), { size = 11.5, font = "mono", color = k.faint })
                ui.add_space(6)
                local tag = string.sub(entry.tag, 1, 9)
                ui.label(tag .. string.rep(" ", math.max(0, 9 - #tag)), {
                    size = 11.5, font = "mono", strong = true,
                    color = if entry.level == "error" then k.accent else color,
                })
                ui.add_space(6)
                ui.label(entry.message, {
                    size = 11.5, font = "mono",
                    strong = entry.level == "error",
                    color = if entry.level == "error" then k.text else k.dim,
                })
                -- Structured fields the event carried, which the message text
                -- deliberately does not repeat.
                for _, field in (entry.fields or {}) do
                    ui.add_space(8)
                    ui.label(`{field.name}={field.value}`, { size = 11, font = "mono", color = k.faint })
                end
            end)
        end
        if shown == 0 then
            ui.horizontal(nil, function()
                ui.add_space(6)
                local why = if hidden > 0 then `{hidden} entries hidden by the filter` else "nothing logged yet"
                ui.label(why, { size = 11.5, font = "mono", color = k.faint })
            end)
        end
    end)
end

local CARD_W, CARD_H = 96, 66

-- Icon and tint by extension, so a directory of assets reads at a glance.
local function asset_look(name, is_dir)
    if is_dir then
        return "▸", "sunken"
    elseif string.match(name, "%.luau$") or string.match(name, "%.rn$") then
        return "‹›", "sage_soft"
    elseif string.match(name, "%.toml$") then
        return "◇", "accent_soft"
    elseif string.match(name, "%.png$") or string.match(name, "%.jpg$") then
        return "▧", "accent_soft"
    end
    return "▫", "sunken"
end

-- Opening a file means the thing the editor can actually do with it: jump to
-- the node running a script, or open a scene. Anything else just selects.
local function open_asset(S, rel)
    if string.match(rel, "%.toml$") and string.sub(rel, 1, 7) == "scenes/" then
        if S.scene_rel ~= rel then
            model.open_scene(S, rel)
        end
        return
    end
    for i, node in S.doc do
        if node.script == rel then
            S.select(i)
            S.set_persona("script")
            return
        end
    end
    log.info(`{rel} is not attached to any node`)
end

local function assets(S, k)
    local dir = S.asset_dir or ""
    local files = model.list(S, dir)
    -- Breadcrumb: where we are, and the way back up.
    ui.horizontal({ height = 24 }, function()
        ui.add_space(12)
        if dir ~= "" then
            if ui.pill("↑", { height = 20, size = 11, strong = true, fill = k.sunken, color = k.dim, tooltip = "Up one folder" }) then
                S.asset_dir = string.match(dir, "^(.*)/[^/]+$") or ""
            end
            ui.add_space(6)
        end
        ui.label(`res://{dir}`, { size = 11, font = "mono", color = k.faint })
        ui.right(function()
            ui.add_space(12)
            ui.label(`{#files} items`, { size = 11, font = "mono", color = k.faint })
        end)
    end)
    -- Cards wrap into rows: the row count is what the dock's height scrolls.
    local per_row = math.max(1, math.floor((ui.available_width() - 24) / (CARD_W + 12)))
    ui.scroll("assets", nil, function()
        ui.add_space(6)
        local index = 0
        while index < #files do
            ui.horizontal(nil, function()
                ui.spacing(12, 0)
                ui.add_space(12)
                for _ = 1, per_row do
                    index += 1
                    local f = files[index]
                    if f == nil then
                        break
                    end
                    local rel = if dir == "" then f.name else `{dir}/{f.name}`
                    local icon, tint = asset_look(f.name, f.is_dir)
                    ui.vertical(function()
                        if ui.pill(icon, {
                            height = CARD_H, min_width = CARD_W, size = 20, radius = 12,
                            fill = k[tint], color = k.dim,
                            tooltip = rel,
                        }) then
                            if f.is_dir then
                                S.asset_dir = rel
                            else
                                open_asset(S, rel)
                            end
                        end
                        ui.label(string.sub(f.name, 1, 14), { size = 11, font = "mono", color = k.dim })
                    end)
                end
            end)
            ui.add_space(8)
        end
        if #files == 0 then
            ui.horizontal(nil, function()
                ui.add_space(12)
                ui.label("empty folder", { size = 11.5, font = "mono", color = k.faint })
            end)
        end
    end)
end

-- Keyframe timeline over the engine's clip (scripts/anim.luau is the adapter):
-- scrubbing seeks the engine's player, so the viewport shows the engine's pose.
local function timeline(S, k)
    local node = animlib.player(S)
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
    -- One lane per track, key dots filled before the playhead and hollow after;
    -- component and method tracks render the same way transform tracks do.
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

-- A local as one line; a table is its JSON, cut short.
local function value_text(v)
    if type(v) ~= "table" then
        return tostring(v)
    end
    local ok, text = pcall(json.encode, v)
    if not ok then
        return "{…}"
    end
    return if #text > 96 then string.sub(text, 1, 93) .. "…" else text
end

-- The pause: transport, then the frames with the chosen one's locals open.
local function debugger_view(S, k)
    local p = S.paused
    if p == nil then
        ui.horizontal({ height = 30 }, function()
            ui.add_space(12)
            local hint = if S.playing
                then "running · click a line number in the script to set a breakpoint"
                else "set breakpoints in the script gutter, then run the scene"
            ui.label(hint, { size = 11.5, font = "mono", color = k.faint })
        end)
        return
    end
    ui.horizontal({ height = 30 }, function()
        ui.add_space(8)
        local steps = {
            { "▶ Continue", debugger.CONTINUE, "F5" },
            { "⤼ Over", debugger.STEP_OVER, "F10" },
            { "⤵ Into", debugger.STEP_INTO, "F11" },
            { "⤴ Out", debugger.STEP_OUT, "⇧F11" },
        }
        for _, step in steps do
            if ui.pill(step[1], {
                height = 24, size = 11.5, strong = true, tooltip = step[3],
                fill = k.accent_soft, color = k.accent,
            }) then
                S.resume(step[2])
            end
        end
        ui.add_space(12)
        ui.label(`{p.reason} · {S.paused_rel or p.path}:{p.line}`, { size = 11.5, font = "mono", color = k.dim })
    end)
    ui.scroll("debug-frames", nil, function()
        ui.spacing(4, 2)
        for i, frame in p.frames do
            local chosen = i == S.debug_frame
            ui.horizontal({ height = 24 }, function()
                ui.add_space(6)
                if ui.pill(frame["function"], {
                    height = 20, size = 11, strong = true,
                    fill = if chosen then k.accent_fill else k.sunken,
                    color = if chosen then k.on_accent else k.dim,
                }) then
                    S.debug_frame = i
                end
                ui.add_space(6)
                ui.label(`{S.script_rel(frame.path)}:{frame.line}`, { size = 11.5, font = "mono", color = k.faint })
            end)
            if chosen then
                local names = {}
                for name in pairs(frame.locals) do
                    table.insert(names, name)
                end
                table.sort(names)
                for _, name in names do
                    ui.horizontal(nil, function()
                        ui.add_space(24)
                        ui.label(name, { size = 11.5, font = "mono", strong = true, color = k.text })
                        ui.add_space(6)
                        ui.label(value_text(frame.locals[name]), { size = 11.5, font = "mono", color = k.dim })
                    end)
                end
                if #names == 0 then
                    ui.horizontal(nil, function()
                        ui.add_space(24)
                        ui.label("no locals", { size = 11.5, font = "mono", color = k.faint })
                    end)
                end
            end
        end
    end)
end

function M.draw(S)
    local k = theme.current
    local height = if S.dock == "timeline" or S.dock == "debugger" then 212 else 150
    ui.bottom_panel("dock", { height = height, fill = k.panel }, function()
        tab_row(S, k)
        if S.dock == "assets" then
            assets(S, k)
        elseif S.dock == "timeline" then
            timeline(S, k)
        elseif S.dock == "debugger" then
            debugger_view(S, k)
        else
            output(S, k)
        end
    end)
end

return M
