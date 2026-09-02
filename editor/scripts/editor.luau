-- The balaur editor: a regular balaur project that drives the whole shell
-- through the `ui` module and edits the game passed on the command line
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
local rig = require("scripts/rig")
local polygonlib = require("scripts/polygon")
local history = require("scripts/history")
local selftest = require("scripts/selftest")
local highlight = require("scripts/highlight")

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
        breakpoints = {},
        debug_frame = 1,
    }
    self.S = S
    _G.ED = S

    function S.select(i)
        if i ~= S.sel then
            history.commit(S)
        end
        S.sel = i
    end

    -- History owns the document; the mirror is rebuilt from it here, because
    -- history must not depend on `model` (model records into history).
    local function step(label, moved)
        if moved == nil then
            log.info(`nothing to {label}`)
            return
        end
        -- Rebuilding stops play, so read the flag before it is cleared: a
        -- keystroke aimed at the document should not silently kill the game.
        local was_playing = S.playing
        model.build_mirror(S)
        if was_playing then
            log.info(`{label} stopped the running scene`)
        end
        theme.apply()
        Editor.apply_camera(S)
        log.info(`{label}: {moved}`)
    end

    function S.undo()
        step("undo", history.undo(S))
    end

    function S.redo()
        step("redo", history.redo(S))
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
            -- The mirror was built with every player held (model.build_mirror);
            -- play is what lets the engine advance them.
            anim.resume_all(S)
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

    -- The other half of the pair: chips that command the engine read their
    -- state back from it, so nothing is mirrored in `S.chip_state`. Returns
    -- nil for a chip the editor owns outright.
    function S.chip_value(persona, key)
        if persona == "phys" and key == "sleep" then
            return physics.sleeping_allowed()
        end
        return nil
    end

    -- Breakpoints are kept per game-relative script path; the host keys the
    -- same file by the absolute path play attaches it under.
    function S.breakpoint_lines(rel)
        local lines = {}
        for line in pairs(S.breakpoints[rel] or {}) do
            table.insert(lines, line)
        end
        table.sort(lines)
        return lines
    end

    function S.toggle_breakpoint(rel, line)
        local set = S.breakpoints[rel] or {}
        set[line] = if set[line] then nil else true
        S.breakpoints[rel] = set
        -- The host reports where each request landed: a blank line moves
        -- to the next line with code.
        local landed = debugger.set_breakpoints(S.game_root .. "/" .. rel, S.breakpoint_lines(rel))
        local kept = {}
        for _, l in landed do
            kept[l] = true
        end
        S.breakpoints[rel] = kept
    end

    -- Game-relative path of a script the host reports by absolute key.
    function S.script_rel(path)
        local prefix = S.game_root .. "/"
        if string.sub(path, 1, #prefix) == prefix then
            return string.sub(path, #prefix + 1)
        end
        return path
    end

    -- The pause is over the moment it is asked to go; the poll would only
    -- notice next frame, and a pause that follows at once must read as new.
    function S.resume(mode)
        debugger.resume(mode)
        S.paused = nil
    end

    -- A fresh pause: show the script and the frame it stopped in.
    function S.on_pause(p)
        S.paused_rel = S.script_rel(p.path)
        S.debug_frame = 1
        for i, node in S.doc do
            local ref = S.refs and S.refs[i]
            if (ref ~= nil and ref == p.node) or node.script == S.paused_rel then
                S.select(i)
                break
            end
        end
        S.set_persona("script")
        S.dock = "debugger"
        log.info(`paused at {S.paused_rel}:{p.line} ({p.reason})`)
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
        history.record(S, "attach script")
        model.forget()
        node.script = path
        log.info("attached " .. path .. " (undo detaches it; the file stays)")
    end

    model.load(S)
    theme.apply()
    render.set_app_icon("assets/balaur-logo.png")
    ui.set_scale(S.ui_scale)
    render.set_grid(true, 1.0, 5, 20)
    Editor.apply_camera(S)
    log.info(`editing {S.game_name} ({#S.doc} nodes)`)

    -- Optional start-up states, comma-separated (persona id, "palette",
    -- "light", "play", "select:<node>", "shot=<png path>"), applied in order.
    for token in string.gmatch(engine.args()[2] or "", "[^,]+") do
        Editor.apply_start_state(S, token)
    end
end

-- One start-up state, mirroring the design prototype's startPersona prop.
function Editor.apply_start_state(S, start)
    if string.sub(start, 1, 5) == "shot=" then
        -- Screenshot the shell once the first frames have laid it out, for
        -- a visual check of an editor state a headless run cannot show.
        S.shot = string.sub(start, 6)
    elseif start == "palette" then
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
        -- Self-test state: author a clip on the Spinner when the mirror resolved
        -- it, otherwise on any resolved node — keying reads a live transform.
        local pick
        for i, node in S.doc do
            if model.ref(S, i) ~= nil and (pick == nil or node.name == "Spinner") then
                pick = i
            end
        end
        if pick == nil then
            log.warn("animdemo: the mirror resolved no node to animate")
        else
            S.sel = pick
            anim.ensure_clip(S)
            anim.add_key(S, 0.0)
            local _, ref = model.selected(S)
            ref:set_position(2.5, 2.5, 0)
            ref:set_rotation_euler(0, math.rad(180), 0)
            anim.add_key(S, 1.0)
            ref:set_position(2.5, 0.5, 0)
            anim.add_key(S, 2.0)
            anim.seek(S, 0.6)
        end
        S.set_persona("anim")
    elseif start == "undodemo" then
        selftest.undo(S)
    elseif start == "breakdemo" then
        Editor.break_demo(S)
    elseif start == "rigdemo" then
        selftest.rig(S)
    elseif start == "polydemo" then
        selftest.polygon(S)
    elseif string.sub(start, 1, 5) == "tool:" then
        S.tool = string.sub(start, 6)
    elseif string.sub(start, 1, 5) == "mode:" then
        polygonlib.set_mode(S, string.sub(start, 6))
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

-- Self-test state: a breakpoint on the first scripted node's update, then
-- play; `Editor:update` alternates a step and a continue on every pause.
function Editor.break_demo(S)
    for i, node in S.doc do
        if node.script ~= nil then
            S.sel = i
            local _, hooks = highlight.analyze(model.source(S, node.script) or "")
            for _, hook in hooks do
                if hook.name == "update" then
                    S.toggle_breakpoint(node.script, hook.line + 1)
                end
            end
            break
        end
    end
    S.breakdemo = 0
    S.toggle_play()
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

-- The selected clip's position keys, joined in order, with a marker on each
-- (Motion path chip, anim persona). The chip used to toggle nothing at all.
local function draw_motion_path(S)
    local def = anim.definition(S)
    if def == nil then
        return
    end
    local keys
    for _, track in anim.tracks(def) do
        if (track.target or "") == "" and track.property == "position" then
            keys = track.keys
        end
    end
    if keys == nil or #keys < 1 then
        return
    end
    local k = theme.current
    local r, g, b = theme.hex_to_rgb(k.accent)
    local is2d = model.is_2d(S)
    local previous
    for _, key in keys do
        local v = key.value or { 0, 0, 0 }
        local x, y, z = v[1] or 0, v[2] or 0, v[3] or 0
        if previous ~= nil then
            if is2d then
                render.draw_line_2d(previous[1], previous[2], x, y, r, g, b, 1)
            else
                render.draw_line(previous[1], previous[2], previous[3], x, y, z, r, g, b, 1)
            end
        end
        local d = 0.15
        if is2d then
            render.draw_line_2d(x - d, y, x + d, y, r, g, b, 2)
            render.draw_line_2d(x, y - d, x, y + d, r, g, b, 2)
        else
            render.draw_line(x - d, y, z, x + d, y, z, r, g, b, 2)
            render.draw_line(x, y - d, z, x, y + d, z, r, g, b, 2)
        end
        previous = { x, y, z }
    end
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
        local c = node.collider3d
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
    -- Smoothed frame rate for the status bar; a raw 1/dt reads as noise.
    if dt > 0 then
        S.fps = (S.fps or 1 / dt) * 0.9 + (1 / dt) * 0.1
    end
    S.frame = (S.frame or 0) + 1
    if S.shot ~= nil and S.frame == 60 then
        render.screenshot(S.shot)
    end
    local paused = debugger.paused()
    if paused ~= nil and S.paused == nil then
        S.on_pause(paused)
    end
    S.paused = paused
    if S.breakdemo ~= nil and S.paused ~= nil then
        S.breakdemo += 1
        S.resume(if S.breakdemo % 2 == 1 then debugger.STEP_OVER else debugger.CONTINUE)
    end
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
    anim.update(S)
    -- Scene-tree UI widgets draw during play (and for preview in the
    -- Interface persona), clipped to the viewport.
    local rect = gizmo.viewport_rect(S)
    local widgets_on = S.playing or S.persona == "ui"
    ui.set_widget_layer(widgets_on, rect.x, rect.y, rect.w, rect.h)
    if S.tool == "key" and S.persona == "anim" and input.mouse_just_pressed(input.MOUSE_LEFT) then
        anim.add_key(S, S.anim_time)
    end
    gizmo.draw(S)
    rig.draw(S)
    polygonlib.idle(S)
    polygonlib.draw(S)
    local chips = S.chip_state[S.persona] or {}
    if S.persona == "scene" and chips.guides then
        draw_guides(S)
    end
    if S.persona == "anim" and chips.path ~= false then
        draw_motion_path(S)
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
            model.forget(node.script)
            S.script_dirty[abs] = nil
            -- Reload only matters when the script is live (playing); outside
            -- of that it was never loaded, which is fine.
            pcall(engine.reload_script, abs)
            log.info("saved " .. node.script)
        end
        return
    end
    -- An external clip is its own file; an inline one rides along with the
    -- scene. `anim.save` knows which it is looking at.
    anim.save(S)
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
    -- Debugger keys work with the code editor focused: they are not text.
    if S.paused ~= nil then
        if ui.shortcut("none", "F5") then
            S.resume(debugger.CONTINUE)
        elseif ui.shortcut("none", "F10") then
            S.resume(debugger.STEP_OVER)
        elseif ui.shortcut("none", "F11") then
            S.resume(debugger.STEP_INTO)
        elseif ui.shortcut("shift", "F11") then
            S.resume(debugger.STEP_OUT)
        end
    end
    if not ui.wants_keyboard() then
        if S.paused == nil and ui.shortcut("none", "F5") then
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
        -- Gated with the rest, so typing in the code editor keeps egui's own
        -- per-widget undo; redo first, to claim the shifted chord.
        if ui.shortcut("shift+cmd", "Z") or ui.shortcut("cmd", "Y") then
            S.redo()
        elseif ui.shortcut("cmd", "Z") then
            S.undo()
        end
        if ui.shortcut("alt", "L") then
            theme.set_dark(not theme.current.dark)
        end
        if S.persona == "anim" and ui.shortcut("none", "K") then
            anim.add_key(S, S.anim_time)
        end
        if ui.shortcut("none", "Escape") then
            polygonlib.cancel(S)
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
