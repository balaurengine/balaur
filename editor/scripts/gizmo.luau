-- Selection gizmo in the steadyum style: an orange padded bounding-box
-- frame whose faces translate, corner handles scale, and three axis rings
-- rotate; the element under the cursor (or being dragged) tints deep blue.
-- All hit-testing and drag math is screen-space Luau over the camera pose
-- the backend publishes; the rail tools remain as drag-anywhere fallbacks.
local model = require("scripts/model")

local M = {}

-- steadyum's selection palette: orange frame, deep-blue hover.
local ORANGE = { 1.0, 0.647, 0.0 }
local HOVER = { 0.118, 0.314, 0.784 }

local function vsub(a, b) return { a[1] - b[1], a[2] - b[2], a[3] - b[3] } end
local function vadd(a, b) return { a[1] + b[1], a[2] + b[2], a[3] + b[3] } end
local function vscale(a, s) return { a[1] * s, a[2] * s, a[3] * s } end
local function vdot(a, b) return a[1] * b[1] + a[2] * b[2] + a[3] * b[3] end
local function vcross(a, b)
    return {
        a[2] * b[3] - a[3] * b[2],
        a[3] * b[1] - a[1] * b[3],
        a[1] * b[2] - a[2] * b[1],
    }
end
local function vnorm(a)
    local l = math.sqrt(vdot(a, a))
    if l < 1e-6 then
        return { 0, 0, 0 }, 0
    end
    return vscale(a, 1 / l), l
end

local AXES = { { 1, 0, 0 }, { 0, 1, 0 }, { 0, 0, 1 } }

-- The 3D viewport rectangle in design px (matches the panel layout).
-- Uses the screen size cached by draw_ui (ui.* is pass-scoped).
function M.viewport_rect(S)
    local sw, sh = S.screen_w or 1280, S.screen_h or 800
    local dock_h = if S.dock == "timeline" then 212 else 150
    return { x = 262 + 46, y = 56 + 38, w = sw - 308 - 262 - 46, h = sh - 28 - dock_h - 56 - 38 }
end

-- Camera basis + world-units-per-design-px at the target's depth. The 3D
-- scene renders across the whole window (panels overlay it), so projection
-- maps NDC to the full screen, not the visible viewport region.
local function camera_frame(S)
    local ex, ey, ez, tx, ty, tz, fov, dpi = render.camera_pose()
    local eye = { ex, ey, ez }
    local forward, dist = vnorm(vsub({ tx, ty, tz }, eye))
    local right = vnorm(vcross(forward, { 0, 1, 0 }))
    local up = vcross(right, forward)
    local sw, sh = S.screen_w or 1280, S.screen_h or 800
    local wpp = 2 * dist * math.tan(fov / 2) / math.max(sh, 1)
    return {
        eye = eye, forward = forward, right = right, up = up,
        dist = dist, fov = fov, dpi = dpi, wpp = wpp,
        rect = { x = 0, y = 0, w = sw, h = sh },
        vp = M.viewport_rect(S),
    }
end

local function mouse_design_px(S, cam)
    local mx, my = input.mouse_position()
    return mx / cam.dpi / S.ui_scale, my / cam.dpi / S.ui_scale
end

local function in_viewport(S, cam, mx, my)
    local r = cam.vp
    -- Leave the chip row and the bottom pill row to egui.
    return mx > r.x and mx < r.x + r.w and my > r.y + 44 and my < r.y + r.h - 40
end

-- Project a world point to design-px screen coordinates (nil if behind).
local function project(S, cam, p)
    local v = vsub(p, cam.eye)
    local z = vdot(v, cam.forward)
    if z < 0.01 then
        return nil
    end
    local tan_half = math.tan(cam.fov / 2)
    local r = cam.rect
    local aspect = r.w / r.h
    local ndc_x = vdot(v, cam.right) / (z * tan_half * aspect)
    local ndc_y = vdot(v, cam.up) / (z * tan_half)
    return r.x + r.w * (0.5 + ndc_x / 2), r.y + r.h * (0.5 - ndc_y / 2)
end

-- Selection geometry: center, padded half-extents, handle size, ring radius.
local function geometry(S)
    local node, ref = model.selected(S)
    if ref == nil then
        return nil
    end
    local x, y, z = ref:global_position()
    local hx, hy, hz = 0.6, 0.6, 0.6
    local shape = node.shape or node.collider
    if shape ~= nil then
        if shape.radius ~= nil and (shape.kind == "ball" or shape.shape == "ball") then
            hx, hy, hz = shape.radius, shape.radius, shape.radius
        elseif shape.half_extents ~= nil then
            hx, hy, hz = shape.half_extents[1], shape.half_extents[2], shape.half_extents[3]
        end
    end
    local sx, sy, sz = ref:scale()
    hx, hy, hz = hx * sx + 0.02, hy * sy + 0.02, hz * sz + 0.02
    local min_he = math.min(hx, hy, hz)
    -- steadyum: handle size scales with camera distance (constant on
    -- screen), rings sit at 75% of the smallest extent.
    local ex, ey, ez = render.camera_pose()
    local dist = math.sqrt((x - ex) ^ 2 + (y - ey) ^ 2 + (z - ez) ^ 2)
    local marker = 0.6 * dist * 0.0144
    return {
        center = { x, y, z },
        he = { hx, hy, hz },
        ring = math.max(min_he * 0.75, marker * 2.5),
        marker = marker,
    }
end

local function corners(geo)
    local out = {}
    for _, sx in { -1, 1 } do
        for _, sy in { -1, 1 } do
            for _, sz in { -1, 1 } do
                table.insert(out, {
                    geo.center[1] + sx * geo.he[1],
                    geo.center[2] + sy * geo.he[2],
                    geo.center[3] + sz * geo.he[3],
                    signs = { sx, sy, sz },
                })
            end
        end
    end
    return out
end

local function faces(geo)
    local out = {}
    for axis = 1, 3 do
        for _, sign in { -1, 1 } do
            local p = { geo.center[1], geo.center[2], geo.center[3] }
            p[axis] += sign * geo.he[axis]
            table.insert(out, { pos = p, axis = axis, sign = sign })
        end
    end
    return out
end

local function ring_point(geo, axis, t)
    local angle = t * math.pi * 2
    local u = AXES[axis % 3 + 1]
    local v = AXES[(axis + 1) % 3 + 1]
    return vadd(geo.center, vadd(vscale(u, math.cos(angle) * geo.ring), vscale(v, math.sin(angle) * geo.ring)))
end

-- What the cursor is over: {kind="handle"/"ring"/"face", index/axis, ...}.
local function hit_test(S, cam, geo, mx, my)
    for i, corner in corners(geo) do
        local sx, sy = project(S, cam, corner)
        if sx ~= nil and (sx - mx) ^ 2 + (sy - my) ^ 2 < 14 ^ 2 then
            return { kind = "handle", index = i }
        end
    end
    for axis = 1, 3 do
        for t = 0, 47 do
            local sx, sy = project(S, cam, ring_point(geo, axis, t / 48))
            if sx ~= nil and (sx - mx) ^ 2 + (sy - my) ^ 2 < 9 ^ 2 then
                return { kind = "ring", axis = axis }
            end
        end
    end
    for i, face in faces(geo) do
        local sx, sy = project(S, cam, face.pos)
        if sx ~= nil and (sx - mx) ^ 2 + (sy - my) ^ 2 < 16 ^ 2 then
            return { kind = "face", index = i, axis = face.axis }
        end
    end
    return nil
end

local function line(a, b, color, width)
    render.draw_line(a[1], a[2], a[3], b[1], b[2], b[3], color[1], color[2], color[3], width)
end

-- Corner marker in steadyum's open-bracket style: of the mini-cube's 12
-- edges, draw only those with at least one outward face (faces whose sign
-- matches the handle's corner sign), leaving an open bracket.
local EDGE_AXES = { { 2, 3 }, { 1, 3 }, { 1, 2 } }

local function draw_handle_marker(corner, signs, half, color)
    for axis = 1, 3 do
        local b_axis, c_axis = EDGE_AXES[axis][1], EDGE_AXES[axis][2]
        for _, b_sign in { -1, 1 } do
            for _, c_sign in { -1, 1 } do
                local b_inner = b_sign == -signs[b_axis]
                local c_inner = c_sign == -signs[c_axis]
                if not (b_inner and c_inner) then
                    local a = { corner[1], corner[2], corner[3] }
                    local b = { corner[1], corner[2], corner[3] }
                    a[axis] -= half
                    b[axis] += half
                    a[b_axis] += b_sign * half
                    b[b_axis] += b_sign * half
                    a[c_axis] += c_sign * half
                    b[c_axis] += c_sign * half
                    line(a, b, color, 6)
                end
            end
        end
    end
end

-- Which gizmo part is "hot" (dragged part wins over hover).
local function hot_part(S)
    if S.gizmo_drag ~= nil then
        return S.gizmo_drag.part
    end
    return S.gizmo_hover
end

-- Draw the frame, handles and rings (steadyum: orange, hot element blue).
function M.draw(S)
    local geo = geometry(S)
    if geo == nil or S.persona == "script" then
        return
    end
    local hot = hot_part(S)
    local c, he = geo.center, geo.he
    local xs = { c[1] - he[1], c[1] + he[1] }
    local ys = { c[2] - he[2], c[2] + he[2] }
    local zs = { c[3] - he[3], c[3] + he[3] }
    local hot_face_axis = if hot ~= nil and hot.kind == "face" then hot.axis else nil
    local function edge_color(axis_a, axis_b)
        -- A face's four edges are those not along its normal axis.
        if hot_face_axis ~= nil and axis_a ~= hot_face_axis and axis_b ~= hot_face_axis then
            return HOVER
        end
        return ORANGE
    end
    for _, y in ys do
        for _, z in zs do
            line({ xs[1], y, z }, { xs[2], y, z }, edge_color(2, 3), 6)
        end
    end
    for _, x in xs do
        for _, z in zs do
            line({ x, ys[1], z }, { x, ys[2], z }, edge_color(1, 3), 6)
        end
    end
    for _, x in xs do
        for _, y in ys do
            line({ x, y, zs[1] }, { x, y, zs[2] }, edge_color(1, 2), 6)
        end
    end
    for i, corner in corners(geo) do
        local hot_handle = hot ~= nil and hot.kind == "handle" and hot.index == i
        draw_handle_marker(corner, corner.signs, geo.marker, if hot_handle then HOVER else ORANGE)
    end
    for axis = 1, 3 do
        local hot_ring = hot ~= nil and hot.kind == "ring" and hot.axis == axis
        local color = if hot_ring then HOVER else ORANGE
        local prev = ring_point(geo, axis, 0)
        for t = 1, 48 do
            local next = ring_point(geo, axis, t / 48)
            line(prev, next, color, 6)
            prev = next
        end
    end
end

-- Screen direction (design px) of a world axis at the gizmo center.
local function screen_axis(S, cam, center, axis)
    local ax, ay = project(S, cam, center)
    local bx, by = project(S, cam, vadd(center, AXES[axis]))
    if ax == nil or bx == nil then
        return nil
    end
    return { bx - ax, by - ay }
end

local function start_gizmo_drag(S, cam, geo, part, mx, my)
    local node, ref = model.selected(S)
    local px, py, pz = ref:position()
    local sx, sy, sz = ref:scale()
    local rot = node.rotation_euler or { 0, 0, 0 }
    local cx, cy = project(S, cam, geo.center)
    S.gizmo_drag = {
        part = part,
        pos = { px, py, pz },
        scale = { sx, sy, sz },
        rot = { rot[1], rot[2], rot[3] },
        mouse = { mx, my },
        center = { cx, cy },
    }
end

local function apply_gizmo_drag(S, cam, mx, my)
    local d = S.gizmo_drag
    local node, ref = model.selected(S)
    if ref == nil then
        return
    end
    if d.part.kind == "handle" then
        -- Uniform scale by radial distance from the projected center.
        local d0 = math.max(8, math.sqrt((d.mouse[1] - d.center[1]) ^ 2 + (d.mouse[2] - d.center[2]) ^ 2))
        local d1 = math.sqrt((mx - d.center[1]) ^ 2 + (my - d.center[2]) ^ 2)
        local f = math.clamp(d1 / d0, 0.05, 20)
        local sx, sy, sz = d.scale[1] * f, d.scale[2] * f, d.scale[3] * f
        ref:set_scale(sx, sy, sz)
        node.scale = { sx, sy, sz }
        S.dirty = true
    elseif d.part.kind == "ring" then
        -- Screen-angle delta around the projected center, signed by whether
        -- the ring axis points toward the camera.
        local a0 = math.atan(d.mouse[2] - d.center[2], d.mouse[1] - d.center[1])
        local a1 = math.atan(my - d.center[2], mx - d.center[1])
        local axis = AXES[d.part.axis]
        local geo_center = { d.pos[1], d.pos[2], d.pos[3] }
        local toward = vdot(axis, vsub(cam.eye, geo_center))
        local delta = (a1 - a0) * (if toward >= 0 then -1 else 1)
        local rot = { d.rot[1], d.rot[2], d.rot[3] }
        rot[d.part.axis] += delta
        node.rotation_euler = rot
        ref:set_rotation_euler(rot[1], rot[2], rot[3])
        S.dirty = true
    elseif d.part.kind == "face" then
        -- Grab a face and slide the object in that face's plane.
        local normal = d.part.axis
        local center = { d.pos[1], d.pos[2], d.pos[3] }
        local dx, dy = mx - d.mouse[1], my - d.mouse[2]
        local pos = { d.pos[1], d.pos[2], d.pos[3] }
        for axis = 1, 3 do
            if axis ~= normal then
                local dir = screen_axis(S, cam, center, axis)
                if dir ~= nil then
                    local len2 = dir[1] ^ 2 + dir[2] ^ 2
                    if len2 > 1e-6 then
                        pos[axis] += (dx * dir[1] + dy * dir[2]) / len2
                    end
                end
            end
        end
        local snap_on = (S.chip_state.scene or {}).snap
        if snap_on then
            for axis = 1, 3 do
                if axis ~= normal then
                    pos[axis] = math.round(pos[axis] * 2) / 2
                end
            end
        end
        ref:set_position(pos[1], pos[2], pos[3])
        node.position = { pos[1], pos[2], pos[3] }
        S.dirty = true
    end
end

local MANIP = { move = true, rotate = true, scale = true, zoom = true }

-- Runs every frame from Editor:update.
function M.update(S)
    local cam = camera_frame(S)
    local mx, my = mouse_design_px(S, cam)
    local geo = geometry(S)
    local over_viewport = in_viewport(S, cam, mx, my)

    -- Hover state (skipped while a drag is latched).
    if S.gizmo_drag == nil then
        S.gizmo_hover = if geo ~= nil and over_viewport and S.persona ~= "script"
            then hit_test(S, cam, geo, mx, my)
            else nil
    end

    -- The camera keeps the pointer unless a manipulation tool is active or
    -- the gizmo itself is hot.
    render.set_camera_input(not MANIP[S.tool] and S.gizmo_hover == nil and S.gizmo_drag == nil)

    if input.mouse_just_pressed(1) and over_viewport then
        if S.gizmo_hover ~= nil and geo ~= nil then
            start_gizmo_drag(S, cam, geo, S.gizmo_hover, mx, my)
        else
            S.drag_in_viewport = true
            if S.tool == "select" then
                local best, best_d = nil, 40
                for i in S.doc do
                    local ref = model.ref(S, i)
                    if ref ~= nil then
                        local x, y, z = ref:global_position()
                        local px, py = project(S, cam, { x, y, z })
                        if px ~= nil then
                            local dist = math.sqrt((px - mx) ^ 2 + (py - my) ^ 2)
                            if dist < best_d then
                                best, best_d = i, dist
                            end
                        end
                    end
                end
                if best ~= nil then
                    S.select(best)
                end
            end
        end
    end
    if not input.is_mouse_down(1) then
        S.gizmo_drag = nil
        S.drag_in_viewport = false
    end

    if S.gizmo_drag ~= nil then
        apply_gizmo_drag(S, cam, mx, my)
        return
    end
    if not S.drag_in_viewport then
        return
    end

    -- Rail-tool fallbacks: drag anywhere in the viewport.
    local dx, dy = input.mouse_delta()
    dx /= cam.dpi * S.ui_scale
    dy /= cam.dpi * S.ui_scale
    if dx == 0 and dy == 0 then
        return
    end
    local node, ref = model.selected(S)
    if S.tool == "move" and ref ~= nil then
        local x, y, z = ref:position()
        if input.is_down("LShift") or input.is_down("RShift") then
            y -= dy * cam.wpp
        else
            local flat = vnorm({ cam.forward[1], 0, cam.forward[3] })
            local d = vadd(vscale(cam.right, dx * cam.wpp), vscale(flat, -dy * cam.wpp))
            x += d[1]
            z += d[3]
        end
        if (S.chip_state.scene or {}).snap then
            x = math.round(x * 2) / 2
            y = math.round(y * 2) / 2
            z = math.round(z * 2) / 2
        end
        ref:set_position(x, y, z)
        node.position = { x, y, z }
        S.dirty = true
    elseif S.tool == "rotate" and ref ~= nil then
        local r = node.rotation_euler or { 0, 0, 0 }
        r[2] += dx * 0.01
        node.rotation_euler = r
        ref:set_rotation_euler(r[1], r[2], r[3])
        S.dirty = true
    elseif S.tool == "scale" and ref ~= nil then
        local sx, sy, sz = ref:scale()
        local f = math.clamp(1 - dy * 0.01, 0.2, 5)
        sx, sy, sz = sx * f, sy * f, sz * f
        ref:set_scale(sx, sy, sz)
        node.scale = { sx, sy, sz }
        S.dirty = true
    elseif S.tool == "zoom" then
        S.set_zoom(S.zoom - dy * 0.5)
    end
end

M.project = project
M.camera_frame = camera_frame

return M
