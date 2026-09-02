-- The edited project: scene document model, live mirror scene, save/reset.
-- `S.doc` is plain data parsed from the scene TOML; the mirror is a real node
-- tree from the same TOML (scripts not attached, physics paused until play).
local highlight = require("scripts/highlight")
local history = require("scripts/history")

local M = {}

-- Project reads, cached: four panels read the disk while drawing, so uncached
-- that is a read per node per frame. `fs` has no mtime, hence interval + drop.
local TTL = 0.5
local sources = {}
local listings = {}

-- The text of a project-relative file, or nil when there is no such file —
-- the caller that wants to say "missing" needs to tell that from "empty".
function M.source(S, rel)
    if rel == nil then
        return nil
    end
    local entry = sources[rel]
    local now = engine.time()
    if entry == nil or now - entry.at >= TTL then
        entry = { text = fs.read(S.game_root .. "/" .. rel), at = now }
        sources[rel] = entry
    end
    return entry.text
end

-- The script's engine-facing hooks, analysed once per cached read.
function M.hooks(S, rel)
    local text = M.source(S, rel)
    if text == nil then
        return {}
    end
    local entry = sources[rel]
    if entry.hooks == nil then
        local _, hooks = highlight.analyze(text)
        entry.hooks = hooks
    end
    return entry.hooks
end

function M.list(S, rel)
    local entry = listings[rel]
    local now = engine.time()
    if entry == nil or now - entry.at >= TTL then
        entry = { files = fs.list(S.game_root .. "/" .. rel), at = now }
        listings[rel] = entry
    end
    return entry.files
end

-- Drop a cached read (nil drops everything): what the editor just wrote must
-- be visible on the next frame, not on the next interval.
function M.forget(rel)
    if rel == nil then
        sources = {}
        listings = {}
    else
        sources[rel] = nil
    end
end

-- Whether the node's script draws its own interface, as opposed to carrying a
-- `widget` component.
function M.draws_ui(S, node)
    if node.script == nil then
        return false
    end
    return string.find(M.source(S, node.script) or "", "draw_ui", 1, true) ~= nil
end

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
    elseif node.bone2d ~= nil then
        return "Bone2D"
    elseif node.bone3d ~= nil then
        return "Bone3D"
    elseif node.polygon ~= nil then
        return "Polygon2D"
    elseif node.shape3d ~= nil then
        return "MeshInstance3D"
    elseif node.shape2d ~= nil then
        return "MeshInstance2D"
    end
    return "Node"
end

function M.body_kind(node)
    if type(node.body3d) == "table" then
        return node.body3d.kind
    end
    return node.body3d
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
        if node.shape2d ~= nil or node.body2d ~= nil or node.collider2d ~= nil
            or node.polygon ~= nil or node.bone2d ~= nil then
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
    elseif node.bone2d ~= nil or node.bone3d ~= nil then
        return "⋈", "dim"
    elseif node.polygon ~= nil then
        return "⬟", "accent"
    elseif node.shape3d ~= nil then
        if node.shape3d.kind == "ball" then
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
    history.reset(S)
    M.build_mirror(S)
end

-- The editor's engine resolves files against the editor's own root, so a
-- game-relative file path (a sprite's or polygon's texture) is made absolute
-- for the mirror; `doc_value` makes it relative again on the way back.
local function absolute_files(S, value)
    if type(value) == "string" then
        if value ~= "" and string.sub(value, 1, 1) ~= "/"
            and fs.exists(S.game_root .. "/" .. value) then
            return S.game_root .. "/" .. value
        end
        return value
    elseif type(value) == "table" then
        local out = {}
        for key, inner in value do
            out[key] = absolute_files(S, inner)
        end
        return out
    end
    return value
end

local function relative_file(S, value)
    local prefix = S.game_root .. "/"
    if type(value) == "string" and string.sub(value, 1, #prefix) == prefix then
        return string.sub(value, #prefix + 1)
    end
    return value
end

-- (Re)build the live mirror under a fresh container node.
function M.build_mirror(S)
    if S.mirror ~= nil and S.mirror:is_valid() then
        S.mirror:queue_free()
    end
    physics.clear()
    S.mirror = scene.spawn("Edited")
    debugger.set_scope(S.mirror)
    -- The document, not the file: a mirror read from disk cannot hold an
    -- unsaved edit, which left a duplicated node with no live node at all.
    scene.instantiate(toml.encode({ nodes = absolute_files(S, S.doc) }), S.mirror, { scripts = false })
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
    M.forget()
    history.mark_saved(S)
    log.info("saved " .. S.scene_rel)
end

-- Add a child under the selected node, in both document and mirror.
-- Add a node, optionally applying a preset to it. A preset is a recipe: the
-- components land on the node and nothing records which one put them there.
function M.add_child(S, preset)
    history.record(S, "add node")
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
        local child = ref:add_child(name)
        S.refs[#S.doc] = child
        if preset ~= nil then
            scene.apply_preset(child, preset)
            -- Mirror what landed back into the document, so the saved scene
            -- carries the components rather than only the live node.
            for _, component in child:component_names() do
                entry[component] = child:get_component(component)
            end
        end
    end
    S.sel = #S.doc
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
        if node.id == nil or node.id == "" then
            node.id = fresh_id(S.doc, node.name)
        end
    end
    S.sel = 1
    history.reset(S)
    M.build_mirror(S)
    log.info("opened " .. rel)
end

-- The document mirrors the live node for saving, with two exceptions:
-- `readonly` properties are stripped, and an `asset` property keeps the
-- document's authored spelling — the engine's resolved one cannot be saved.
local function doc_value(S, name, data, previous)
    local schema = scene.component_schema(name) or {}
    local out = {}
    for key, value in data do
        local spec = schema[key]
        local authored = if previous ~= nil then previous[key] else nil
        if spec ~= nil and spec.type == "asset" and authored ~= nil then
            out[key] = authored
        elseif spec == nil or not spec.readonly then
            out[key] = relative_file(S, value)
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
    node[name] = if data ~= nil then doc_value(S, name, data, node[name]) else nil
end

function M.add_component(S, name)
    local ref = M.ref(S, S.sel)
    if ref == nil then
        return
    end
    history.record(S, `add {name}`)
    ref:set_component(name)
    M.sync_component_doc(S, name)
    log.info("added " .. name .. " to " .. S.doc[S.sel].name)
end

function M.remove_component(S, name)
    local ref = M.ref(S, S.sel)
    if ref == nil then
        return
    end
    history.record(S, `remove {name}`)
    ref:remove_component(name)
    S.doc[S.sel][name] = nil
end

-- Push the document's component onto the live node, for a tool that edited
-- the document in place and recorded history itself.
function M.apply_component(S, name)
    local ref = M.ref(S, S.sel)
    local node = S.doc[S.sel]
    if ref == nil or node[name] == nil then
        return
    end
    local authored = node[name]
    ref:set_component(name, absolute_files(S, authored))
    M.sync_component_doc(S, name)
    -- `doc_value` keeps an asset's authored spelling only when it is a
    -- reference; an inline table is the authored form, so it goes back whole.
    local schema = scene.component_schema(name) or {}
    for key, spec in schema do
        if spec.type == "asset" and type(authored[key]) == "table" then
            node[name][key] = authored[key]
        end
    end
end

function M.set_component_prop(S, name, prop, value)
    local ref = M.ref(S, S.sel)
    if ref == nil then
        return
    end
    history.record(S, `set {name}.{prop}`, `prop:{name}:{prop}:{S.sel}`)
    local node = S.doc[S.sel]
    local data = ref:get_component(name) or {}
    local params = doc_value(S, name, data, node[name])
    params[prop] = value
    ref:set_component(name, absolute_files(S, params))
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
    history.record(S, "duplicate node")
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
    -- The copy carried the original's id, which the engine then had to
    -- regenerate for itself; children address a parent by id, so it is ours.
    copy.id = fresh_id(S.doc, copy.name)
    table.insert(S.doc, S.sel + 1, copy)
    S.sel += 1
    M.build_mirror(S)
end

function M.delete_selected(S)
    if #S.doc <= 1 then
        log.warn("cannot delete the last node")
        return
    end
    history.record(S, `delete {S.doc[S.sel].name}`)
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
