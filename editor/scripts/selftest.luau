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

return M
