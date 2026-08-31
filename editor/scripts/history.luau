-- Undo/redo over the scene document. `model.build_mirror` rebuilds the live
-- scene from the document, so restoring a document restores everything.
local M = {}

-- Whole documents rather than inverse commands: an inverse per property would
-- put per-type knowledge back where the schema-driven inspector removed it.
local MAX = 100
-- A drag reports a change every frame; records sharing a key inside this many
-- seconds are one step. `engine.time()` advances once per frame, which is the
-- resolution this wants.
local COALESCE = 0.4

local function clone(value)
    if type(value) ~= "table" then
        return value
    end
    local out = {}
    for key, inner in value do
        out[key] = clone(inner)
    end
    return out
end

local function frame(S, label)
    return { doc = clone(S.doc), sel = S.sel, label = label }
end

function M.reset(S)
    S.history = { past = {}, future = {}, saved = 0, key = nil, at = -1 }
end

-- Called by every mutator *before* it mutates. `key` coalesces a continuing
-- drag into the step it started; omit it for a discrete action.
function M.record(S, label, key)
    local h = S.history
    if h == nil then
        return
    end
    local now = engine.time()
    if key ~= nil and h.key == key and now - h.at < COALESCE then
        h.at = now
        return
    end
    h.key = key
    h.at = now
    table.insert(h.past, frame(S, label))
    if #h.past > MAX then
        table.remove(h.past, 1)
        -- The saved point scrolled off the end: nothing can reach it again.
        h.saved = if h.saved > 0 then h.saved - 1 else -1
    end
    h.future = {}
end

-- Break any open coalescing run, so the next edit starts its own step. Called
-- when the selection moves or a drag ends.
function M.commit(S)
    if S.history ~= nil then
        S.history.key = nil
    end
end

local function restore(S, into, from)
    local top = table.remove(from)
    if top == nil then
        return nil
    end
    table.insert(into, frame(S, top.label))
    S.doc = top.doc
    S.sel = math.clamp(top.sel or 1, 1, #top.doc)
    S.history.key = nil
    return top.label
end

-- Both return the label of the step they moved through, or nil when the stack
-- is empty. The caller rebuilds the mirror: history owns data, not the engine.
function M.undo(S)
    return restore(S, S.history.future, S.history.past)
end

function M.redo(S)
    return restore(S, S.history.past, S.history.future)
end

function M.can_undo(S)
    return S.history ~= nil and #S.history.past > 0
end

function M.can_redo(S)
    return S.history ~= nil and #S.history.future > 0
end

-- What ⌘Z would undo, for a menu or palette label.
function M.undo_label(S)
    local top = S.history.past[#S.history.past]
    return if top ~= nil then top.label else nil
end

function M.redo_label(S)
    local top = S.history.future[#S.history.future]
    return if top ~= nil then top.label else nil
end

function M.mark_saved(S)
    if S.history ~= nil then
        S.history.saved = #S.history.past
    end
end

-- Position in the stack, not a latch: undoing back to the last save is clean
-- again, which a boolean set by every mutator could never say.
function M.dirty(S)
    local h = S.history
    if h == nil then
        return false
    end
    return h.saved < 0 or #h.past ~= h.saved
end

return M
