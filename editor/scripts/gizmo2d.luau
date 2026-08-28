-- 2D selection gizmo replicating steadyum's dim2 bbox/transform gizmo:
-- the object's local AABB frame with eight scale handles floating just
-- outside it (4 corner brackets scale per-axis, 4 edge midpoints scale one
-- axis, Shift makes either uniform; the opposite corner/edge stays
-- anchored), the whole interior translates, and a center rotation disc
-- (drawn as a circle, picked as the full disc, priority over everything)
-- rotates about z. All math is 2D world space over the camera state the
-- backend publishes.
local model = require("scripts/model")

local ORANGE = { 1.0, 0.647, 0.0 }
local HOVER = { 0.118, 0.314, 0.784 }
-- steadyum's 6px bbox stroke, 30% thinner per taste (matches the 3D gizmo).
local W = 4.2
-- steadyum HANDLE_SCALE_2D: handle pick radius is ~12 px on screen.
local HANDLE_PX = 12
-- steadyum HANDLE_DRAW_HALF: drawn marker half-size vs the pick radius.
local DRAW_HALF = 0.6
-- steadyum handle_outward_offset: corners sit 1 pick-radius outside the
-- frame, edge midpoints 1.25 (compensates the corners' diagonal reach).
local OUT_CORNER = 1.0
local OUT_EDGE = 1.25

-- steadyum HANDLE_SIGNS (dim2): 4 corners, then 4 edge midpoints.
local HANDLE_SIGNS = {
    { -1, -1 },
    { 1, -1 },
    { 1, 1 },
    { -1, 1 },
    { -1, 0 },
    { 1, 0 },
    { 0, -1 },
    { 0, 1 },
}

local M = {}

-- World units per design pixel (design px = logical px / ui scale).
local function world_per_px(S)
    local _, _, zoom = render.camera_2d()
    return S.ui_scale / math.max(zoom, 0.01)
end

local function line(a, b, color, width)
    render.draw_line_2d(a[1], a[2], b[1], b[2], color[1], color[2], color[3], width or W)
end

-- Rotated local offset -> world point.
local function local_point(geo, ox, oy)
    local c, s = math.cos(geo.angle), math.sin(geo.angle)
    return { geo.center[1] + c * ox - s * oy, geo.center[2] + s * ox + c * oy }
end

-- World point -> box-local coordinates.
local function to_local(geo, x, y)
    local dx, dy = x - geo.center[1], y - geo.center[2]
    local c, s = math.cos(geo.angle), math.sin(geo.angle)
    return c * dx + s * dy, -s * dx + c * dy
end

-- The 2D half extents a node renders/collides with (nil when it has none).
function M.node_half_extents(node)
    local shape = node.shape2d or node.collider2d
    if shape == nil then
        return nil
    end
    if shape.kind == "circle" or shape.shape == "circle" then
        local r = shape.radius or 0.5
        return r, r
    end
    local he = shape.half_extents or { 0.5, 0.5 }
    return he[1], he[2]
end

-- Handle center in box-local coordinates (steadyum: signs * half_extents
-- plus the normalized outward offset).
local function handle_local(geo, signs)
    local nx, ny = signs[1], signs[2]
    local len = math.sqrt(nx * nx + ny * ny)
    local corner = nx ~= 0 and ny ~= 0
    local out = geo.rs * (if corner then OUT_CORNER else OUT_EDGE) / len
    return signs[1] * geo.he[1] + nx * out, signs[2] * geo.he[2] + ny * out
end

-- Selection geometry: center, angle, half extents, camera-adaptive sizes.
local function geometry(S)
    local node, ref = model.selected(S)
    if ref == nil then
        return nil
    end
    local x, y = ref:global_position()
    local hx, hy = M.node_half_extents(node)
    if hx == nil then
        hx, hy = 0.6, 0.6
    end
    local sx, sy = ref:global_scale()
    hx, hy = hx * sx, hy * sy
    local _, _, angle = ref:global_rotation_euler()
    local rs = HANDLE_PX * world_per_px(S)
    return {
        center = { x, y },
        angle = angle,
        he = { hx, hy },
        -- steadyum: the pick radius doubles as the camera-adaptive scale.
        rs = rs,
        marker = DRAW_HALF * rs,
        -- steadyum ring: diameter 75% of the smallest bbox side, capped at
        -- 4x the handle's full drawn size.
        ring = math.min(0.75 * math.min(hx, hy), 2.4 * rs),
    }
end

-- What the cursor is over. steadyum priority: the rotation disc (the full
-- disc, not just the circle line) wins over every bbox element, then the
-- scale handles, then the interior translate picker.
local function hit_test(S, geo, mx, my)
    local lx, ly = to_local(geo, mx, my)
    if math.sqrt(lx * lx + ly * ly) <= geo.ring then
        return { kind = "ring" }
    end
    for _, signs in HANDLE_SIGNS do
        local cx, cy = handle_local(geo, signs)
        if (lx - cx) ^ 2 + (ly - cy) ^ 2 <= geo.rs ^ 2 then
            return { kind = "handle", signs = { signs[1], signs[2] } }
        end
    end
    if math.abs(lx) <= geo.he[1] and math.abs(ly) <= geo.he[2] then
        return { kind = "face" }
    end
    return nil
end

local function hot_part(S)
    if S.gizmo_drag ~= nil then
        return S.gizmo_drag.part
    end
    return S.gizmo_hover
end

-- Handle marker, steadyum style: an open square of half-size
-- `geo.marker` at the handle position. Corners keep their two outer
-- edges (an L bracket opening toward the box); edge midpoints keep the
-- single outer edge parallel to the frame.
local MARKER_EDGES = {
    -- face_axis, face_sign, ax, ay, bx, by (unit square edge endpoints)
    { 2, -1, -1, -1, 1, -1 }, -- bottom
    { 2, 1, -1, 1, 1, 1 },    -- top
    { 1, -1, -1, -1, -1, 1 }, -- left
    { 1, 1, 1, -1, 1, 1 },    -- right
}

local function draw_handle_marker(geo, signs, color)
    local hx, hy = handle_local(geo, signs)
    local h = geo.marker
    local is_edge_handle = (if signs[1] ~= 0 then 1 else 0) + (if signs[2] ~= 0 then 1 else 0) == 1
    for _, e in MARKER_EDGES do
        local face_axis, face_sign = e[1], e[2]
        -- Edge-midpoint handles: only the edge on their face's axis.
        if is_edge_handle and signs[face_axis] == 0 then
            continue
        end
        -- Skip inner edges (the ones facing into the box).
        if signs[face_axis] ~= 0 and face_sign == -signs[face_axis] then
            continue
        end
        line(
            local_point(geo, hx + e[3] * h, hy + e[4] * h),
            local_point(geo, hx + e[5] * h, hy + e[6] * h),
            color,
            W
        )
    end
end

function M.draw(S)
    local geo = geometry(S)
    if geo == nil or S.persona == "script" then
        return
    end
    local hot = hot_part(S)
    local hx, hy = geo.he[1], geo.he[2]
    -- Frame: in steadyum the 2D translate picker covers the whole interior,
    -- so a translate hover/drag highlights all four edges.
    local box_color = if hot ~= nil and hot.kind == "face" then HOVER else ORANGE
    local corners = {
        local_point(geo, -hx, -hy),
        local_point(geo, hx, -hy),
        local_point(geo, hx, hy),
        local_point(geo, -hx, hy),
    }
    for i = 1, 4 do
        line(corners[i], corners[i % 4 + 1], box_color, W)
    end
    for _, signs in HANDLE_SIGNS do
        local hot_handle = hot ~= nil
            and hot.kind == "handle"
            and hot.signs[1] == signs[1]
            and hot.signs[2] == signs[2]
        draw_handle_marker(geo, signs, if hot_handle then HOVER else ORANGE)
    end
    -- Rotation circle, same stroke and colors as the frame.
    local ring_color = if hot ~= nil and hot.kind == "ring" then HOVER else ORANGE
    local prev = local_point(geo, geo.ring, 0)
    for t = 1, 48 do
        local a = t / 48 * math.pi * 2
        local next = local_point(geo, math.cos(a) * geo.ring, math.sin(a) * geo.ring)
        line(prev, next, ring_color, W)
        prev = next
    end
end

local function start_drag(S, geo, part, mx, my)
    local node, ref = model.selected(S)
    local px, py, pz = ref:position()
    local sx, sy, sz = ref:scale()
    local rot = node.rotation_euler or { 0, 0, 0 }
    local drag = {
        part = part,
        pos = { px, py, pz },
        scale = { sx, sy, sz },
        rot = { rot[1], rot[2], rot[3] },
        center = { geo.center[1], geo.center[2] },
        angle = geo.angle,
        he = { geo.he[1], geo.he[2] },
        mouse = { mx, my },
        angle0 = math.atan(my - geo.center[2], mx - geo.center[1]),
    }
    if part.kind == "handle" then
        -- The opposite corner/edge stays fixed in world space.
        drag.anchor = local_point(geo, -part.signs[1] * geo.he[1], -part.signs[2] * geo.he[2])
    end
    S.gizmo_drag = drag
end

local function apply_drag(S, mx, my)
    local d = S.gizmo_drag
    local node, ref = model.selected(S)
    if ref == nil then
        return
    end
    if d.part.kind == "face" then
        local x = d.pos[1] + (mx - d.mouse[1])
        local y = d.pos[2] + (my - d.mouse[2])
        if (S.chip_state.scene or {}).snap then
            x = math.round(x * 2) / 2
            y = math.round(y * 2) / 2
        end
        ref:set_position(x, y, d.pos[3])
        node.position = { x, y, d.pos[3] }
        S.dirty = true
    elseif d.part.kind == "ring" then
        local a1 = math.atan(my - d.center[2], mx - d.center[1])
        local rot = { d.rot[1], d.rot[2], d.rot[3] + (a1 - d.angle0) }
        node.rotation_euler = rot
        ref:set_rotation_euler(rot[1], rot[2], rot[3])
        S.dirty = true
    elseif d.part.kind == "handle" then
        -- steadyum drive_single_drag: per-axis factor from the cursor's
        -- world travel projected on the box's (rotated) axes; Shift applies
        -- the dominant factor to every active axis.
        local dx, dy = mx - d.mouse[1], my - d.mouse[2]
        local c, s = math.cos(d.angle), math.sin(d.angle)
        local axes = { { c, s }, { -s, c } }
        local factors = { 1, 1 }
        for i = 1, 2 do
            local sign = d.part.signs[i]
            if sign ~= 0 then
                local denom = 2 * sign * d.he[i]
                if math.abs(denom) > 1e-6 then
                    local travel = dx * axes[i][1] + dy * axes[i][2]
                    factors[i] = math.max(0.001, 1 + travel / denom)
                end
            end
        end
        if input.is_down("LShift") or input.is_down("RShift") then
            local dominant = 1
            for i = 1, 2 do
                if d.part.signs[i] ~= 0 and math.abs(factors[i] - 1) > math.abs(dominant - 1) then
                    dominant = factors[i]
                end
            end
            for i = 1, 2 do
                if d.part.signs[i] ~= 0 then
                    factors[i] = math.max(0.001, dominant)
                end
            end
        end
        local sx, sy = d.scale[1] * factors[1], d.scale[2] * factors[2]
        ref:set_scale(sx, sy, d.scale[3])
        node.scale = { sx, sy, d.scale[3] }
        -- Keep the anchor (opposite corner/edge) fixed: recompute the box
        -- center from the anchor and the scaled half extents.
        local off_x = d.part.signs[1] * d.he[1] * factors[1]
        local off_y = d.part.signs[2] * d.he[2] * factors[2]
        local ncx = d.anchor[1] + c * off_x - s * off_y
        local ncy = d.anchor[2] + s * off_x + c * off_y
        local x = d.pos[1] + (ncx - d.center[1])
        local y = d.pos[2] + (ncy - d.center[2])
        ref:set_position(x, y, d.pos[3])
        node.position = { x, y, d.pos[3] }
        S.dirty = true
    end
end

-- Click-select: the smallest 2D shape containing the point, with a
-- proximity fallback for shapeless nodes.
local function pick(S, mx, my)
    local best, best_area = nil, math.huge
    for i, node in S.doc do
        local ref = model.ref(S, i)
        if ref ~= nil then
            local hx, hy = M.node_half_extents(node)
            if hx ~= nil then
                local sx, sy = ref:global_scale()
                hx, hy = hx * sx, hy * sy
                local x, y = ref:global_position()
                local _, _, angle = ref:global_rotation_euler()
                local lx, ly = to_local({ center = { x, y }, angle = angle }, mx, my)
                if math.abs(lx) <= hx and math.abs(ly) <= hy then
                    local area = hx * hy
                    if area < best_area then
                        best, best_area = i, area
                    end
                end
            end
        end
    end
    if best ~= nil then
        return best
    end
    -- Fallback: nearest node origin within 40 design px.
    local wpp = world_per_px(S)
    local best_d = 40 * wpp
    for i in S.doc do
        local ref = model.ref(S, i)
        if ref ~= nil then
            local x, y = ref:global_position()
            local dist = math.sqrt((x - mx) ^ 2 + (y - my) ^ 2)
            if dist < best_d then
                best, best_d = i, dist
            end
        end
    end
    return best
end

local MANIP = { move = true, rotate = true, scale = true, zoom = true }

-- Runs every frame from Editor:update (via gizmo.update's 2D dispatch).
-- `vp` is the viewport rect in design px, from gizmo.viewport_rect.
function M.update(S, vp)
    local _, _, _, _, _, _, _, dpi = render.camera_pose()
    local mx, my = render.mouse_world_2d()
    local geo = geometry(S)
    local pmx, pmy = input.mouse_position()
    pmx /= dpi * S.ui_scale
    pmy /= dpi * S.ui_scale
    local over_viewport = pmx > vp.x and pmx < vp.x + vp.w
        and pmy > vp.y + 44 and pmy < vp.y + vp.h - 40

    if S.gizmo_drag == nil then
        S.gizmo_hover = if geo ~= nil and over_viewport and S.persona ~= "script"
            then hit_test(S, geo, mx, my)
            else nil
    end

    render.set_camera_input(not MANIP[S.tool] and S.gizmo_hover == nil and S.gizmo_drag == nil)

    if input.mouse_just_pressed(1) and over_viewport then
        if S.gizmo_hover ~= nil and geo ~= nil then
            start_drag(S, geo, S.gizmo_hover, mx, my)
        else
            S.drag_in_viewport = true
            if S.tool == "select" then
                local best = pick(S, mx, my)
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
        apply_drag(S, mx, my)
        return
    end
    if not S.drag_in_viewport then
        return
    end

    -- Rail-tool fallbacks: drag anywhere in the viewport. Deltas in design
    -- px; translation converts to world units through wpp.
    local dx, dy = input.mouse_delta()
    dx /= dpi * S.ui_scale
    dy /= dpi * S.ui_scale
    if dx == 0 and dy == 0 then
        return
    end
    local wpp = world_per_px(S)
    local node, ref = model.selected(S)
    if S.tool == "move" and ref ~= nil then
        local x, y, z = ref:position()
        x += dx * wpp
        y -= dy * wpp
        if (S.chip_state.scene or {}).snap then
            x = math.round(x * 2) / 2
            y = math.round(y * 2) / 2
        end
        ref:set_position(x, y, z)
        node.position = { x, y, z }
        S.dirty = true
    elseif S.tool == "rotate" and ref ~= nil then
        local r = node.rotation_euler or { 0, 0, 0 }
        r[3] -= dx * 0.01
        node.rotation_euler = r
        ref:set_rotation_euler(r[1], r[2], r[3])
        S.dirty = true
    elseif S.tool == "scale" and ref ~= nil then
        local sx, sy, sz = ref:scale()
        local f = math.clamp(1 - dy * 0.01, 0.2, 5)
        sx, sy = sx * f, sy * f
        ref:set_scale(sx, sy, sz)
        node.scale = { sx, sy, sz }
        S.dirty = true
    elseif S.tool == "zoom" then
        S.set_zoom(S.zoom - dy * 0.5)
    end
end

return M
