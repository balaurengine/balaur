-- Start-up states that exercise the editor and assert the result. `log.error`
-- is the failure channel: `scripts/e2e.sh` fails any run that logs one.
local model = require("scripts/model")
local history = require("scripts/history")

local M = {}

local function check(ok, what)
    if ok then
        log.info(`selftest ok: {what}`)
    else
        log.error(`selftest FAILED: {what}`)
    end
    return ok
end

-- Every document node must resolve to a live node — the invariant e2e already
-- checks at start-up, asserted here after each mutation instead.
local function mirror_resolves(S)
    for i in S.doc do
        if model.ref(S, i) == nil then
            return false
        end
    end
    return true
end

local function name_at(S, i)
    local node = S.doc[i]
    return if node ~= nil then node.name else "<none>"
end

-- The first writable float on any component of the selection, so the test
-- exercises `set_component_prop` on whatever the scene actually has.
local function writable_float(ref)
    for _, comp in ref:component_names() do
        for prop, spec in (scene.component_schema(comp) or {}) do
            if spec.type == "float" and not spec.readonly then
                return comp, prop, spec
            end
        end
    end
    return nil
end

function M.component_write(S)
    local _, ref = model.selected(S)
    if ref == nil then
        return
    end
    local comp, prop, spec = writable_float(ref)
    if comp == nil then
        log.info("selftest: the selection has no writable float; skipped")
        return
    end
    local was = (ref:get_component(comp) or {})[prop] or spec.default or 0
    model.set_component_prop(S, comp, prop, was + 3)
    local now = (model.ref(S, S.sel):get_component(comp) or {})[prop]
    check(math.abs(now - (was + 3)) < 1e-6, `{comp}.{prop} took the write ({now})`)
    S.undo()
    local back = (model.ref(S, S.sel):get_component(comp) or {})[prop]
    check(math.abs(back - was) < 1e-6, `undo restores {comp}.{prop} ({back} vs {was})`)
end

-- Mutate, undo, redo; assert the document, the mirror and the selection all
-- come back. Runs headless, so it is CI's only view of the editor's Luau.
function M.undo(S)
    local start = #S.doc
    check(history.dirty(S) == false, "a freshly loaded scene is clean")
    check(history.can_undo(S) == false, "nothing to undo on load")

    -- Duplicate: the case that used to leave a node with no live node at all.
    for i, node in S.doc do
        if node.name == "Bird" then
            S.select(i)
        end
    end
    local picked = name_at(S, S.sel)
    model.duplicate_selected(S)
    check(#S.doc == start + 1, `duplicate adds a node ({#S.doc})`)
    check(mirror_resolves(S), "duplicate resolves every node in the mirror")
    check(history.dirty(S), "duplicating marks the scene dirty")
    local copy = name_at(S, S.sel)

    S.undo()
    check(#S.doc == start, `undo removes it again ({#S.doc})`)
    check(mirror_resolves(S), "undo resolves every node in the mirror")
    check(name_at(S, S.sel) == picked, `undo restores the selection ({name_at(S, S.sel)})`)
    check(history.dirty(S) == false, "undo back to the load point is clean")

    S.redo()
    check(#S.doc == start + 1, `redo adds it back ({#S.doc})`)
    check(name_at(S, S.sel) == copy, `redo restores the selection ({name_at(S, S.sel)})`)
    check(mirror_resolves(S), "redo resolves every node in the mirror")

    -- Delete: the destructive one, and the one that moves the selection.
    local before_delete = #S.doc
    model.delete_selected(S)
    check(#S.doc == before_delete - 1, `delete removes a node ({#S.doc})`)
    S.undo()
    check(#S.doc == before_delete, `undo restores the deleted node ({#S.doc})`)
    check(mirror_resolves(S), "the restored node resolves in the mirror")

    -- A transform write, the way the inspector's vector rows make one.
    local node, ref = model.selected(S)
    if ref ~= nil then
        local x, y, z = table.unpack(ref:position())
        history.record(S, "move node", "selftest:move")
        history.commit(S)
        node.position = { x + 5, y, z }
        ref:set_position(x + 5, y, z)
        S.undo()
        local rx = table.unpack(model.selected(S).position or { 0 })
        check(math.abs(rx - x) < 1e-6, `undo restores the position ({rx} vs {x})`)
    end

    -- A component property, discovered rather than named: every scene has
    -- different components, and a 3D one has no `shape2d` to write to.
    M.component_write(S)

    -- The stack is finite and ordered: undo everything, then nothing is left.
    local guard = 0
    while history.can_undo(S) and guard < 200 do
        S.undo()
        guard += 1
    end
    check(#S.doc == start, `undoing everything returns the original scene ({#S.doc})`)
    check(history.dirty(S) == false, "the fully-undone scene is clean again")
    check(mirror_resolves(S), "the fully-undone scene resolves in the mirror")
    log.info("selftest undo: done")
end

-- Grow a bone, round-trip the rest pose, key the bone on its player's clip
-- by path, then undo it all. Skips scenes with no rig.
function M.rig(S)
    local riglib = require("scripts/rig")
    local animlib = require("scripts/anim")
    local first
    for i, node in S.doc do
        if riglib.is_bone(node) then
            first = i
            break
        end
    end
    if first == nil then
        log.info("selftest rig: no bones; skipped")
        return
    end
    local start = #S.doc
    S.select(first)
    local bone_name = name_at(S, first)

    local added = riglib.add_bone(S, first, 1.0, 2.0)
    check(added ~= nil and #S.doc == start + 1, `rig bone adds a node ({#S.doc})`)
    check(mirror_resolves(S), "the new bone resolves in the mirror")
    check(riglib.is_bone(S.doc[#S.doc]), "the new node carries a bone component")
    check(S.doc[#S.doc].parent == S.doc[first].id, `the new bone hangs under {bone_name}`)

    -- Rest round trip: record the pose, move, reset, and it is back.
    S.select(first)
    local ref = model.ref(S, first)
    local x, y, z = table.unpack(ref:position())
    riglib.overwrite_rest(S)
    ref:set_position(x + 1, y, z)
    riglib.apply_rest(S)
    local bx = table.unpack(model.ref(S, first):position())
    check(math.abs(bx - x) < 1e-5, `reset to rest pose puts {bone_name} back ({bx} vs {x})`)
    check(math.abs((S.doc[first].position or { 0 })[1] - x) < 1e-5, "the document follows the reset")

    -- Keying a bone lands on the player above it, addressed by path.
    local player_i = animlib.player_index(S)
    local target = animlib.target_path(S, player_i)
    animlib.add_key(S, 0.0)
    local found, positioned = false, false
    for _, track in animlib.tracks(animlib.definition(S)) do
        if (track.target or "") == target then
            found = found or track.property == "rotation_euler"
            positioned = positioned or track.property == "position"
        end
    end
    check(found, `keying {bone_name} writes a rotation track at '{target}'`)
    check(not positioned, "a bone keys no position track")

    local guard = 0
    while history.can_undo(S) and guard < 200 do
        S.undo()
        guard += 1
    end
    check(#S.doc == start, `undoing everything returns the original scene ({#S.doc})`)
    check(mirror_resolves(S), "the fully-undone scene resolves in the mirror")
    log.info("selftest rig: done")
end

-- Trace a vertex onto a polygon, sync its bones from the rig, paint one,
-- draw a polygon, remove a vertex, then undo it all. Skips scenes with no
-- inline polygon.
function M.polygon(S)
    local polygonlib = require("scripts/polygon")
    local picked
    for i, node in S.doc do
        if polygonlib.component(node) ~= nil and type(node.polygon.mesh) == "table" then
            picked = i
            break
        end
    end
    if picked == nil then
        log.info("selftest polygon: no inline polygon; skipped")
        return
    end
    S.select(picked)
    local name = name_at(S, picked)
    local mesh = polygonlib.mesh(S)
    local vertices = #mesh.positions
    local internal = math.floor(mesh.internal or 0)

    local at = polygonlib.add_vertex(S, 3.0, 3.0, false)
    check(at == vertices - internal + 1, `an outline vertex goes after the last outline one ({at})`)
    check(#polygonlib.mesh(S).positions == vertices + 1, "add vertex grows the mesh")
    local live = model.ref(S, picked):get_component("polygon")
    check(live ~= nil and string.sub(live.mesh, 1, 2) == "#!", "the live polygon re-read its inline mesh")

    polygonlib.sync_bones(S)
    local bones = polygonlib.bones(S)
    check(#bones > 0, `sync bones lists the rig's bones ({#bones})`)
    check(#bones == 0 or #bones[1].weights == vertices + 1, "every bone has one weight per vertex")
    if #bones > 0 then
        S.paint_bone = 1
        polygonlib.paint(S, at, 1.0)
        check(bones[1].weights[at] == 1, "painting sets the vertex's weight")
    end

    local internal_at = polygonlib.add_vertex(S, 0.1, 0.1, true)
    check(math.floor(polygonlib.mesh(S).internal or 0) == internal + 1, "an alt-click adds an internal vertex")
    S.polygon_loop = { 1, 2, internal_at }
    polygonlib.close_loop(S)
    local polygons = polygonlib.mesh(S).polygons or {}
    check(#polygons >= 1, `closing a loop draws a polygon ({#polygons})`)

    polygonlib.remove_vertex(S, at)
    check(#polygonlib.mesh(S).positions == vertices + 1, "removing the traced vertex shrinks the mesh")
    check(mirror_resolves(S), "the polygon node still resolves")

    local guard = 0
    while history.can_undo(S) and guard < 200 do
        S.undo()
        guard += 1
    end
    local back = polygonlib.mesh(S)
    check(#back.positions == vertices, `undoing everything restores {name}'s vertices ({#back.positions})`)
    check(math.floor(back.internal or 0) == internal, "and its internal count")
    log.info("selftest polygon: done")
end

return M
