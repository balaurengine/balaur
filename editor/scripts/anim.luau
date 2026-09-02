-- The Animate persona's adapter between the scene document and the engine's
-- `assets` / `animation` modules: scrubbing seeks the engine, nothing here
-- samples. The clip belongs to the player (`M.player`), keyed by node path.
local model = require("scripts/model")
local history = require("scripts/history")

local M = {}

-- The asset type `balaur_anim` registers, written into the document so a
-- promoted file declares what it is.
local CLIP_TYPE = "animation_clip"
-- The group a library document holds its named clips under. The engine finds
-- an entry one level down whatever the group is called; the editor has to
-- pick one to write.
local GROUP = "clips"
-- How close two key times have to be to count as the same key: half a frame
-- at 24 fps, which is the grain a person scrubbing can actually hit.
local SAME_TIME = 0.021
-- A hand-edited file can name a parent that names it back.
local MAX_DEPTH = 256

-- The editor's engine resolves relative paths against the *editor's* root, so
-- references handed to it are made absolute; the document keeps the relative form.
local function engine_path(S, rel)
    if string.sub(rel, 1, 1) == "/" then
        return rel
    end
    return S.game_root .. "/" .. rel
end

-- The `animation` component as authored in the document, or nil.
function M.component(node)
    if node == nil or type(node.animation) ~= "table" then
        return nil
    end
    return node.animation
end

-- Doc index of the node whose clip the Timeline edits: the selection when
-- it carries `animation`, else its nearest ancestor that does, else the
-- selection itself (the node a new clip would be created on).
function M.player_index(S)
    local i = S.sel
    local node = S.doc[i]
    for _ = 1, MAX_DEPTH do
        if node == nil then
            break
        end
        if M.component(node) ~= nil then
            return i
        end
        if node.parent == nil or node.parent == "" then
            break
        end
        local parent_i
        for j, candidate in S.doc do
            if candidate.id == node.parent then
                parent_i = j
                break
            end
        end
        if parent_i == nil then
            break
        end
        i, node = parent_i, S.doc[parent_i]
    end
    return S.sel
end

-- The player's document node, live node and doc index.
function M.player(S)
    local i = M.player_index(S)
    return S.doc[i], model.ref(S, i), i
end

-- The selection's node path relative to the player: what a track's `target`
-- says. Empty when the selection is the player.
function M.target_path(S, player_i)
    if player_i == S.sel then
        return ""
    end
    local names = {}
    local node = S.doc[S.sel]
    for _ = 1, MAX_DEPTH do
        if node == nil or node == S.doc[player_i] then
            break
        end
        table.insert(names, 1, node.name)
        node = model.by_id(S.doc, node.parent)
    end
    return table.concat(names, "/")
end

-- The clip the Timeline edits: the one the scene starts. An empty name means
-- the library reference is itself the clip, which is what a single-clip file
-- and an anonymous inline table each are.
function M.clip_name(node)
    local comp = M.component(node)
    if comp == nil or type(comp.autoplay) ~= "string" then
        return ""
    end
    return comp.autoplay
end

-- Where the library lives, for display: a path, or nil when it is inline.
function M.library_path(node)
    local comp = M.component(node)
    if comp ~= nil and type(comp.library) == "string" and comp.library ~= "" then
        return comp.library
    end
    return nil
end

-- The named entry inside a library document, by the engine's own rule: a
-- top-level table of that name, or one a single level down.
local function entry_of(doc, name)
    if name == "" then
        return doc
    end
    if type(doc[name]) == "table" then
        return doc[name]
    end
    for _, group in doc do
        if type(group) == "table" and type(group[name]) == "table" then
            return group[name]
        end
    end
    return nil
end

-- The library document for the player, cached on `S.clip`. Inline it *is*
-- the scene document's own table; external it is written back by `M.save`.
function M.document(S)
    local node, _, player_i = M.player(S)
    local comp = M.component(node)
    if comp == nil then
        return nil
    end
    local cached = S.clip
    if cached ~= nil and cached.index == player_i and cached.source == comp.library then
        return cached.doc
    end
    local doc, path
    if type(comp.library) == "table" then
        doc = comp.library
    elseif type(comp.library) == "string" and comp.library ~= "" then
        path = comp.library
        local ok, loaded = pcall(assets.load, engine_path(S, path))
        if not ok or type(loaded) ~= "table" then
            log.warn(`clip {path}: {tostring(loaded)}`)
            -- Cached as a failure so the warning is printed once rather than
            -- once a frame.
            S.clip = { index = player_i, source = comp.library, path = path, dirty = false }
            return nil
        end
        doc = loaded
    else
        S.clip = { index = player_i, source = comp.library, dirty = false }
        return nil
    end
    S.clip = { index = player_i, source = comp.library, doc = doc, path = path, dirty = false }
    return doc
end

-- The clip definition under edit: length, loop, tracks.
function M.definition(S)
    local doc = M.document(S)
    if doc == nil then
        return nil
    end
    return entry_of(doc, M.clip_name(S.doc[M.player_index(S)]))
end

function M.tracks(def)
    if def == nil or type(def.tracks) ~= "table" then
        return {}
    end
    return def.tracks
end

-- A lane's name: the property, prefixed by the node it drives when that is
-- not the player itself.
function M.track_label(track)
    local property = track.property or "call"
    local target = track.target or ""
    if target == "" then
        return property
    end
    return `{target} · {property}`
end

function M.length(def)
    if def == nil then
        return 2.0
    end
    return math.max(tonumber(def.length) or 2.0, 0.001)
end

function M.key_count(def)
    local n = 0
    for _, track in M.tracks(def) do
        n += #(track.keys or {})
    end
    return n
end

-- Put the edited clip in front of the engine and hold it at the playhead:
-- `animation.define` hands over the editor's in-memory table, edits included.
function M.arm(S)
    local node, ref, player_i = M.player(S)
    local comp = M.component(node)
    if comp == nil then
        S.clip = { index = player_i, dirty = false }
        return
    end
    if ref == nil then
        -- Nothing live to push the clip to. The document still reads, so the
        -- Timeline shows it; filling the cache is what stops the re-arm loop.
        M.document(S)
        return
    end
    local library = comp.library
    if type(library) == "string" and library ~= "" then
        library = engine_path(S, library)
    end
    local ok, why = pcall(function()
        ref:set_component("animation", {
            library = library,
            autoplay = comp.autoplay or "",
            speed = comp.speed or 1.0,
            root = comp.root or "",
        })
    end)
    if not ok then
        log.warn(`animation on {node.name}: {tostring(why)}`)
        return
    end
    local def = M.definition(S)
    if def == nil then
        return
    end
    local name = M.clip_name(node)
    ok, why = pcall(animation.define, ref, name, def)
    if not ok then
        log.warn(`clip '{name}' on {node.name}: {tostring(why)}`)
        return
    end
    -- While the scene is running it is the game's transport, not the editor's:
    -- restarting the clip to show the playhead would be the editor animating
    -- over the thing it is supposed to be previewing.
    if S.playing then
        return
    end
    ok, why = pcall(animation.play, ref, name)
    if not ok then
        log.warn(`clip '{name}' on {node.name}: {tostring(why)}`)
        return
    end
    animation.pause(ref)
    animation.seek(ref, S.anim_time)
end

-- The clip changed: the scene needs saving, an external clip needs writing,
-- and the engine needs the new table.
function M.touch(S)
    if S.clip ~= nil then
        S.clip.dirty = true
    end
    M.arm(S)
end

-- Create an inline clip on the player, named after it. With no player above
-- the selection, the selection becomes one.
function M.ensure_clip(S)
    local node = S.doc[M.player_index(S)]
    if M.component(node) ~= nil then
        return M.definition(S)
    end
    history.record(S, "create clip")
    local name = string.lower(node.name)
    node.animation = {
        library = { type = CLIP_TYPE, [GROUP] = { [name] = { length = 2.0, loop = "loop" } } },
        autoplay = name,
        speed = 1.0,
        root = "",
    }
    S.clip = nil
    M.arm(S)
    log.info(`created clip '{name}' on {node.name}`)
    return M.definition(S)
end

local function track_for(def, property, target)
    for _, track in M.tracks(def) do
        if (track.target or "") == target and track.property == property then
            return track
        end
    end
    local track = { property = property, interp = "linear", keys = {} }
    if target ~= "" then
        track.target = target
    end
    if type(def.tracks) ~= "table" then
        def.tracks = {}
    end
    table.insert(def.tracks, track)
    return track
end

local function put_key(track, t, value)
    local key = { t = t, value = value }
    for i, k in track.keys do
        if math.abs(k.t - t) < SAME_TIME then
            track.keys[i] = key
            return
        end
        if k.t > t then
            table.insert(track.keys, i, key)
            return
        end
    end
    table.insert(track.keys, key)
end

-- Insert (or replace) a key at `t` from the selection's current transform,
-- on the player's clip, addressed by path. A bone keys its rotation alone:
-- its position is its rest, and a key there would tear the rig.
function M.add_key(S, t)
    local def = M.ensure_clip(S)
    local node, ref = model.selected(S)
    if ref == nil then
        -- A key is a snapshot of the live node, so there has to be one.
        log.warn(`{node.name}: no live node to read a transform from`)
        return
    end
    local player_i = M.player_index(S)
    if def == nil then
        log.warn(`{node.name}: the library has no clip called '{M.clip_name(S.doc[player_i])}'`)
        return
    end
    history.record(S, "insert keyframe")
    local target = M.target_path(S, player_i)
    local rx, ry, rz = table.unpack(ref:rotation_euler())
    put_key(track_for(def, "rotation_euler", target), t, { rx, ry, rz })
    if type(node.bone2d) ~= "table" and type(node.bone3d) ~= "table" then
        local px, py, pz = table.unpack(ref:position())
        put_key(track_for(def, "position", target), t, { px, py, pz })
    end
    if t > M.length(def) then
        def.length = t
    end
    M.touch(S)
    log.info(string.format("key at %.2fs (%d keys)", t, M.key_count(def)))
end

-- Delete the selection's keys at the playhead; other nodes' lanes keep theirs.
function M.delete_key_at(S, t)
    local def = M.definition(S)
    if def == nil then
        return
    end
    history.record(S, "delete keyframe")
    local target = M.target_path(S, M.player_index(S))
    local kept = {}
    for _, track in M.tracks(def) do
        local keys = {}
        for _, key in (track.keys or {}) do
            if (track.target or "") ~= target or math.abs(key.t - t) >= 0.05 then
                table.insert(keys, key)
            end
        end
        track.keys = keys
        -- A track with no keys is not a track — the engine rejects one, so
        -- the editor never writes one.
        if #keys > 0 then
            table.insert(kept, track)
        end
    end
    def.tracks = if #kept > 0 then kept else nil
    M.touch(S)
end

-- Move the playhead. The engine poses the node; the editor samples nothing.
function M.seek(S, t)
    S.anim_time = t
    local node, ref = M.player(S)
    if ref ~= nil and M.component(node) ~= nil then
        pcall(animation.seek, ref, t)
    end
end

-- Inline → external: the same table, in a file of its own.
function M.promote(S)
    local node = M.player(S)
    local comp = M.component(node)
    local doc = M.document(S)
    if comp == nil or doc == nil then
        return
    end
    history.record(S, "save clip as file")
    if type(comp.library) ~= "table" then
        log.warn("this clip is already a file")
        return
    end
    local rel = "animations/" .. string.lower(node.name) .. ".toml"
    doc.type = CLIP_TYPE
    fs.write(engine_path(S, rel), toml.encode(doc))
    pcall(assets.reload, engine_path(S, rel))
    comp.library = rel
    S.clip = nil
    M.arm(S)
    log.info("saved " .. rel .. " (undo detaches it; the file stays)")
end

-- External → inline: a private copy of the document, owned by this node.
function M.make_inline(S)
    local node = M.player(S)
    local comp = M.component(node)
    local rel = M.library_path(node)
    if comp == nil or rel == nil then
        log.warn("this clip is already inline")
        return
    end
    history.record(S, "make clip inline")
    local ok, copy = pcall(assets.duplicate, engine_path(S, rel))
    if not ok or type(copy) ~= "table" then
        log.warn(`cannot inline {rel}: {tostring(copy)}`)
        return
    end
    -- A reference to one entry brings back that entry alone, so it is wrapped
    -- back into a library under the name it was addressed by — which is also
    -- the name `autoplay` now has to carry.
    local entry = string.match(rel, "#(.+)$")
    if entry ~= nil then
        copy = { [GROUP] = { [entry] = copy } }
        comp.autoplay = entry
    end
    copy.type = CLIP_TYPE
    comp.library = copy
    S.clip = nil
    M.arm(S)
    log.info(`clip is now inline on {node.name}`)
end

-- Write an external clip back to its file. An inline clip is saved with the
-- scene, so there is nothing separate to write.
function M.save(S)
    local clip = S.clip
    if clip == nil or clip.path == nil or clip.doc == nil or not clip.dirty then
        return
    end
    fs.write(engine_path(S, clip.path), toml.encode(clip.doc))
    pcall(assets.reload, engine_path(S, clip.path))
    clip.dirty = false
    log.info("saved " .. clip.path)
end

-- After a mirror rebuild, re-apply each animated `library` as an absolute
-- path: game-relative references do not resolve in the editor's engine.
function M.rebind(S)
    for i, node in S.doc do
        local comp = M.component(node)
        local ref = model.ref(S, i)
        local rel = M.library_path(node)
        if ref ~= nil and comp ~= nil and rel ~= nil then
            local ok, why = pcall(function()
                ref:set_component("animation", {
                    library = engine_path(S, rel),
                    autoplay = comp.autoplay or "",
                    speed = comp.speed or 1.0,
                    root = comp.root or "",
                })
            end)
            if not ok then
                log.warn(`animation on {node.name}: {tostring(why)}`)
            end
            if not S.playing then
                animation.pause(ref)
            end
        end
    end
end

-- Let the mirrored players run again (F5). `M.arm` paused them; the mirror is
-- built paused so the edited scene does not animate under the inspector.
function M.resume_all(S)
    for i, node in S.doc do
        local ref = model.ref(S, i)
        if ref ~= nil and M.component(node) ~= nil then
            animation.resume(ref)
        end
    end
end

-- The engine advances every player itself. All the editor does per frame is
-- follow the playhead while the scene runs, and re-arm when the selection
-- moves to another player.
function M.update(S)
    if S.anim_rebind then
        S.anim_rebind = nil
        M.rebind(S)
    end
    local player_i = M.player_index(S)
    if S.playing then
        local node, ref = M.player(S)
        if ref ~= nil and M.component(node) ~= nil then
            S.anim_time = animation.time(ref)
        end
        return
    end
    if S.clip == nil or S.clip.index ~= player_i then
        -- The cache is about to be replaced, so an external clip edited but
        -- not yet written goes to its file now rather than being dropped.
        M.save(S)
        S.clip = nil
        M.arm(S)
    end
end

return M
