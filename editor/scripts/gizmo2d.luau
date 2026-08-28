-- 2D selection gizmo, steadyum-flavored like the 3D one: an orange padded
-- box whose interior translates, corner brackets scale uniformly,
-- edge-center rectangles scale along one axis, and a rotation ring rotates
-- about z. All math is in 2D world space over the camera state the backend
-- publishes (`render.camera_2d`, `render.mouse_world_2d`).
local model = require("scripts/model")

local M = {}

local ORANGE = { 1.0, 0.647, 0.0 }
local HOVER = { 0.118, 0.314, 0.784 }
local W = 4.2

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

-- Selection geometry: center, angle, padded half extents, marker sizes.
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
    hx, hy = hx * sx + 0.06, hy * sy + 0.06
    local _, _, angle = ref:global_rotation_euler()
    local wpp = world_per_px(S)
    local marker = 9 * wpp
    return {
        center = { x, y },
        angle = angle,
        he = { hx, hy },
        marker = marker,
        ring = math.clamp(math.min(hx, hy) * 0.75, marker * 2.5, 110 * wpp),
        wpp = wpp,
    }
end

local EDGE_RECTS = {
    { axis = 1, sign = 1 },
    { axis = 1, sign = -1 },
    { axis = 2, sign = 1 },
    { axis = 2, sign = -1 },
}

local function edge_rect_center(geo, part)
    local o = { 0, 0 }
    o[part.axis] = part.sign * (geo.he[part.axis] + geo.marker * 1.2)
    return o[1], o[2]
end

local function hit_test(S, geo, mx, my)
    local lx, ly = to_local(geo, mx, my)
    local wpp = geo.wpp
    -- The rotation ring wins over everything it overlaps.
    local dist = math.sqrt(lx ^ 2 + ly ^ 2)
    if math.abs(dist - geo.ring) < 8 * wpp then
        return { kind = "ring" }
    end
    -- Edge-center scale rectangles (generous hotspot).
    for _, part in EDGE_RECTS do
        local cx, cy = edge_rect_center(geo, part)
        local h = geo.marker * 1.4
        if math.abs(lx - cx) < h and math.abs(ly - cy) < h then
            return { kind = "facerect", axis = part.axis, sign = part.sign }
        end
    end
    -- Corner brackets: uniform scale.
    for _, sx in { -1, 1 } do
        for _, sy in { -1, 1 } do
            local cx, cy = sx * geo.he[1], sy * geo.he[2]
            if math.abs(lx - cx) < geo.marker * 1.8 and math.abs(ly - cy) < geo.marker * 1.8 then
                return { kind = "handle", signs = { sx, sy } }
            end
        end
    end
    -- Box interior: translate.
    if math.abs(lx) < geo.he[1] and math.abs(ly) < geo.he[2] then
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

function M.draw(S)
    local geo = geometry(S)
    if geo == nil or S.persona == "script" then
        return
    end
    local hot = hot_part(S)
    local hx, hy = geo.he[1], geo.he[2]
    -- Box edges (all four highlight during a translate hover/drag).
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
    -- Corner brackets: two short outward segments per corner.
    for _, sx in { -1, 1 } do
        for _, sy in { -1, 1 } do
            local hot_handle = hot ~= nil
                and hot.kind == "handle"
                and hot.signs[1] == sx
                and hot.signs[2] == sy
            local color = if hot_handle then HOVER else ORANGE
            local m = geo.marker
            local cx, cy = sx * hx, sy * hy
            line(
                local_point(geo, cx - sx * m, cy),
                local_point(geo, cx + sx * m * 0.4, cy),
                color, W
            )
            line(
                local_point(geo, cx, cy - sy * m),
                local_point(geo, cx, cy + sy * m * 0.4),
                color, W
            )
        end
    end
    -- Edge-center rectangles (scale along one local axis).
    for _, part in EDGE_RECTS do
        local hot_rect = hot ~= nil
            and hot.kind == "facerect"
            and hot.axis == part.axis
            and hot.sign == part.sign
        local color = if hot_rect then HOVER else ORANGE
        local cx, cy = edge_rect_center(geo, part)
        local h = geo.marker * 0.9
        local pts = {
            local_point(geo, cx - h, cy - h),
            local_point(geo, cx + h, cy - h),
            local_point(geo, cx + h, cy + h),
            local_point(geo, cx - h, cy + h),
        }
        for i = 1, 4 do
            line(pts[i], pts[i % 4 + 1], color, W)
        end
    end
    -- Rotation ring.
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
    S.gizmo_drag = {
        part = part,
        pos = { px, py, pz },
        scale = { sx, sy, sz },
        rot = { rot[1], rot[2], rot[3] },
        center = { geo.center[1], geo.center[2] },
        angle = geo.angle,
        he = { geo.he[1], geo.he[2] },
        anchor = { mx, my },
        angle0 = math.atan(my - geo.center[2], mx - geo.center[1]),
    }
end

local function apply_drag(S, mx, my)
    local d = S.gizmo_drag
    local node, ref = model.selected(S)
    if ref == nil then
        return
    end
    if d.part.kind == "face" then
        local x = d.pos[1] + (mx - d.anchor[1])
        local y = d.pos[2] + (my - d.anchor[2])
        if (S.chip_state.scene or {}).snap then
            x = math.round(x * 2) / 2
            y = math.round(y * 2) / 2
        end
        ref:set_position(x, y, d.pos[3])
        node.position = { x, y, d.pos[3] }
        S.dirty = true
    elseif d.part.kind == "handle" then
        -- Uniform scale by radial world distance from the center.
        local d0 = math.max(1e-3, math.sqrt((d.anchor[1] - d.center[1]) ^ 2 + (d.anchor[2] - d.center[2]) ^ 2))
        local d1 = math.sqrt((mx - d.center[1]) ^ 2 + (my - d.center[2]) ^ 2)
        local f = math.clamp(d1 / d0, 0.05, 20)
        local sx, sy = d.scale[1] * f, d.scale[2] * f
        ref:set_scale(sx, sy, d.scale[3])
        node.scale = { sx, sy, d.scale[3] }
        S.dirty = true
    elseif d.part.kind == "facerect" then
        -- Scale along one local axis by the mouse's local travel.
        local axis = d.part.axis
        local c, s = math.cos(d.angle), math.sin(d.angle)
        local dx, dy = mx - d.center[1], my - d.center[2]
        local l = if axis == 1 then c * dx + s * dy else -s * dx + c * dy
        local ext0 = math.max(d.he[axis], 1e-3)
        local f = math.clamp(math.abs(l) / ext0, 0.05, 20)
        local sc = { d.scale[1], d.scale[2] }
        sc[axis] *= f
        ref:set_scale(sc[1], sc[2], d.scale[3])
        node.scale = { sc[1], sc[2], d.scale[3] }
        S.dirty = true
    elseif d.part.kind == "ring" then
        local a1 = math.atan(my - d.center[2], mx - d.center[1])
        local rot = { d.rot[1], d.rot[2], d.rot[3] + (a1 - d.angle0) }
        node.rotation_euler = rot
        ref:set_rotation_euler(rot[1], rot[2], rot[3])
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
