-- The edited project: scene document model, live mirror scene, save/reset.
-- `S.doc` is plain data parsed from the scene TOML; the mirror is a real node
-- tree from the same TOML (scripts not attached, physics paused until play).
local M = {}

-- A scene node's `parent` is the parent's **id**, not a path
-- (`crates/balaur_core/src/project.rs`); everything below walks that id chain.
local function by_id(doc, id)
    for _, node in doc do
        if node.id == id then
            return node
        end
    end
    return nil
end
M.by_id = by_id

-- A hand-edited file can name a parent that names it back. Every walk is
-- bounded so a cycle costs a wrong answer, not a hung editor.
local MAX_DEPTH = 256

-- The name path `scene.get_node` resolves, built by walking ids to the root.
local function node_path(doc, node)
    local names = { node.name }
    local current = node
    for _ = 1, MAX_DEPTH do
        if current.parent == nil or current.parent == "" then
            break
        end
        local parent = by_id(doc, current.parent)
        if parent == nil then
            break
        end
        table.insert(names, 1, parent.name)
        current = parent
    end
    return table.concat(names, "/")
end
M.node_path = node_path

-- Whether `id` is an ancestor of `node`, for subtree operations.
local function has_ancestor(doc, node, id)
    local current = node
    for _ = 1, MAX_DEPTH do
        if current.parent == nil or current.parent == "" then
            return false
        end
        if current.parent == id then
            return true
        end
        current = by_id(doc, current.parent)
        if current == nil then
            return false
        end
    end
    return false
end
M.has_ancestor = has_ancestor

-- The engine's own id rule (`slug` in project.rs), so an id the editor
-- generates is the one the engine would have generated for that name.
local function slug(name)
    local out = "n_"
    local gap = true
    for i = 1, #name do
        local c = string.sub(name, i, i)
        if string.match(c, "^[%a%d]$") ~= nil and string.byte(c) < 128 then
            out ..= string.lower(c)
            gap = false
        elseif not gap then
            out ..= "_"
            gap = true
        end
    end
    out = string.gsub(out, "_+$", "")
    return if #out > 2 then out else "n_node"
end

-- An id no node in `doc` is using, derived from `name`.
local function fresh_id(doc, name)
    local taken = {}
    for _, node in doc do
        if node.id ~= nil then
            taken[node.id] = true
        end
    end
    local base = slug(name)
    local candidate = base
    local n = 2
    while taken[candidate] do
        candidate = `{base}_{n}`
        n += 1
    end
    return candidate
end
M.fresh_id = fresh_id

-- A friendly type name derived from the node's components.
function M.node_type(node)
    local kind = M.body_kind(node)
    local kind2d = M.body2d_kind(node)
    if kind == "dynamic" then
        return "RigidBody3D"
    elseif kind == "static" then
        return "StaticBody3D"
    elseif kind == "kinematic" then
        return "KinematicBody3D"
    elseif kind2d == "dynamic" then
        return "RigidBody2D"
    elseif kind2d == "static" then
        return "StaticBody2D"
    elseif kind2d == "kinematic" then
        return "KinematicBody2D"
    elseif node.widget ~= nil then
        return "Widget"
    elseif node.shape ~= nil then
        return "MeshInstance3D"
    elseif node.shape2d ~= nil then
        return "MeshInstance2D"
    end
    return "Node"
end

function M.body_kind(node)
    if type(node.body) == "table" then
        return node.body.kind
    end
    return node.body
end

function M.body2d_kind(node)
    if type(node.body2d) == "table" then
        return node.body2d.kind
    end
    return node.body2d
end

-- True when the edited scene is 2D (any node carries a 2D component). The
-- editor switches camera, grid and gizmo accordingly.
function M.is_2d(S)
    for _, node in S.doc do
        if node.shape2d ~= nil or node.body2d ~= nil or node.collider2d ~= nil then
            return true
        end
    end
    return false
end

-- Icon glyph + color role ("sage" physics, "accent" render/ui, "dim" other).
function M.node_icon(node)
    local kind = M.body_kind(node) or M.body2d_kind(node)
    if node.widget ~= nil then
        return "▤", "accent"
    elseif kind == "dynamic" then
        return "●", "sage"
    elseif kind == "kinematic" then
        return "◐", "sage"
    elseif kind ~= nil then
        return "▣", "sage"
    elseif node.shape ~= nil then
        if node.shape.kind == "ball" then
            return "◯", "accent"
        end
        return "◆", "accent"
    elseif node.shape2d ~= nil then
        if node.shape2d.kind == "circle" then
            return "◯", "accent"
        end
        return "▭", "accent"
    elseif node.script ~= nil then
        return "⚙", "dim"
    end
    return "○", "faint"
end

-- Load the game project pointed at by `engine.args()[1]`.
function M.load(S)
    local root = engine.args()[1]
    S.game_root = root
    local manifest = toml.parse(fs.read(root .. "/project.toml"))
    S.game_name = manifest.name
    S.scene_rel = manifest.main_scene
    S.scene_path = root .. "/" .. manifest.main_scene
    local doc = toml.parse(fs.read(S.scene_path))
    S.doc = doc.nodes or {}
    for _, node in S.doc do
        if node.parent == nil then
            node.parent = ""
        end
        -- The engine repairs a missing id at load; the editor has to arrive at
        -- the same one or its refs point at nothing.
        if node.id == nil or node.id == "" then
            node.id = fresh_id(S.doc, node.name)
        end
    end
    S.sel = 1
    S.dirty = false
    M.build_mirror(S)
end

-- (Re)build the live mirror under a fresh container node.
function M.build_mirror(S)
    if S.mirror ~= nil and S.mirror:is_valid() then
        S.mirror:queue_free()
    end
    physics.clear()
    S.mirror = scene.spawn("Edited")
    local src = fs.read(S.scene_path)
    scene.instantiate(src, S.mirror, { scripts = false })
    physics.set_paused(true)
    S.playing = false
    S.refs = {}
    local unresolved = 0
    for i, node in S.doc do
        local path = node_path(S.doc, node)
        S.refs[i] = S.mirror:get_node(path)
        if S.refs[i] == nil then
            unresolved += 1
            log.warn(`no mirror node at '{path}' for '{node.name}'`)
        end
    end
    -- A node with no ref is invisible to the inspector, the gizmo and every
    -- transform read. Silence here is what let a whole class of nested nodes
    -- go missing unnoticed, so it says so now.
    if unresolved > 0 then
        log.warn(`{unresolved} of {#S.doc} nodes did not resolve in the mirror`)
    end
    -- The mirror instantiates with the game's own `autoplay`, so pause every
    -- animated node: nothing may animate under the inspector until play is pressed.
    for i, node in S.doc do
        local ref = S.refs[i]
        if ref ~= nil and type(node.animation) == "table" then
            animation.pause(ref)
        end
    end
    -- The clip cache belongs to the mirror that is gone, and every clip file
    -- the new mirror names has to be pointed at the edited project before it
    -- can load (`anim.rebind`, next frame).
    S.clip = nil
    S.anim_rebind = true
end

-- The live node for a doc index (may be nil right after a rebuild).
function M.ref(S, i)
    local ref = S.refs[i]
    if ref ~= nil and ref:is_valid() then
        return ref
    end
    return nil
end

function M.selected(S)
    return S.doc[S.sel], M.ref(S, S.sel)
end

-- Write the document back to the scene file (hot reload on the game side is
-- the game's watcher's business; the editor is a plain writer).
function M.save(S)
    local nodes = {}
    for i, node in S.doc do
        nodes[i] = node
    end
    fs.write(S.scene_path, toml.encode({ nodes = nodes }))
    S.dirty = false
    log.info("saved " .. S.scene_rel)
end

-- Add a child under the selected node, in both document and mirror.
function M.add_child(S)
    local parent = S.doc[S.sel]
    local base = "Node"
    local name = base
    local n = 1
    local function taken(candidate)
        for _, node in S.doc do
            if node.name == candidate then
                return true
            end
        end
        return false
    end
    while taken(name) do
        n += 1
        name = base .. n
    end
    local entry = {
        id = fresh_id(S.doc, name),
        name = name,
        parent = parent.id or "",
        position = { 0, 0, 0 },
    }
    table.insert(S.doc, entry)
    local ref = M.ref(S, S.sel)
    if ref ~= nil then
        S.refs[#S.doc] = ref:add_child(name)
    end
    S.sel = #S.doc
    S.dirty = true
end

-- Attach the game's scripts to the mirrored nodes (absolute paths, so the
-- editor's script host loads them from the game project). This is what play
-- runs: the actual game code against the actual scene.
function M.attach_game_scripts(S)
    for i, node in S.doc do
        local ref = M.ref(S, i)
        if node.script ~= nil and ref ~= nil then
            ref:attach_script(S.game_root .. "/" .. node.script)
        end
    end
end

-- Open another scene file from the game project.
function M.open_scene(S, rel)
    S.scene_rel = rel
    S.scene_path = S.game_root .. "/" .. rel
    local doc = toml.parse(fs.read(S.scene_path))
    S.doc = doc.nodes or {}
    for _, node in S.doc do
        if node.parent == nil then
            node.parent = ""
        end
    end
    S.sel = 1
    S.dirty = false
    M.build_mirror(S)
    log.info("opened " .. rel)
end

-- The document mirrors the live node for saving, with two exceptions:
-- `readonly` properties are stripped, and an `asset` property keeps the
-- document's authored spelling — the engine's resolved one cannot be saved.
local function doc_value(name, data, previous)
    local schema = scene.component_schema(name) or {}
    local out = {}
    for key, value in data do
        local spec = schema[key]
        local authored = if previous ~= nil then previous[key] else nil
        if spec ~= nil and spec.type == "asset" and authored ~= nil then
            out[key] = authored
        elseif spec == nil or not spec.readonly then
            out[key] = value
        end
    end
    return out
end

function M.sync_component_doc(S, name)
    local node = S.doc[S.sel]
    local ref = M.ref(S, S.sel)
    if ref == nil then
        return
    end
    local data = ref:get_component(name)
    node[name] = if data ~= nil then doc_value(name, data, node[name]) else nil
    S.dirty = true
end

function M.add_component(S, name)
    local ref = M.ref(S, S.sel)
    if ref == nil then
        return
    end
    ref:set_component(name)
    M.sync_component_doc(S, name)
    log.info("added " .. name .. " to " .. S.doc[S.sel].name)
end

function M.remove_component(S, name)
    local ref = M.ref(S, S.sel)
    if ref == nil then
        return
    end
    ref:remove_component(name)
    S.doc[S.sel][name] = nil
    S.dirty = true
end

function M.set_component_prop(S, name, prop, value)
    local ref = M.ref(S, S.sel)
    if ref == nil then
        return
    end
    local node = S.doc[S.sel]
    local data = ref:get_component(name) or {}
    local params = doc_value(name, data, node[name])
    params[prop] = value
    ref:set_component(name, params)
    M.sync_component_doc(S, name)
    -- `doc_value` keeps the document's spelling of an asset property, which is
    -- exactly wrong for the one property that was just retyped, so it is
    -- stamped back over afterwards.
    local spec = (scene.component_schema(name) or {})[prop]
    if spec ~= nil and spec.type == "asset" and node[name] ~= nil then
        node[name][prop] = value
    end
end

-- Duplicate the selected node (without children), then rebuild the mirror.
function M.duplicate_selected(S)
    local src = S.doc[S.sel]
    local copy = {}
    for key, value in src do
        if type(value) == "table" then
            local t = {}
            for k2, v2 in value do
                t[k2] = v2
            end
            copy[key] = t
        else
            copy[key] = value
        end
    end
    local base = src.name
    local n = 2
    local function taken(candidate)
        for _, node in S.doc do
            if node.name == candidate then
                return true
            end
        end
        return false
    end
    while taken(base .. n) do
        n += 1
    end
    copy.name = base .. n
    table.insert(S.doc, S.sel + 1, copy)
    S.sel += 1
    S.dirty = true
    M.build_mirror(S)
end

function M.delete_selected(S)
    if #S.doc <= 1 then
        log.warn("cannot delete the last node")
        return
    end
    local victim = S.doc[S.sel]
    local ref = M.ref(S, S.sel)
    if ref ~= nil then
        ref:queue_free()
    end
    local kept = {}
    for _, node in S.doc do
        if node ~= victim and not has_ancestor(S.doc, node, victim.id) then
            table.insert(kept, node)
        end
    end
    S.doc = kept
    S.sel = math.clamp(S.sel, 1, #S.doc)
    S.dirty = true
    M.build_mirror(S)
    log.info("deleted " .. victim.name)
end

-- Count stats for the status bar.
function M.stats(S)
    local scripts = 0
    for _, node in S.doc do
        if node.script ~= nil then
            scripts += 1
        end
    end
    return #S.doc, scripts
end

return M
