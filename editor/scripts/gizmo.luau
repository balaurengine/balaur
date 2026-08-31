-- Selection gizmo, steadyum style: an orange bbox frame (faces translate,
-- corners scale, axis rings rotate), hover tints deep blue. All hit-testing is
-- screen-space Luau over the camera pose the backend publishes.
local model = require("scripts/model")
local gizmo2d = require("scripts/gizmo2d")

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

-- Rotation basis from the engine's ZYX euler (roll=x, pitch=y, yaw=z):
-- R = Rz(yaw) * Ry(pitch) * Rx(roll). Returns the world-space directions of
-- the node's local axes (the matrix columns).
local function basis_from_euler(r)
    local cr, sr = math.cos(r[1]), math.sin(r[1])
    local cp, sp = math.cos(r[2]), math.sin(r[2])
    local cy, sy = math.cos(r[3]), math.sin(r[3])
    return {
        { cy * cp, sy * cp, -sp },
        { cy * sp * sr - sy * cr, sy * sp * sr + cy * cr, cp * sr },
        { cy * sp * cr + sy * sr, sy * sp * cr - cy * sr, cp * cr },
    }
end

-- Euler (roll, pitch, yaw) back from a basis (columns bx, by, bz).
local function euler_from_basis(b)
    local pitch = math.asin(math.clamp(-b[1][3], -1, 1))
    local yaw = math.atan(b[1][2], b[1][1])
    local roll = math.atan(b[2][3], b[3][3])
    return { roll, pitch, yaw }
end

-- basis * elementary rotation about local axis `axis` by `angle`.
local function rotate_basis_local(b, axis, angle)
    local c, s = math.cos(angle), math.sin(angle)
    local i = axis % 3 + 1
    local j = (axis + 1) % 3 + 1
    -- Columns i and j rotate into each other around column `axis`.
    local bi, bj = b[i], b[j]
    local out = { b[1], b[2], b[3] }
    out[i] = { bi[1] * c + bj[1] * s, bi[2] * c + bj[2] * s, bi[3] * c + bj[3] * s }
    out[j] = { bj[1] * c - bi[1] * s, bj[2] * c - bi[2] * s, bj[3] * c - bi[3] * s }
    return out
end

-- center + basis-space offset -> world point.
local function local_point(geo, ox, oy, oz)
    local b = geo.basis
    return {
        geo.center[1] + b[1][1] * ox + b[2][1] * oy + b[3][1] * oz,
        geo.center[2] + b[1][2] * ox + b[2][2] * oy + b[3][2] * oz,
        geo.center[3] + b[1][3] * ox + b[2][3] * oy + b[3][3] * oz,
    }
end

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
    local rox, roy, roz, rdx, rdy, rdz = render.mouse_ray()
    return {
        eye = eye, forward = forward, right = right, up = up,
        dist = dist, fov = fov, dpi = dpi, wpp = wpp,
        -- The renderer's exact projection*view (column-major).
        m = render.camera_matrix(),
        ray_o = { rox, roy, roz },
        ray_d = { rdx, rdy, rdz },
        rect = { x = 0, y = 0, w = sw, h = sh },
        vp = M.viewport_rect(S),
    }
end

-- Intersect the current mouse ray with a plane (point p, normal n).
local function ray_plane(cam, p, n)
    local denom = vdot(cam.ray_d, n)
    if math.abs(denom) < 1e-6 then
        return nil
    end
    local t = vdot(vsub(p, cam.ray_o), n) / denom
    if t < 0 then
        return nil
    end
    return vadd(cam.ray_o, vscale(cam.ray_d, t))
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

-- Project a world point to design-px screen coordinates using the
-- renderer's exact projection*view matrix (nil if behind the camera).
local function project(S, cam, p)
    local m = cam.m
    local x, y, z = p[1], p[2], p[3]
    local cw = m[4] * x + m[8] * y + m[12] * z + m[16]
    if cw < 1e-4 then
        return nil
    end
    local cx = (m[1] * x + m[5] * y + m[9] * z + m[13]) / cw
    local cy = (m[2] * x + m[6] * y + m[10] * z + m[14]) / cw
    local r = cam.rect
    return r.w * (0.5 + cx * 0.5), r.h * (0.5 - cy * 0.5)
end

-- Selection geometry: center, padded half-extents, handle size, ring radius.
local function geometry(S)
    local node, ref = model.selected(S)
    if ref == nil then
        return nil
    end
    local x, y, z = table.unpack(ref:global_position())
    local hx, hy, hz = 0.6, 0.6, 0.6
    local shape = node.shape or node.collider
    if shape ~= nil then
        if shape.radius ~= nil and shape.kind == "ball" then
            hx, hy, hz = shape.radius, shape.radius, shape.radius
        elseif shape.half_extents ~= nil then
            hx, hy, hz = shape.half_extents[1], shape.half_extents[2], shape.half_extents[3]
        end
    end
    -- The renderer sizes and orients by the GLOBAL transform (parents may
    -- carry scale/rotation), so the gizmo must too.
    local sx, sy, sz = table.unpack(ref:global_scale())
    hx, hy, hz = hx * sx + 0.06, hy * sy + 0.06, hz * sz + 0.06
    local min_he = math.min(hx, hy, hz)
    -- steadyum: handle size scales with camera distance (constant on
    -- screen), rings sit at 75% of the smallest extent.
    local ex, ey, ez = render.camera_pose()
    local dist = math.sqrt((x - ex) ^ 2 + (y - ey) ^ 2 + (z - ez) ^ 2)
    local marker = 0.6 * dist * 0.0144
    local rx, ry, rz = table.unpack(ref:global_rotation_euler())
    return {
        center = { x, y, z },
        basis = basis_from_euler({ rx, ry, rz }),
        he = { hx, hy, hz },
        -- The rotation ball tracks the selection but never grows past a
        -- fraction of the camera distance (or shrinks below pickability).
        ring = math.clamp(min_he * 0.75, marker * 2.5, dist * 0.14),
        marker = marker,
    }
end

local function corners(geo)
    local out = {}
    for _, sx in { -1, 1 } do
        for _, sy in { -1, 1 } do
            for _, sz in { -1, 1 } do
                local p = local_point(geo, sx * geo.he[1], sy * geo.he[2], sz * geo.he[3])
                p.signs = { sx, sy, sz }
                table.insert(out, p)
            end
        end
    end
    return out
end

local FACE_AXES = { { 2, 3 }, { 1, 3 }, { 1, 2 } }

local function faces(geo)
    local out = {}
    for axis = 1, 3 do
        for _, sign in { -1, 1 } do
            local offset = { 0, 0, 0 }
            offset[axis] = sign * geo.he[axis]
            local u_axis, v_axis = FACE_AXES[axis][1], FACE_AXES[axis][2]
            local quad = {}
            for _, uv in { { -1, -1 }, { 1, -1 }, { 1, 1 }, { -1, 1 } } do
                local o = { offset[1], offset[2], offset[3] }
                o[u_axis] += uv[1] * geo.he[u_axis]
                o[v_axis] += uv[2] * geo.he[v_axis]
                table.insert(quad, local_point(geo, o[1], o[2], o[3]))
            end
            table.insert(out, {
                pos = local_point(geo, offset[1], offset[2], offset[3]),
                axis = axis, sign = sign, quad = quad,
            })
        end
    end
    return out
end

-- Point-in-quad (screen space) via consistent cross-product winding.
local function quad_contains(S, cam, quad, mx, my)
    local pts = {}
    for _, c in quad do
        local sx, sy = project(S, cam, c)
        if sx == nil then
            return false
        end
        table.insert(pts, { sx, sy })
    end
    local sign = nil
    for i = 1, 4 do
        local a, b = pts[i], pts[i % 4 + 1]
        local cross = (b[1] - a[1]) * (my - a[2]) - (b[2] - a[2]) * (mx - a[1])
        if math.abs(cross) > 1e-6 then
            local this = cross > 0
            if sign == nil then
                sign = this
            elseif sign ~= this then
                return false
            end
        end
    end
    return true
end

-- The small floating rectangle at a face's center (scale along the normal).
local function face_rect_points(geo, face)
    local u_axis, v_axis = FACE_AXES[face.axis][1], FACE_AXES[face.axis][2]
    local h = geo.marker * 0.9
    local out = {}
    for _, uv in { { -1, -1 }, { 1, -1 }, { 1, 1 }, { -1, 1 } } do
        local o = { 0, 0, 0 }
        -- Float slightly off the face so it never z-fights the surface.
        o[face.axis] = face.sign * (geo.he[face.axis] + geo.marker * 0.25)
        o[u_axis] += uv[1] * h
        o[v_axis] += uv[2] * h
        table.insert(out, local_point(geo, o[1], o[2], o[3]))
    end
    return out
end

local function ring_point(geo, axis, t)
    local angle = t * math.pi * 2
    local o = { 0, 0, 0 }
    o[axis % 3 + 1] = math.cos(angle) * geo.ring
    o[(axis + 1) % 3 + 1] = math.sin(angle) * geo.ring
    return local_point(geo, o[1], o[2], o[3])
end

-- What the cursor is over. Priority mirrors steadyum: the rotation ball
-- always wins over the face behind it, then the face-center scale rects,
-- then corner brackets, and finally the face surfaces (plane translate).
local function face_normal(geo, face)
    return vscale(geo.basis[face.axis], face.sign)
end

local function face_front_facing(cam, geo, face)
    return vdot(face_normal(geo, face), vsub(cam.eye, face.pos)) > 0
end

local function hit_test(S, cam, geo, mx, my)
    -- The rotation ball wins over everything behind it.
    for axis = 1, 3 do
        for t = 0, 47 do
            local sx, sy = project(S, cam, ring_point(geo, axis, t / 48))
            if sx ~= nil and (sx - mx) ^ 2 + (sy - my) ^ 2 < 12 ^ 2 then
                return { kind = "ring", axis = axis }
            end
        end
    end
    -- Face-center scale rects and corner brackets: only front-facing /
    -- generous radii so the hotspot is at least as big as the visual.
    local all_faces = faces(geo)
    for i, face in all_faces do
        if face_front_facing(cam, geo, face) then
            local sx, sy = project(S, cam, face.pos)
            if quad_contains(S, cam, face_rect_points(geo, face), mx, my)
                or (sx ~= nil and (sx - mx) ^ 2 + (sy - my) ^ 2 < 16 ^ 2)
            then
                return { kind = "facerect", index = i, axis = face.axis, sign = face.sign }
            end
        end
    end
    for i, corner in corners(geo) do
        local sx, sy = project(S, cam, corner)
        if sx ~= nil and (sx - mx) ^ 2 + (sy - my) ^ 2 < 18 ^ 2 then
            return { kind = "handle", index = i }
        end
    end
    -- Face surfaces (plane translate): only the faces looking at the
    -- camera; front faces tile the silhouette, so the hit is unambiguous.
    for i, face in all_faces do
        if face_front_facing(cam, geo, face) and quad_contains(S, cam, face.quad, mx, my) then
            return { kind = "face", index = i, axis = face.axis, sign = face.sign }
        end
    end
    return nil
end

-- steadyum's 6px stroke, 30% thinner per taste.
local W = 4.2

local function line(a, b, color, width, on_top)
    render.draw_line(a[1], a[2], a[3], b[1], b[2], b[3], color[1], color[2], color[3], width, false, on_top == true)
end

-- Corner marker in steadyum's open-bracket style: of the mini-cube's 12
-- edges, draw only those with at least one outward face (faces whose sign
-- matches the handle's corner sign), leaving an open bracket.
local EDGE_AXES = { { 2, 3 }, { 1, 3 }, { 1, 2 } }

local function draw_handle_marker(geo, corner, signs, half, color)
    -- Corner offset in the box's local frame.
    local base = { signs[1] * geo.he[1], signs[2] * geo.he[2], signs[3] * geo.he[3] }
    for axis = 1, 3 do
        local b_axis, c_axis = EDGE_AXES[axis][1], EDGE_AXES[axis][2]
        for _, b_sign in { -1, 1 } do
            for _, c_sign in { -1, 1 } do
                local b_inner = b_sign == -signs[b_axis]
                local c_inner = c_sign == -signs[c_axis]
                if not (b_inner and c_inner) then
                    local a = { base[1], base[2], base[3] }
                    local b = { base[1], base[2], base[3] }
                    a[axis] -= half
                    b[axis] += half
                    a[b_axis] += b_sign * half
                    b[b_axis] += b_sign * half
                    a[c_axis] += c_sign * half
                    b[c_axis] += c_sign * half
                    line(
                        local_point(geo, a[1], a[2], a[3]),
                        local_point(geo, b[1], b[2], b[3]),
                        color,
                        W
                    )
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
    if model.is_2d(S) then
        return gizmo2d.draw(S)
    end
    local geo = geometry(S)
    if geo == nil or S.persona == "script" then
        return
    end
    local hot = hot_part(S)
    local he = geo.he
    local hot_face = if hot ~= nil and hot.kind == "face" then hot else nil
    -- The 12 box edges, in the node's rotated frame. An edge bounds the
    -- hovered face when its fixed position on the face's normal axis
    -- matches the face's sign.
    for axis = 1, 3 do
        local b_axis, c_axis = FACE_AXES[axis][1], FACE_AXES[axis][2]
        for _, sb in { -1, 1 } do
            for _, sc2 in { -1, 1 } do
                local color = ORANGE
                if hot_face ~= nil
                    and ((b_axis == hot_face.axis and sb == hot_face.sign)
                        or (c_axis == hot_face.axis and sc2 == hot_face.sign))
                then
                    color = HOVER
                end
                local a_off, b_off = { 0, 0, 0 }, { 0, 0, 0 }
                a_off[axis], b_off[axis] = -he[axis], he[axis]
                for _, o in { a_off, b_off } do
                    o[b_axis] = sb * he[b_axis]
                    o[c_axis] = sc2 * he[c_axis]
                end
                line(
                    local_point(geo, a_off[1], a_off[2], a_off[3]),
                    local_point(geo, b_off[1], b_off[2], b_off[3]),
                    color,
                    W
                )
            end
        end
    end
    for i, corner in corners(geo) do
        local hot_handle = hot ~= nil and hot.kind == "handle" and hot.index == i
        draw_handle_marker(geo, corner, corner.signs, geo.marker, if hot_handle then HOVER else ORANGE)
    end
    -- Face-center scale rectangles (drag = scale along the face normal).
    for i, face in faces(geo) do
        local hot_rect = hot ~= nil and hot.kind == "facerect" and hot.index == i
        local color = if hot_rect then HOVER else ORANGE
        local pts = face_rect_points(geo, face)
        for j = 1, 4 do
            line(pts[j], pts[j % 4 + 1], color, W)
        end
    end
    -- The rotation ball renders on top of everything.
    for axis = 1, 3 do
        local hot_ring = hot ~= nil and hot.kind == "ring" and hot.axis == axis
        local color = if hot_ring then HOVER else ORANGE
        local prev = ring_point(geo, axis, 0)
        for t = 1, 48 do
            local next = ring_point(geo, axis, t / 48)
            line(prev, next, color, W, true)
            prev = next
        end
    end
end

-- Screen direction (design px per world unit) of a world direction at a point.
local function screen_dir(S, cam, center, dir)
    local ax, ay = project(S, cam, center)
    local bx, by = project(S, cam, vadd(center, dir))
    if ax == nil or bx == nil then
        return nil
    end
    return { bx - ax, by - ay }
end

local function start_gizmo_drag(S, cam, geo, part, mx, my)
    local node, ref = model.selected(S)
    local px, py, pz = table.unpack(ref:position())
    local sx, sy, sz = table.unpack(ref:scale())
    local rot = node.rotation_euler or { 0, 0, 0 }
    local cx, cy = project(S, cam, geo.center)
    local drag = {
        part = part,
        pos = { px, py, pz },
        scale = { sx, sy, sz },
        rot = { rot[1], rot[2], rot[3] },
        basis = geo.basis,
        mouse = { mx, my },
        center = { cx, cy },
        extent = geo.he[part.axis or 1],
        world_center = { geo.center[1], geo.center[2], geo.center[3] },
    }
    if part.kind == "face" then
        -- Anchor: where the mouse ray first hits the face's plane.
        local n = vscale(geo.basis[part.axis], part.sign or 1)
        drag.plane_n = n
        drag.plane_p = { geo.center[1], geo.center[2], geo.center[3] }
        drag.anchor = ray_plane(cam, drag.plane_p, n)
    end
    S.gizmo_drag = drag
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
        -- Screen-angle delta around the projected center, applied about the
        -- node's LOCAL axis (the rings rotate with the object), signed by
        -- whether that axis points toward the camera.
        local a0 = math.atan(d.mouse[2] - d.center[2], d.mouse[1] - d.center[1])
        local a1 = math.atan(my - d.center[2], mx - d.center[1])
        local axis_dir = d.basis[d.part.axis]
        local toward = vdot(axis_dir, vsub(cam.eye, d.world_center))
        local delta = (a1 - a0) * (if toward >= 0 then -1 else 1)
        local rot = euler_from_basis(rotate_basis_local(basis_from_euler(d.rot), d.part.axis, delta))
        node.rotation_euler = rot
        ref:set_rotation_euler(rot[1], rot[2], rot[3])
        S.dirty = true
    elseif d.part.kind == "facerect" then
        -- Scale along the face's (rotated) normal: mouse movement projected
        -- onto its screen direction, converted to world units.
        local axis = d.part.axis
        local dir = screen_dir(S, cam, d.world_center, d.basis[axis])
        if dir ~= nil then
            local len2 = dir[1] ^ 2 + dir[2] ^ 2
            if len2 > 1e-6 then
                local moved = ((mx - d.mouse[1]) * dir[1] + (my - d.mouse[2]) * dir[2]) / len2
                local ext0 = math.max(d.extent, 1e-3)
                local f = math.clamp((ext0 + moved * d.part.sign) / ext0, 0.05, 20)
                local sc = { d.scale[1], d.scale[2], d.scale[3] }
                sc[axis] *= f
                ref:set_scale(sc[1], sc[2], sc[3])
                node.scale = { sc[1], sc[2], sc[3] }
                S.dirty = true
            end
        end
    elseif d.part.kind == "face" then
        -- Slide the object in the face's plane: the world-space delta
        -- between where the mouse ray hits that plane now vs at drag start.
        if d.anchor ~= nil then
            local hit = ray_plane(cam, d.plane_p, d.plane_n)
            if hit ~= nil then
                local delta = vsub(hit, d.anchor)
                local pos = vadd({ d.pos[1], d.pos[2], d.pos[3] }, delta)
                if (S.chip_state.scene or {}).snap then
                    for axis = 1, 3 do
                        pos[axis] = math.round(pos[axis] * 2) / 2
                    end
                end
                ref:set_position(pos[1], pos[2], pos[3])
                node.position = { pos[1], pos[2], pos[3] }
                S.dirty = true
            end
        end
    end
end

local MANIP = { move = true, rotate = true, scale = true, zoom = true }

-- Runs every frame from Editor:update.
function M.update(S)
    if model.is_2d(S) then
        return gizmo2d.update(S, M.viewport_rect(S))
    end
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
                        local x, y, z = table.unpack(ref:global_position())
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
        local x, y, z = table.unpack(ref:position())
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
        local sx, sy, sz = table.unpack(ref:scale())
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
