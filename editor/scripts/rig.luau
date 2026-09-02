-- Rigging: bones drawn as diamonds (2D) or octahedra (3D), the Rig bone tool
-- that grows a chain, and the two rest-pose verbs. A bone is a node with
-- `bone2d` or `bone3d`; this module only reads and writes those.
local model = require("scripts/model")
local history = require("scripts/history")
local theme = require("scripts/theme")

local M = {}

local HOVER = { 0.118, 0.314, 0.784 }
local W = 2.4
-- A tip bone with no length set still needs a diamond to pick.
local TIP_LENGTH = 0.35
local PICK_PX = 10

local function world_per_px(S)
    local _, _, zoom = render.camera_2d()
    return S.ui_scale / math.max(zoom, 0.01)
end

function M.is_bone(node)
    return node ~= nil and (type(node.bone2d) == "table" or type(node.bone3d) == "table")
end

local function bone_data(node)
    return node.bone2d or node.bone3d
end

-- Doc index of the first child bone of doc node `i`, or nil.
local function first_child_bone(S, i)
    local id = S.doc[i].id
    for j, node in S.doc do
        if node.parent == id and M.is_bone(node) then
            return j
        end
    end
    return nil
end

-- A bone's world geometry: its origin and the tip its diamond points at —
-- the first child bone, or `length` along `angle` for a tip bone.
function M.geometry(S, i)
    local node = S.doc[i]
    local ref = model.ref(S, i)
    if ref == nil or not M.is_bone(node) then
        return nil
    end
    local ox, oy, oz = table.unpack(ref:global_position())
    local child = first_child_bone(S, i)
    local tx, ty, tz
    if child ~= nil and model.ref(S, child) ~= nil then
        tx, ty, tz = table.unpack(model.ref(S, child):global_position())
    else
        local bone = bone_data(node)
        local length = bone.length or 0
        if length <= 0 then
            length = TIP_LENGTH
        end
        local _, _, angle = table.unpack(ref:global_rotation_euler())
        angle += bone.angle or 0
        local sx = table.unpack(ref:global_scale())
        -- A 3D tip bone points along its own y, the way an imported joint
        -- chain grows; a 2D one along `angle`.
        if node.bone3d ~= nil then
            tx, ty, tz = ox, oy + length * sx, oz
        else
            tx, ty, tz = ox + math.cos(angle) * length * sx, oy + math.sin(angle) * length * sx, oz
        end
    end
    return { origin = { ox, oy, oz }, tip = { tx, ty, tz } }
end

local function segment_distance(p, a, b)
    local abx, aby = b[1] - a[1], b[2] - a[2]
    local len2 = abx * abx + aby * aby
    local t = 0
    if len2 > 1e-9 then
        t = math.clamp(((p[1] - a[1]) * abx + (p[2] - a[2]) * aby) / len2, 0, 1)
    end
    local cx, cy = a[1] + abx * t, a[2] + aby * t
    return math.sqrt((p[1] - cx) ^ 2 + (p[2] - cy) ^ 2)
end

-- The bone under the cursor: the one whose diamond's spine is nearest,
-- within a screen-space radius.
function M.pick(S, mx, my)
    local best, best_d = nil, PICK_PX * world_per_px(S)
    for i, node in S.doc do
        if M.is_bone(node) then
            local g = M.geometry(S, i)
            if g ~= nil then
                local d = segment_distance({ mx, my }, g.origin, g.tip)
                if d < best_d then
                    best, best_d = i, d
                end
            end
        end
    end
    return best
end

-- The bone under the cursor in a 3D viewport: nearest projected spine, in
-- design px. `project(point)` is the 3D gizmo's, handed in so this module
-- stays free of camera math.
function M.pick_3d(S, mx, my, project)
    local best, best_d = nil, PICK_PX
    for i, node in S.doc do
        if M.is_bone(node) then
            local g = M.geometry(S, i)
            if g ~= nil then
                local ax, ay = project(g.origin)
                local bx, by = project(g.tip)
                if ax ~= nil and bx ~= nil then
                    local d = segment_distance({ mx, my }, { ax, ay }, { bx, by })
                    if d < best_d then
                        best, best_d = i, d
                    end
                end
            end
        end
    end
    return best
end

local function line(a, b, color)
    render.draw_line_2d(a[1], a[2], b[1], b[2], color[1], color[2], color[3], W)
end

-- Godot's bone: a diamond from origin to tip, widest a fifth of the way.
local function draw_bone(S, g, color)
    local dx, dy = g.tip[1] - g.origin[1], g.tip[2] - g.origin[2]
    local len = math.sqrt(dx * dx + dy * dy)
    if len < 1e-6 then
        return
    end
    local nx, ny = -dy / len, dx / len
    local half = math.min(0.12 * len, 6 * world_per_px(S))
    local jx, jy = g.origin[1] + dx * 0.2, g.origin[2] + dy * 0.2
    local a = { jx + nx * half, jy + ny * half }
    local b = { jx - nx * half, jy - ny * half }
    line(g.origin, a, color)
    line(a, g.tip, color)
    line(g.tip, b, color)
    line(b, g.origin, color)
end

-- A 3D bone: an octahedron from origin to tip, its waist a fifth of the way.
local function draw_bone_3d(g, color)
    local dx, dy, dz = g.tip[1] - g.origin[1], g.tip[2] - g.origin[2], g.tip[3] - g.origin[3]
    local len = math.sqrt(dx * dx + dy * dy + dz * dz)
    if len < 1e-6 then
        return
    end
    -- Two directions across the bone: any vector not along it, crossed.
    local ax, ay, az = 0, 0, 1
    if math.abs(dz) > 0.9 * len then
        ax, ay, az = 1, 0, 0
    end
    local ux, uy, uz = dy * az - dz * ay, dz * ax - dx * az, dx * ay - dy * ax
    local ul = math.sqrt(ux * ux + uy * uy + uz * uz)
    ux, uy, uz = ux / ul, uy / ul, uz / ul
    local vx, vy, vz = dy * uz - dz * uy, dz * ux - dx * uz, dx * uy - dy * ux
    local vl = math.sqrt(vx * vx + vy * vy + vz * vz)
    vx, vy, vz = vx / vl, vy / vl, vz / vl
    local half = 0.1 * len
    local jx, jy, jz = g.origin[1] + dx * 0.2, g.origin[2] + dy * 0.2, g.origin[3] + dz * 0.2
    local waist = {
        { jx + ux * half, jy + uy * half, jz + uz * half },
        { jx + vx * half, jy + vy * half, jz + vz * half },
        { jx - ux * half, jy - uy * half, jz - uz * half },
        { jx - vx * half, jy - vy * half, jz - vz * half },
    }
    for i, w in waist do
        local n = waist[i % 4 + 1]
        render.draw_line(g.origin[1], g.origin[2], g.origin[3], w[1], w[2], w[3], color[1], color[2], color[3], W)
        render.draw_line(w[1], w[2], w[3], g.tip[1], g.tip[2], g.tip[3], color[1], color[2], color[3], W)
        render.draw_line(w[1], w[2], w[3], n[1], n[2], n[3], color[1], color[2], color[3], W)
    end
end

-- Every bone, whenever rigging is what the user is doing: the Animate
-- persona, the tool, or a selected bone.
function M.draw(S)
    local show = S.persona == "anim" or S.tool == "bone" or M.is_bone(S.doc[S.sel])
    if not show then
        return
    end
    local k = theme.current
    local sage = { theme.hex_to_rgb(k.sage) }
    local accent = { theme.hex_to_rgb(k.accent) }
    for i, node in S.doc do
        if M.is_bone(node) then
            local g = M.geometry(S, i)
            if g ~= nil then
                local color = if i == S.rig_hover then HOVER elseif i == S.sel then accent else sage
                if S.mode2d then
                    draw_bone(S, g, color)
                else
                    draw_bone_3d(g, color)
                end
            end
        end
    end
end

-- A bone name no node has yet.
local function fresh_name(S)
    local taken = {}
    for _, node in S.doc do
        taken[node.name] = true
    end
    local name = "Bone"
    local n = 1
    while taken[name] do
        n += 1
        name = `Bone{n}`
    end
    return name
end

-- A new bone under doc node `parent_i`, at world point (wx, wy), posed at
-- its rest there. Returns the new doc index.
function M.add_bone(S, parent_i, wx, wy)
    local parent = S.doc[parent_i]
    local pref = model.ref(S, parent_i)
    if pref == nil then
        log.warn(`{parent.name}: no live node to hang a bone on`)
        return nil
    end
    history.record(S, "add bone")
    local lx, ly = 0, 1
    if S.mode2d then
        -- The click, in the parent's own frame.
        local px, py = table.unpack(pref:global_position())
        local _, _, angle = table.unpack(pref:global_rotation_euler())
        local sx, sy = table.unpack(pref:global_scale())
        local dx, dy = wx - px, wy - py
        local c, s = math.cos(angle), math.sin(angle)
        lx = (c * dx + s * dy) / (if math.abs(sx) > 1e-6 then sx else 1)
        ly = (-s * dx + c * dy) / (if math.abs(sy) > 1e-6 then sy else 1)
    end
    local name = fresh_name(S)
    local child = pref:add_child(name)
    child:set_position(lx, ly, 0)
    -- A 3D rig has no click to place a bone by, so it grows one unit up.
    local component = if S.mode2d then "bone2d" else "bone3d"
    if S.mode2d then
        child:set_component(component, { rest_position = { lx, ly }, rest_rotation = 0 })
    else
        child:set_component(component, { rest_position = { lx, ly, 0 } })
    end
    local entry = {
        id = model.fresh_id(S.doc, name),
        name = name,
        parent = parent.id or "",
        position = { lx, ly, 0 },
        [component] = child:get_component(component),
    }
    table.insert(S.doc, entry)
    S.refs[#S.doc] = child
    S.sel = #S.doc
    log.info(`bone {name} under {parent.name}`)
    return #S.doc
end

-- Runs from the 2D gizmo while the Rig bone tool is active: a click on a bone
-- selects it, a click anywhere else grows a bone under the selection.
function M.update(S, over_viewport, mx, my)
    -- No 3D picking: a 3D rig is selected in the tree and grown from the
    -- inspector's Skeleton section.
    if not S.mode2d then
        S.rig_hover = nil
        return
    end
    S.rig_hover = if over_viewport then M.pick(S, mx, my) else nil
    if not (over_viewport and input.mouse_just_pressed(input.MOUSE_LEFT)) then
        return
    end
    if S.rig_hover ~= nil then
        S.select(S.rig_hover)
        return
    end
    M.add_bone(S, S.sel, mx, my)
end

-- How many bones the selection's subtree holds (itself included).
function M.bone_count(S, i)
    local ref = model.ref(S, i)
    if ref == nil then
        return 0
    end
    return #skeleton.bones(ref)
end

-- Mirror the live bones under doc node `root_i` back into the document,
-- either their transforms or their `bone2d`, after a rest verb moved one.
local function sync_bones(S, root_i, transforms)
    local root = model.ref(S, root_i)
    if root == nil then
        return
    end
    for _, bone in skeleton.bones(root) do
        for i in S.doc do
            local ref = model.ref(S, i)
            if ref ~= nil and ref == bone then
                if transforms then
                    S.doc[i].position = ref:position()
                    S.doc[i].rotation_euler = ref:rotation_euler()
                    S.doc[i].scale = ref:scale()
                else
                    local component = if S.doc[i].bone3d ~= nil then "bone3d" else "bone2d"
                    S.doc[i][component] = ref:get_component(component)
                end
            end
        end
    end
end

-- Reset to Rest Pose, for every bone under the selection.
function M.apply_rest(S)
    local ref = model.ref(S, S.sel)
    if ref == nil then
        return
    end
    history.record(S, "reset to rest pose")
    skeleton.apply_rest(ref)
    sync_bones(S, S.sel, true)
end

-- Overwrite Rest Pose: the pose every bone under the selection is in now
-- becomes the one it returns to.
function M.overwrite_rest(S)
    local ref = model.ref(S, S.sel)
    if ref == nil then
        return
    end
    history.record(S, "overwrite rest pose")
    skeleton.overwrite_rest(ref)
    sync_bones(S, S.sel, false)
end

return M
