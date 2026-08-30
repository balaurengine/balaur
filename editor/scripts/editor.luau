-- The balaur editor. A regular balaur project: this script drives the whole
-- shell (persona bar, node tree, viewport, inspector, dock, palette) through
-- the `ui` module, and edits the game project passed on the command line
-- (`balaur edit <project>`). Editing this file hot reloads the editor.
local theme = require("scripts/theme")
local model = require("scripts/model")
local chrome = require("scripts/chrome")
local left = require("scripts/left")
local inspector = require("scripts/inspector")
local center = require("scripts/center")
local dock = require("scripts/dock")
local palette = require("scripts/palette")
local gizmo = require("scripts/gizmo")
local anim = require("scripts/anim")

local Editor = {}

local BASE_EYE = { 9, 6, 13 }
local TARGET = { 0, 1, 0 }

function Editor:init()
    local S = {
        persona = "scene",
        tool = "select",
        doctab = "scene",
        dock = "output",
        zoom = 100,
        playing = false,
        palette = false,
        palette_query = "",
        chip_state = {},
        compact = false,
        version = "0.1.0",
        ui_scale = 1.25,
        script_buffers = {},
        script_dirty = {},
        anim_time = 0,
        collapsed = {},
    }
    self.S = S
    _G.ED = S

    function S.select(i)
        S.sel = i
    end

    function S.set_persona(id)
        S.persona = id
        S.tool = "select"
        S.doctab = if id == "script" then "script" else "scene"
        S.dock = if id == "anim" then "timeline" else "output"
    end

    function S.set_zoom(z)
        S.zoom = math.clamp(z, 25, 400)
        Editor.apply_camera(S)
    end

    function S.open_palette(query)
        S.palette = true
        S.palette_query = query or ""
        ui.set_text("palette-query", query or "")
    end

    function S.set_ui_scale(f)
        S.ui_scale = math.clamp(f, 0.75, 2.5)
        ui.set_scale(S.ui_scale)
    end

    function S.close_palette()
        S.palette = false
    end

    function S.toggle_play()
        if S.playing then
            S.stop_play()
        else
            -- Real play-in-editor: attach the game's scripts to the mirrored
            -- nodes (their init sets up physics etc.), then run.
            model.attach_game_scripts(S)
            S.playing = true
            S.dock = "output"
            physics.set_paused(false)
            log.info("play: running " .. S.game_name)
        end
    end

    function S.stop_play()
        if S.playing then
            log.info("stop: scene reset")
        end
        model.build_mirror(S)
        -- The game may have restyled the viewport (background, camera).
        theme.apply()
        Editor.apply_camera(S)
    end

    -- Viewport chip side effects (chips themselves live in center.luau).
    function S.on_chip(persona, key, value)
        if persona == "phys" and key == "sleep" then
            physics.set_sleeping_allowed(value)
        end
    end

    function S.attach_script()
        local node = S.doc[S.sel]
        if node.script ~= nil then
            return
        end
        local path = "scripts/" .. string.lower(node.name) .. ".luau"
        local abs = S.game_root .. "/" .. path
        if not fs.exists(abs) then
            fs.write(abs, "local " .. node.name .. " = {}\n\nfunction " .. node.name
                .. ":init()\nend\n\nfunction " .. node.name
                .. ":update(dt)\nend\n\nreturn " .. node.name .. "\n")
        end
        node.script = path
        S.dirty = true
        log.info("attached " .. path)
    end

    model.load(S)
    theme.apply()
    render.set_app_icon("assets/balaur-logo.png")
    ui.set_scale(S.ui_scale)
    render.set_grid(true, 1.0, 5, 20)
    Editor.apply_camera(S)
    log.info(`editing {S.game_name} ({#S.doc} nodes)`)

    -- Optional start-up state (persona id, "palette", "light", "play"),
    -- mirroring the design prototype's startPersona prop.
    local start = engine.args()[2]
    if start == "palette" then
        S.open_palette()
    elseif start == "light" then
        theme.set_dark(false)
    elseif start == "play" then
        S.toggle_play()
    elseif start == "rotdemo" then
        for i, node in S.doc do
            if node.name == "Spinner" then
                S.sel = i
            end
        end
        local _, ref = model.selected(S)
        ref:set_rotation_euler(math.rad(15), math.rad(30), 0)
        S.doc[S.sel].rotation_euler = { math.rad(15), math.rad(30), 0 }
        ref:set_scale(3, 3, 3)
        S.doc[S.sel].scale = { 3, 3, 3 }
        S.set_zoom(100)
    elseif start == "animdemo" then
        -- Self-test state: author a clip on the Spinner programmatically.
        for i, node in S.doc do
            if node.name == "Spinner" then
                S.sel = i
            end
        end
        local clip = anim.ensure_clip(S)
        clip.length = 2.0
        anim.add_key(S, 0.0)
        local _, ref = model.selected(S)
        ref:set_position(2.5, 2.5, 0)
        ref:set_rotation_euler(0, math.rad(180), 0)
        anim.add_key(S, 1.0)
        ref:set_position(2.5, 0.5, 0)
        anim.add_key(S, 2.0)
        S.anim_time = 0.6
        S.set_persona("anim")
    elseif start ~= nil and string.sub(start, 1, 7) == "select:" then
        -- Self-test state: select a node by name (e.g. select:Pig1).
        local name = string.sub(start, 8)
        for i, node in S.doc do
            if node.name == name then
                S.sel = i
            end
        end
    elseif start ~= nil and start ~= "" then
        S.set_persona(start)
        if start == "script" then
            for i, node in S.doc do
                if node.script ~= nil then
                    S.sel = i
                    break
                end
            end
        end
    end
end

function Editor.apply_camera(S)
    if model.is_2d(S) then
        -- Keep the current pan; zoom in logical px per world unit.
        local cx, cy = render.camera_2d()
        render.set_camera_2d(cx, cy, 55 * S.zoom / 100)
        return
    end
    local scale = 100 / S.zoom
    render.set_camera(
        TARGET[1] + (BASE_EYE[1] - TARGET[1]) * scale,
        TARGET[2] + (BASE_EYE[2] - TARGET[2]) * scale,
        TARGET[3] + (BASE_EYE[3] - TARGET[3]) * scale,
        TARGET[1], TARGET[2], TARGET[3]
    )
end

-- 2D grid, drawn as per-frame world-space lines over the visible extents.
local function draw_grid2d(S)
    local k = theme.current
    local cx, cy, zoom = render.camera_2d()
    local sw = (S.screen_w or 1280) * S.ui_scale
    local sh = (S.screen_h or 800) * S.ui_scale
    local half_w = sw / zoom / 2 + 1
    local half_h = sh / zoom / 2 + 1
    if half_w > 200 or half_h > 200 then
        return
    end
    local mr, mg, mb = theme.hex_to_rgb(k.grid)
    local jr, jg, jb = theme.hex_to_rgb(k.grid_major)
    for x = math.floor(cx - half_w), math.ceil(cx + half_w) do
        local r, g, b = mr, mg, mb
        if x % 5 == 0 then
            r, g, b = jr, jg, jb
        end
        render.draw_line_2d(x, cy - half_h, x, cy + half_h, r, g, b, if x == 0 then 2 else 1)
    end
    for y = math.floor(cy - half_h), math.ceil(cy + half_h) do
        local r, g, b = mr, mg, mb
        if y % 5 == 0 then
            r, g, b = jr, jg, jb
        end
        render.draw_line_2d(cx - half_w, y, cx + half_w, y, r, g, b, if y == 0 then 2 else 1)
    end
end

-- Axis guide lines through the selection (Guides chip).
local function draw_guides(S)
    local _, ref = model.selected(S)
    if ref == nil then
        return
    end
    local x, y, z = table.unpack(ref:global_position())
    local k = theme.current
    local ar, ag, ab = theme.hex_to_rgb(k.accent)
    local sr, sg, sb = theme.hex_to_rgb(k.sage)
    if model.is_2d(S) then
        render.draw_line_2d(x - 4, y, x + 4, y, ar, ag, ab, 1)
        render.draw_line_2d(x, y - 4, x, y + 4, sr, sg, sb, 1)
        return
    end
    local dr, dg, db = theme.hex_to_rgb(k.dim)
    render.draw_line(x - 4, y, z, x + 4, y, z, ar, ag, ab, 1)
    render.draw_line(x, y - 4, z, x, y + 4, z, sr, sg, sb, 1)
    render.draw_line(x, y, z - 4, x, y, z + 4, dr, dg, db, 1)
end

-- 2D collider outlines (Show colliders chip, phys persona).
local function draw_colliders2d(S)
    local k = theme.current
    local r, g, b = theme.hex_to_rgb(k.sage)
    for i, node in S.doc do
        local c = node.collider2d
        local ref = model.ref(S, i)
        if c ~= nil and ref ~= nil then
            local x, y = table.unpack(ref:global_position())
            local _, _, angle = table.unpack(ref:global_rotation_euler())
            local sx, sy = table.unpack(ref:global_scale())
            local ca, sa = math.cos(angle), math.sin(angle)
            local function pt(ox, oy)
                return x + ca * ox - sa * oy, y + sa * ox + ca * oy
            end
            if c.kind == "circle" then
                local radius = (c.radius or 0.5) * sx
                local px, py = pt(radius, 0)
                for t = 1, 32 do
                    local a = t / 32 * math.pi * 2
                    local nx, ny = pt(math.cos(a) * radius, math.sin(a) * radius)
                    render.draw_line_2d(px, py, nx, ny, r, g, b, 1)
                    px, py = nx, ny
                end
            else
                local he = c.half_extents or { 0.5, 0.5 }
                local hx, hy = he[1] * sx, he[2] * sy
                local x1, y1 = pt(-hx, -hy)
                local x2, y2 = pt(hx, -hy)
                local x3, y3 = pt(hx, hy)
                local x4, y4 = pt(-hx, hy)
                render.draw_line_2d(x1, y1, x2, y2, r, g, b, 1)
                render.draw_line_2d(x2, y2, x3, y3, r, g, b, 1)
                render.draw_line_2d(x3, y3, x4, y4, r, g, b, 1)
                render.draw_line_2d(x4, y4, x1, y1, r, g, b, 1)
            end
            -- Center of mass.
            render.draw_line_2d(x - 0.1, y, x + 0.1, y, r, g, b, 3)
            render.draw_line_2d(x, y - 0.1, x, y + 0.1, r, g, b, 3)
        end
    end
end

-- Collider wireboxes + center-of-mass dots (Show colliders chip).
local function draw_colliders(S)
    local k = theme.current
    local r, g, b = theme.hex_to_rgb(k.sage)
    for i, node in S.doc do
        local c = node.collider
        local ref = model.ref(S, i)
        if c ~= nil and ref ~= nil then
            local x, y, z = table.unpack(ref:global_position())
            local hx, hy, hz = 0.5, 0.5, 0.5
            if c.radius ~= nil then
                hx, hy, hz = c.radius, c.radius, c.radius
            elseif c.half_extents ~= nil then
                hx, hy, hz = c.half_extents[1], c.half_extents[2], c.half_extents[3]
            end
            local xs, ys, zs = { x - hx, x + hx }, { y - hy, y + hy }, { z - hz, z + hz }
            for _, yy in ys do
                for _, zz in zs do
                    render.draw_line(xs[1], yy, zz, xs[2], yy, zz, r, g, b, 1)
                end
            end
            for _, xx in xs do
                for _, zz in zs do
                    render.draw_line(xx, ys[1], zz, xx, ys[2], zz, r, g, b, 1)
                end
            end
            for _, xx in xs do
                for _, yy in ys do
                    render.draw_line(xx, yy, zs[1], xx, yy, zs[2], r, g, b, 1)
                end
            end
            -- Center of mass.
            render.draw_line(x - 0.15, y, z, x + 0.15, y, z, r, g, b, 3)
            render.draw_line(x, y - 0.15, z, x, y + 0.15, z, r, g, b, 3)
        end
    end
end

function Editor:update(dt)
    local S = self.S
    if S.refs ~= nil and S.refs[S.sel] == nil then
        -- Mirror was rebuilt last frame; re-resolve node handles.
        for i, node in S.doc do
            S.refs[i] = S.mirror:get_node(model.node_path(S.doc, node))
        end
    end
    -- 2D/3D viewport mode follows the edited scene's components.
    local is2d = model.is_2d(S)
    if is2d ~= S.mode2d then
        S.mode2d = is2d
        render.set_grid(not is2d, 1.0, 5, 20)
        Editor.apply_camera(S)
    end
    if is2d then
        draw_grid2d(S)
    end
    gizmo.update(S)
    anim.update(S, dt)
    -- Scene-tree UI widgets draw during play (and for preview in the
    -- Interface persona), clipped to the viewport.
    local rect = gizmo.viewport_rect(S)
    local widgets_on = S.playing or S.persona == "ui"
    ui.set_widget_layer(widgets_on, rect.x, rect.y, rect.w, rect.h)
    if S.tool == "key" and S.persona == "anim" and input.mouse_just_pressed(1) then
        anim.add_key(S, S.anim_time)
    end
    gizmo.draw(S)
    local chips = S.chip_state[S.persona] or {}
    if S.persona == "scene" and chips.guides then
        draw_guides(S)
    end
    if S.persona == "phys" and chips.colliders ~= false then
        if S.mode2d then
            draw_colliders2d(S)
        else
            draw_colliders(S)
        end
    end
end

local function save_active(S)
    -- cmd+S saves the visible script buffer when the code editor is open,
    -- the scene document otherwise.
    local node = S.doc[S.sel]
    local in_script = (S.persona == "script" or S.doctab == "script") and node.script ~= nil
    if in_script then
        local abs = S.game_root .. "/" .. node.script
        local buffer = S.script_buffers[abs]
        if buffer ~= nil then
            fs.write(abs, buffer)
            S.script_dirty[abs] = nil
            -- Reload only matters when the script is live (playing); outside
            -- of that it was never loaded, which is fine.
            pcall(engine.reload_script, abs)
            log.info("saved " .. node.script)
        end
        return
    end
    model.save(S)
end

function Editor:draw_ui()
    local S = self.S
    local sw, sh = ui.screen_size()
    S.screen_w, S.screen_h = sw, sh
    S.compact = sw < 1240

    -- Global shortcuts.
    if ui.shortcut("cmd", "K") then
        if S.palette then S.close_palette() else S.open_palette() end
    end
    if ui.shortcut("cmd", "S") then
        save_active(S)
    end
    if ui.shortcut("cmd", "Equals") then
        S.set_ui_scale(S.ui_scale + 0.125)
    end
    if ui.shortcut("cmd", "Minus") then
        S.set_ui_scale(S.ui_scale - 0.125)
    end
    if not ui.wants_keyboard() then
        if ui.shortcut("none", "F5") then
            S.toggle_play()
        end
        local personas = { "scene", "script", "anim", "phys", "ui" }
        for i, id in personas do
            if ui.shortcut("alt", tostring(i)) then
                S.set_persona(id)
            end
        end
        if ui.shortcut("shift", "A") then
            model.add_child(S)
        end
        if ui.shortcut("alt", "L") then
            theme.set_dark(not theme.current.dark)
        end
        if S.persona == "anim" and ui.shortcut("none", "K") then
            anim.add_key(S, S.anim_time)
        end
    end

    -- Fixed skeleton: outer chrome, then dock, rail, tabs, then the center.
    chrome.persona_bar(S)
    chrome.status_bar(S)
    left.draw(S)
    inspector.draw(S)
    dock.draw(S)
    center.draw(S)
    palette.draw(S)
end

Editor.save_active = save_active

function Editor:hot_reload()
    theme.apply()
end

return Editor
