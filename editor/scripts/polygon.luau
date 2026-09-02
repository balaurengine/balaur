-- The Polygon tool: trace a polygon's outline in the viewport, draw its
-- polygons over interior points, place its UVs over the texture, and paint
-- bone weights. Every edit lands in the node's inline `mesh` table.
local model = require("scripts/model")
local history = require("scripts/history")
local theme = require("scripts/theme")

local M = {}

M.MODES = { "points", "polygons", "uv", "weights" }
M.mode_names = { points = "Points", polygons = "Polygons", uv = "UV", weights = "Weights" }

local HOVER = { 0.118, 0.314, 0.784 }
local W = 2.0
local PICK_PX = 8
local MARK_PX = 4
local LEFT, RIGHT = input.MOUSE_LEFT, input.MOUSE_RIGHT

local function world_per_px(S)
    local _, _, zoom = render.camera_2d()
    return S.ui_scale / math.max(zoom, 0.01)
end

function M.component(node)
    if node == nil or type(node.polygon) ~= "table" then
        return nil
    end
    return node.polygon
end

-- The inline mesh table under edit, or nil: a mesh that is a file reference
-- is edited by making it inline first (the inspector's asset row).
function M.mesh(S)
    local comp = M.component(S.doc[S.sel])
    if comp == nil or type(comp.mesh) ~= "table" then
        return nil
    end
    return comp.mesh
end

function M.editable(S)
    local comp = M.component(S.doc[S.sel])
    return comp ~= nil and (type(comp.mesh) == "table" or comp.mesh == nil or comp.mesh == "")
end

-- The polygon's world frame: how a local vertex lands on screen.
local function frame(S)
    local _, ref = model.selected(S)
    if ref == nil then
        return nil
    end
    local x, y = table.unpack(ref:global_position())
    local _, _, angle = table.unpack(ref:global_rotation_euler())
    local sx, sy = table.unpack(ref:global_scale())
    return { x = x, y = y, c = math.cos(angle), s = math.sin(angle), sx = sx, sy = sy }
end

local function to_world(f, lx, ly)
    local px, py = lx * f.sx, ly * f.sy
    return f.x + f.c * px - f.s * py, f.y + f.s * px + f.c * py
end

local function to_local(f, wx, wy)
    local dx, dy = wx - f.x, wy - f.y
    local px, py = f.c * dx + f.s * dy, -f.s * dx + f.c * dy
    return px / (if math.abs(f.sx) > 1e-6 then f.sx else 1), py / (if math.abs(f.sy) > 1e-6 then f.sy else 1)
end

local function internal_count(mesh)
    return math.max(0, math.floor(mesh.internal or 0))
end

local function outline_count(mesh)
    return #mesh.positions - internal_count(mesh)
end

-- Texture pixel size for the selection, cached per path.
local function texture_size(S)
    local comp = M.component(S.doc[S.sel])
    if comp == nil or comp.texture == nil or comp.texture == "" then
        return nil
    end
    local cache = S.polygon_texture
    if cache ~= nil and cache.path == comp.texture then
        return cache.w, cache.h
    end
    local ok, w, h = pcall(render.texture_size, S.game_root .. "/" .. comp.texture)
    if not ok then
        log.warn(`texture {comp.texture}: {tostring(w)}`)
        w, h = nil, nil
    end
    S.polygon_texture = { path = comp.texture, w = w, h = h }
    return w, h
end

-- The engine's default UV of a local point: the texture centred on the
-- origin at `pixels_per_unit`, v downward.
local function default_uv(S, lx, ly)
    local comp = M.component(S.doc[S.sel])
    local w, h = texture_size(S)
    if w == nil then
        return { 0.5, 0.5 }
    end
    local ppu = comp.pixels_per_unit or 100
    return { 0.5 + lx * ppu / w, 0.5 - ly * ppu / h }
end

-- A UV's place in the polygon's local frame, over the texture backdrop.
local function uv_to_local(S, u, v)
    local comp = M.component(S.doc[S.sel])
    local w, h = texture_size(S)
    if w == nil then
        return (u - 0.5), (0.5 - v)
    end
    local ppu = comp.pixels_per_unit or 100
    return (u - 0.5) * w / ppu, (0.5 - v) * h / ppu
end

local function local_to_uv(S, lx, ly)
    return default_uv(S, lx, ly)
end

-- Every edit records history *before* touching the mesh, then pushes the
-- document's polygon onto the live node. A `key` coalesces a drag.
local function begin(S, label, key)
    history.record(S, label, key)
end

local function apply(S)
    model.apply_component(S, "polygon")
end

-- The mesh, created empty when the polygon has none yet. The caller has
-- already recorded history.
local function ensure_mesh(S)
    local mesh = M.mesh(S)
    if mesh ~= nil then
        return mesh
    end
    if not M.editable(S) then
        log.warn("this polygon's mesh is a file; make it inline to edit it here")
        return nil
    end
    local comp = M.component(S.doc[S.sel])
    comp.mesh = { positions = {} }
    return comp.mesh
end

-- Every per-vertex list a mesh carries, so edits keep them square.
local function per_vertex_lists(mesh)
    local lists = {}
    if type(mesh.uvs) == "table" then
        table.insert(lists, mesh.uvs)
    end
    if type(mesh.skin) == "table" and type(mesh.skin.bones) == "table" then
        for _, bone in mesh.skin.bones do
            if type(bone.weights) == "table" then
                table.insert(lists, bone.weights)
            end
        end
    end
    return lists
end

-- Insert a vertex at 1-based `at`, shifting polygon indices past it.
local function insert_vertex(S, mesh, at, lx, ly)
    table.insert(mesh.positions, at, { lx, ly })
    for _, list in per_vertex_lists(mesh) do
        local filler = if list == mesh.uvs then default_uv(S, lx, ly) else 0
        table.insert(list, at, filler)
    end
    if type(mesh.polygons) == "table" then
        for _, ring in mesh.polygons do
            for j, index in ring do
                if index >= at - 1 then
                    ring[j] = index + 1
                end
            end
        end
    end
end

local function remove_vertex(mesh, at)
    table.remove(mesh.positions, at)
    for _, list in per_vertex_lists(mesh) do
        table.remove(list, at)
    end
    if at > outline_count(mesh) + 1 then
        mesh.internal = internal_count(mesh) - 1
    end
    if type(mesh.polygons) == "table" then
        local kept = {}
        for _, ring in mesh.polygons do
            local uses = false
            for j, index in ring do
                if index == at - 1 then
                    uses = true
                elseif index > at - 1 then
                    ring[j] = index - 1
                end
            end
            if not uses then
                table.insert(kept, ring)
            end
        end
        mesh.polygons = if #kept > 0 then kept else nil
    end
end

-- Add an outline vertex (after the last outline one) or an internal one, at
-- a world point. Returns its 1-based index.
function M.add_vertex(S, wx, wy, internal)
    local f = frame(S)
    if f == nil or not M.editable(S) then
        return nil
    end
    begin(S, if internal then "add internal vertex" else "add vertex")
    local mesh = ensure_mesh(S)
    if mesh == nil then
        return nil
    end
    local lx, ly = to_local(f, wx, wy)
    local at
    if internal then
        at = #mesh.positions + 1
        insert_vertex(S, mesh, at, lx, ly)
        mesh.internal = internal_count(mesh) + 1
    else
        at = outline_count(mesh) + 1
        insert_vertex(S, mesh, at, lx, ly)
    end
    apply(S)
    return at
end

function M.remove_vertex(S, at)
    local mesh = M.mesh(S)
    if mesh == nil or mesh.positions[at] == nil then
        return
    end
    begin(S, "remove vertex")
    remove_vertex(M.mesh(S), at)
    apply(S)
end

function M.move_vertex(S, at, wx, wy)
    local mesh = M.mesh(S)
    local f = frame(S)
    if mesh == nil or f == nil or mesh.positions[at] == nil then
        return
    end
    begin(S, "move vertex", `poly:move:{S.sel}:{at}`)
    local lx, ly = to_local(f, wx, wy)
    M.mesh(S).positions[at] = { lx, ly }
    apply(S)
end

-- Close the loop being drawn into one of the mesh's polygons.
function M.close_loop(S)
    local mesh = M.mesh(S)
    local ring = S.polygon_loop
    if mesh == nil or ring == nil or #ring < 3 then
        S.polygon_loop = nil
        return
    end
    begin(S, "draw polygon")
    mesh = M.mesh(S)
    local zero_based = {}
    for _, index in ring do
        table.insert(zero_based, index - 1)
    end
    mesh.polygons = mesh.polygons or {}
    table.insert(mesh.polygons, zero_based)
    S.polygon_loop = nil
    apply(S)
end

-- Back to automatic triangulation of the outline.
function M.clear_polygons(S)
    local mesh = M.mesh(S)
    if mesh == nil or mesh.polygons == nil then
        return
    end
    begin(S, "clear polygons")
    M.mesh(S).polygons = nil
    apply(S)
end

-- UVs materialised from the defaults, so one can be moved on its own.
local function ensure_uvs(S, mesh)
    if type(mesh.uvs) == "table" and #mesh.uvs == #mesh.positions then
        return mesh.uvs
    end
    local uvs = {}
    for i, p in mesh.positions do
        uvs[i] = default_uv(S, p[1], p[2])
    end
    mesh.uvs = uvs
    return uvs
end

function M.move_uv(S, at, wx, wy)
    local mesh = M.mesh(S)
    local f = frame(S)
    if mesh == nil or f == nil or mesh.positions[at] == nil then
        return
    end
    begin(S, "move uv", `poly:uv:{S.sel}:{at}`)
    local uvs = ensure_uvs(S, M.mesh(S))
    local lx, ly = to_local(f, wx, wy)
    uvs[at] = local_to_uv(S, lx, ly)
    apply(S)
end

-- The rig's bones, as paths relative to the rig root the polygon names.
local function rig_bone_paths(S)
    local node, ref = model.selected(S)
    local comp = M.component(node)
    if ref == nil or comp == nil then
        return nil
    end
    local rig = if comp.skeleton == nil or comp.skeleton == "" then ref else ref:get_node(comp.skeleton)
    if rig == nil then
        log.warn(`polygon rig '{comp.skeleton}' names no node`)
        return nil
    end
    local base = rig:path()
    local paths = {}
    for _, bone in skeleton.bones(rig) do
        local full = bone:path()
        if full ~= base then
            table.insert(paths, string.sub(full, #base + 2))
        end
    end
    return paths
end

-- Sync Bones to Polygon: the skin lists the rig's bones, in rig order,
-- keeping the weights of any it already had.
function M.sync_bones(S)
    local paths = rig_bone_paths(S)
    if paths == nil or not M.editable(S) then
        return
    end
    begin(S, "sync bones to polygon")
    local mesh = ensure_mesh(S)
    if mesh == nil then
        return
    end
    local had = {}
    if type(mesh.skin) == "table" and type(mesh.skin.bones) == "table" then
        for _, bone in mesh.skin.bones do
            had[bone.path] = bone.weights
        end
    end
    local bones = {}
    for _, path in paths do
        local weights = had[path] or {}
        for i = 1, #mesh.positions do
            weights[i] = weights[i] or 0
        end
        table.insert(bones, { path = path, weights = weights })
    end
    mesh.skin = { bones = bones }
    S.paint_bone = if #bones > 0 then 1 else nil
    apply(S)
    log.info(`{#bones} bones synced to the polygon`)
end

function M.bones(S)
    local mesh = M.mesh(S)
    if mesh == nil or type(mesh.skin) ~= "table" or type(mesh.skin.bones) ~= "table" then
        return {}
    end
    return mesh.skin.bones
end

-- Add `amount` (negative to unpaint) to the active bone's weight on vertex
-- `at`, clamped; the engine renormalises across bones when it loads.
function M.paint(S, at, amount)
    local mesh = M.mesh(S)
    if mesh == nil or S.paint_bone == nil or M.bones(S)[S.paint_bone] == nil
        or mesh.positions[at] == nil then
        return
    end
    begin(S, "paint weights", `poly:paint:{S.sel}`)
    local bone = M.bones(S)[S.paint_bone]
    bone.weights[at] = math.clamp((bone.weights[at] or 0) + amount, 0, 1)
    apply(S)
end

-- The vertex under the cursor, in position space or UV space.
local function pick(S, mesh, f, mx, my, in_uv)
    local best, best_d = nil, PICK_PX * world_per_px(S)
    for i, p in mesh.positions do
        local lx, ly
        if in_uv then
            local uv = (mesh.uvs or {})[i] or default_uv(S, p[1], p[2])
            lx, ly = uv_to_local(S, uv[1], uv[2])
        else
            lx, ly = p[1], p[2]
        end
        local wx, wy = to_world(f, lx, ly)
        local d = math.sqrt((wx - mx) ^ 2 + (wy - my) ^ 2)
        if d < best_d then
            best, best_d = i, d
        end
    end
    return best
end

local function line(a, b, color)
    render.draw_line_2d(a[1], a[2], b[1], b[2], color[1], color[2], color[3], W)
end

local function marker(S, x, y, color, filled)
    local h = MARK_PX * world_per_px(S)
    local width = if filled then 2 * MARK_PX else W
    render.draw_line_2d(x - h, y - h, x + h, y - h, color[1], color[2], color[3], width)
    render.draw_line_2d(x + h, y - h, x + h, y + h, color[1], color[2], color[3], width)
    render.draw_line_2d(x + h, y + h, x - h, y + h, color[1], color[2], color[3], width)
    render.draw_line_2d(x - h, y + h, x - h, y - h, color[1], color[2], color[3], width)
end

-- World points of every vertex, in position space or UV space.
local function world_points(S, mesh, f, in_uv)
    local out = {}
    for i, p in mesh.positions do
        local lx, ly = p[1], p[2]
        if in_uv then
            local uv = (mesh.uvs or {})[i] or default_uv(S, p[1], p[2])
            lx, ly = uv_to_local(S, uv[1], uv[2])
        end
        out[i] = { to_world(f, lx, ly) }
    end
    return out
end

function M.draw(S)
    if S.tool ~= "polygon" or not S.mode2d then
        return
    end
    local mesh = M.mesh(S)
    local f = frame(S)
    if mesh == nil or f == nil then
        return
    end
    local k = theme.current
    local accent = { theme.hex_to_rgb(k.accent) }
    local sage = { theme.hex_to_rgb(k.sage) }
    local mode = S.polygon_mode or "points"
    local points = world_points(S, mesh, f, mode == "uv")
    local outline = outline_count(mesh)
    -- The outline, closed; then the drawn polygons over it.
    for i = 1, outline do
        local j = if i == outline then 1 else i + 1
        if points[i] ~= nil and points[j] ~= nil then
            line(points[i], points[j], accent)
        end
    end
    if type(mesh.polygons) == "table" then
        for _, ring in mesh.polygons do
            for i, index in ring do
                local next = ring[if i == #ring then 1 else i + 1]
                local a, b = points[index + 1], points[next + 1]
                if a ~= nil and b ~= nil then
                    line(a, b, sage)
                end
            end
        end
    end
    if S.polygon_loop ~= nil then
        for i = 2, #S.polygon_loop do
            line(points[S.polygon_loop[i - 1]], points[S.polygon_loop[i]], HOVER)
        end
    end
    local bones = M.bones(S)
    local active = if S.paint_bone ~= nil then bones[S.paint_bone] else nil
    for i, p in points do
        local color = if i == S.polygon_hover then HOVER else accent
        if mode == "weights" and active ~= nil then
            local w = active.weights[i] or 0
            color = { w, w, w }
        end
        marker(S, p[1], p[2], color, i > outline)
    end
end

-- A translucent copy of the texture, undeformed, under the UV points.
local function enter_uv(S)
    if S.uv_backdrop ~= nil and S.uv_backdrop:is_valid() then
        return
    end
    local node = S.doc[S.sel]
    local comp = M.component(node)
    if comp == nil or comp.texture == nil or comp.texture == "" or S.mirror == nil then
        return
    end
    local backdrop = S.mirror:add_child("UvBackdrop")
    backdrop:set_component("sprite", {
        texture = S.game_root .. "/" .. comp.texture,
        pixels_per_unit = comp.pixels_per_unit or 100,
        color = { 1, 1, 1, 0.55 },
    })
    S.uv_backdrop = backdrop
end

local function leave_uv(S)
    if S.uv_backdrop ~= nil then
        if S.uv_backdrop:is_valid() then
            S.uv_backdrop:queue_free()
        end
        S.uv_backdrop = nil
    end
end

function M.set_mode(S, mode)
    S.polygon_mode = mode
    S.polygon_loop = nil
    if mode ~= "uv" then
        leave_uv(S)
    end
    if mode == "weights" and S.paint_bone == nil and #M.bones(S) > 0 then
        S.paint_bone = 1
    end
end

-- Runs every frame whatever the tool: the backdrop follows the polygon and
-- goes away with the mode.
function M.idle(S)
    if S.tool ~= "polygon" or (S.polygon_mode or "points") ~= "uv" or M.mesh(S) == nil then
        leave_uv(S)
        return
    end
    enter_uv(S)
    local f = frame(S)
    if S.uv_backdrop ~= nil and S.uv_backdrop:is_valid() and f ~= nil then
        local _, ref = model.selected(S)
        local x, y, z = table.unpack(ref:global_position())
        local rx, ry, rz = table.unpack(ref:global_rotation_euler())
        S.uv_backdrop:set_position(x, y, z)
        S.uv_backdrop:set_rotation_euler(rx, ry, rz)
        S.uv_backdrop:set_scale(f.sx, f.sy, 1)
    end
end

function M.cancel(S)
    S.polygon_loop = nil
    S.polygon_drag = nil
end

local function alt_down()
    return input.is_down("LAlt") or input.is_down("RAlt")
end

-- Runs from the 2D gizmo while the Polygon tool is active.
function M.update(S, over_viewport, mx, my)
    local mode = S.polygon_mode or "points"
    local mesh = M.mesh(S)
    local f = frame(S)
    if f == nil then
        return
    end
    if mesh == nil then
        S.polygon_hover = nil
        if over_viewport and input.mouse_just_pressed(LEFT) and mode == "points" then
            M.add_vertex(S, mx, my, alt_down())
        end
        return
    end
    local hover = if over_viewport then pick(S, mesh, f, mx, my, mode == "uv") else nil
    S.polygon_hover = hover
    if not input.is_mouse_down(LEFT) then
        S.polygon_drag = nil
    end
    if S.polygon_drag ~= nil then
        if mode == "uv" then
            M.move_uv(S, S.polygon_drag, mx, my)
        else
            M.move_vertex(S, S.polygon_drag, mx, my)
        end
        return
    end
    if not over_viewport then
        return
    end
    if mode == "points" then
        if input.mouse_just_pressed(LEFT) then
            if hover ~= nil then
                S.polygon_drag = hover
            else
                M.add_vertex(S, mx, my, alt_down())
            end
        elseif input.mouse_just_pressed(RIGHT) and hover ~= nil then
            M.remove_vertex(S, hover)
        end
    elseif mode == "polygons" then
        if input.mouse_just_pressed(LEFT) and hover ~= nil then
            local ring = S.polygon_loop or {}
            if #ring >= 3 and hover == ring[1] then
                S.polygon_loop = ring
                M.close_loop(S)
            elseif not table.find(ring, hover) then
                table.insert(ring, hover)
                S.polygon_loop = ring
            end
        elseif input.mouse_just_pressed(RIGHT) then
            S.polygon_loop = nil
        end
    elseif mode == "uv" then
        if input.mouse_just_pressed(LEFT) and hover ~= nil then
            S.polygon_drag = hover
        end
    elseif mode == "weights" then
        local sign = if input.is_mouse_down(LEFT) then 1 elseif input.is_mouse_down(RIGHT) then -1 else 0
        if sign ~= 0 then
            local radius = (S.paint_radius or 40) * world_per_px(S)
            local amount = sign * (S.paint_strength or 0.5) * engine.delta() * 2
            for i, p in world_points(S, mesh, f, false) do
                if math.sqrt((p[1] - mx) ^ 2 + (p[2] - my) ^ 2) <= radius then
                    M.paint(S, i, amount)
                end
            end
        end
    end
end

-- The four modes as pills in the viewport's chip row.
function M.mode_chips(S, k)
    local mode = S.polygon_mode or "points"
    for _, id in M.MODES do
        local on = mode == id
        if ui.pill(M.mode_names[id], {
            height = 26, size = 11, strong = true,
            fill = if on then k.accent_soft else k.panel,
            stroke = k.line_soft,
            color = if on then k.accent else k.dim,
        }) then
            M.set_mode(S, id)
        end
    end
end

return M
