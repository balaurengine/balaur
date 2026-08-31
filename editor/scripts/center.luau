-- Center region: tool rail (46 px), document tabs (38 px), then either the
-- 3D viewport overlays or the script editor.
local theme = require("scripts/theme")
local defs = require("scripts/defs")
local model = require("scripts/model")
local highlight = require("scripts/highlight")

local M = {}

-- The viewport's design-px rect, published on `S` for the gizmo, the widget
-- layer and the overlay: three readers, one derivation, computed where layout is.
local function viewport_rect(S)
    local sw, sh = S.screen_w or 1280, S.screen_h or 800
    local dock_h = if S.dock == "timeline" then 212 else 150
    local rail_w = if S.rail_visible then 46 else 0
    return {
        x = 262 + rail_w, y = 56 + 38,
        w = sw - 308 - 262 - rail_w, h = sh - 28 - dock_h - 56 - 38,
    }
end

local function rail(S, k)
    ui.left_panel("rail", { width = 46, fill = k.bg, padding_y = 8 }, function()
        ui.vertical(function()
            ui.spacing(4, 3)
            for _, tool in defs.tools[S.persona] do
                ui.horizontal(nil, function()
                    ui.add_space(3)
                    local active = S.tool == tool
                    if ui.circle_button(defs.tool_icons[tool], {
                        d = 32, size = 15,
                        fill = if active then k.accent_soft else "#00000000",
                        color = if active then k.accent else k.dim,
                        tooltip = defs.tool_names[tool],
                    }) then
                        S.tool = tool
                    end
                end)
            end
        end)
    end)
end

local function doc_tabs(S, k)
    local node = model.selected(S)
    ui.top_panel("tabs", { height = 38, fill = k.panel, padding_x = 12 }, function()
        ui.horizontal({ height = 38 }, function()
            ui.spacing(6, 0)
            local scene_name = string.match(S.scene_rel, "([^/]+)$")
            local tabs = {
                { id = "scene", icon = "◇", label = scene_name },
                {
                    id = "script",
                    icon = "‹›",
                    label = if node.script
                        then string.match(node.script, "([^/]+)$")
                            .. (if S.script_dirty[S.game_root .. "/" .. node.script] then " ●" else "")
                        else "no script",
                },
                { id = "events", icon = "⚯", label = "events" },
            }
            for _, tab in tabs do
                local active = S.doctab == tab.id
                if ui.pill(tab.label, {
                    height = 27, size = 12, icon = tab.icon,
                    fill = if active then k.accent_soft else "#00000000",
                    color = if active then k.accent else k.dim,
                }) then
                    S.doctab = tab.id
                    if tab.id == "script" then
                        S.set_persona("script")
                    end
                end
            end
            if not S.compact then
                ui.right(function()
                    local hint
                    if S.doctab == "script" and node.script then
                        hint = "Luau · " .. node.script
                    else
                        local nodes = model.stats(S)
                        hint = `{S.game_name} · {nodes} nodes`
                    end
                    ui.label(hint, { size = 11, font = "mono", color = k.faint })
                end)
            end
        end)
    end)
end

local function chip(S, k, key, label, default_on)
    local state = S.chip_state[S.persona] or {}
    S.chip_state[S.persona] = state
    -- Engine-backed chips (no declared default, see defs.chips) read their
    -- value back from the engine every frame; the rest live in `chip_state`.
    local live = if S.chip_value ~= nil then S.chip_value(S.persona, key) else nil
    if live == nil and state[key] == nil then
        state[key] = default_on
    end
    local on = if live ~= nil then live else state[key]
    if ui.pill(label, {
        height = 26, size = 11, strong = true,
        fill = if on then k.accent_soft else k.panel,
        stroke = k.line_soft,
        color = if on then k.accent else k.dim,
    }) then
        if live == nil then
            state[key] = not on
        end
        if S.on_chip ~= nil then
            S.on_chip(S.persona, key, not on)
        end
    end
end

local function viewport(S, k)
    -- The central panel is a transparent hole for the 3D scene; the chrome on
    -- top of it goes in the overlay layer, which is the only thing that draws
    -- above a previewed widget (panels all share egui's background layer).
    ui.central_panel({ transparent = true }, function() end)
    local vp = S.viewport
    ui.overlay("viewport", { x = vp.x + 12, y = vp.y + 12, w = vp.w - 24, h = vp.h - 24 }, function()
        local vw = vp.w - 24
        local vh = vp.h - 24
        -- Interface persona overlays: safe-area inset and 16:9 frame.
        local chips_ui = S.chip_state[S.persona] or {}
        if S.persona == "ui" and chips_ui.safe ~= false then
            ui.rect_stroke(34, 34, vw - 68, vh - 68, { color = k.accent_fill, dashed = true })
        end
        if S.persona == "ui" and chips_ui.res then
            local target = 16 / 9
            local w, h = vw, vw / target
            if h > vh then
                h = vh
                w = vh * target
            end
            ui.rect_stroke((vw - w) / 2, (vh - h) / 2, w, h, { color = k.line, dashed = true })
        end
        -- Overlay chips, top-left.
        ui.horizontal(nil, function()
            ui.spacing(6, 0)
            for _, c in defs.chips[S.persona] do
                local label = c[2]
                if c[1] == "proj" and S.mode2d then
                    label = "2D · Orthographic"
                end
                chip(S, k, c[1], label, c[3])
            end
        end)
        -- Bottom row: axis pill (left), zoom pill (right).
        ui.add_space(vh - 26 - 34)
        ui.horizontal({ height = 28 }, function()
            ui.frame({ fill = k.panel, stroke = k.line_soft, radius = 14, padding_x = 10 }, function()
                ui.horizontal({ height = 28, tight = true }, function()
                    ui.label("x →", { size = 11, font = "mono", color = k.accent })
                    ui.add_space(6)
                    ui.label("y ↑", { size = 11, font = "mono", color = k.sage })
                    ui.add_space(6)
                    ui.label("z ↓", { size = 11, font = "mono", color = k.dim })
                end)
            end)
            ui.right(function()
                ui.frame({ fill = k.panel, stroke = k.line_soft, radius = 14, padding_x = 3, padding_y = 3 }, function()
                    ui.horizontal({ height = 22, width = 116 }, function()
                        if ui.pill("−", { height = 22, size = 12, color = k.dim }) then
                            S.set_zoom(S.zoom - 25)
                        end
                        ui.label(`{S.zoom} %`, { size = 11, font = "mono", color = k.text })
                        if ui.pill("＋", { height = 22, size = 12, color = k.dim }) then
                            S.set_zoom(S.zoom + 25)
                        end
                    end)
                end)
            end)
        end)
    end)
end

local function script_editor(S, k)
    local node = model.selected(S)
    ui.central_panel({ fill = k.code_bg }, function()
        if node.script == nil then
            ui.add_space(24)
            ui.horizontal(nil, function()
                ui.add_space(24)
                ui.label("no script attached to " .. node.name, { size = 12, font = "mono", color = k.faint })
            end)
            return
        end
        local path = S.game_root .. "/" .. node.script
        local src = model.source(S, node.script)
        if src == nil then
            ui.label("missing file: " .. node.script, { size = 12, font = "mono", color = k.faint })
            return
        end
        -- Hooks come from the live buffer, so they update while typing —
        -- re-analysed only when that buffer actually changed.
        local current = S.script_buffers[path] or src
        if S.hooks_src ~= current then
            S.hooks_src = current
            local _, parsed = highlight.analyze(current)
            S.hooks_live = parsed
        end
        local hooks = S.hooks_live
        ui.right_panel("hooks", { width = 172, fill = k.panel, padding_x = 12 }, function()
            ui.add_space(11)
            ui.label("HOOKS IN FILE", { size = 10, font = "heading", strong = true, color = k.dim })
            ui.add_space(9)
            for _, hook in hooks do
                ui.frame({ fill = k.sunken, radius = 14, padding_x = 10 }, function()
                    ui.horizontal({ height = 27 }, function()
                        local color = if hook.name == "init" or hook.name == "update" then k.accent else k.sage
                        ui.dot(color, 6)
                        ui.add_space(4)
                        ui.label(hook.name, { size = 11, font = "mono", color = k.text })
                    end)
                end)
                ui.add_space(4)
            end
            if S.script_dirty[path] then
                ui.add_space(8)
                ui.label("● unsaved · ⌘S", { size = 10, font = "mono", color = k.accent })
            end
        end)
        -- Editable, syntax-highlighted buffer (persists per file; ⌘S saves
        -- and hot reloads the game code).
        ui.scroll("code", nil, function()
            ui.add_space(12)
            local text, changed = ui.code_editor("edit:" .. path, src, {
                size = 12.5, gutter_width = 34, gutter_color = k.faint,
                k_key = k.k_key, k_str = k.k_str, k_num = k.k_num,
                k_com = k.k_com, k_fn = k.k_fn, k_type = k.k_type,
                k_punc = k.k_punc,
            })
            S.script_buffers[path] = text
            if changed then
                S.script_dirty[path] = true
            end
            ui.add_space(12)
        end)
    end)
end

-- The `events` document tab: every script's engine-facing hooks, per node.
local function events_view(S, k)
    ui.central_panel({ fill = k.bg, padding_x = 24, padding_y = 16 }, function()
        ui.label("EVENT BINDINGS", { size = 10, font = "heading", strong = true, color = k.dim })
        ui.add_space(8)
        for i, node in S.doc do
            if node.script ~= nil then
                for _, hook in model.hooks(S, node.script) do
                    ui.horizontal({ height = 30 }, function()
                        ui.frame({ fill = k.sunken, radius = 15, padding_x = 11 }, function()
                            ui.horizontal({ height = 30 }, function()
                                local color = if hook.name == "init" or hook.name == "update" then k.accent else k.sage
                                ui.dot(color, 6)
                                ui.add_space(4)
                                ui.label(`{node.name}.{hook.name}()`, { size = 11.5, font = "mono", color = k.text })
                                ui.right(function()
                                    ui.label(node.script .. ":" .. hook.line, { size = 11, font = "mono", color = k.faint })
                                end)
                            end)
                        end)
                    end)
                end
            end
        end
    end)
end

function M.draw(S)
    local k = theme.current
    local code = S.persona == "script" or S.doctab == "script"
    local document = S.doctab == "events" or code
    -- Viewport tools belong to the viewport: with a document open the rail is
    -- 46 px of dead column, and the code editor is the panel starved for it.
    S.rail_visible = not document and #defs.tools[S.persona] > 0
    S.viewport = viewport_rect(S)
    if S.rail_visible then
        rail(S, k)
    end
    doc_tabs(S, k)
    if S.doctab == "events" then
        events_view(S, k)
    elseif code then
        script_editor(S, k)
    else
        viewport(S, k)
    end
end

return M
