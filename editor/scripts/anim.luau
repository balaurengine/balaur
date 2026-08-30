-- A minimal, real keyframe system, editor-side: clips live on scene nodes
-- (`node.animation`, saved with the scene), keys snapshot position and
-- rotation, the timeline dock scrubs and inserts keys, and play samples
-- every animated node. Entirely Luau: the engine core stays lean and the
-- editor dogfoods the scripting surface.
local model = require("scripts/model")

local M = {}

function M.clip(node)
    return node.animation
end

function M.ensure_clip(S)
    local node = S.doc[S.sel]
    if node.animation == nil then
        node.animation = {
            clip = string.lower(node.name),
            length = 2.0,
            loop = true,
            speed = 1.0,
            keys = {},
        }
        S.dirty = true
    end
    return node.animation
end

-- Insert (or replace) a key at time `t` from the node's current transform.
function M.add_key(S, t)
    local anim = M.ensure_clip(S)
    local _, ref = model.selected(S)
    if ref == nil then
        return
    end
    local px, py, pz = table.unpack(ref:position())
    local rx, ry, rz = table.unpack(ref:rotation_euler())
    local key = { t = t, pos = { px, py, pz }, rot = { rx, ry, rz } }
    for i, k in anim.keys do
        if math.abs(k.t - t) < 0.021 then
            anim.keys[i] = key
            S.dirty = true
            return
        end
        if k.t > t then
            table.insert(anim.keys, i, key)
            S.dirty = true
            return
        end
    end
    table.insert(anim.keys, key)
    S.dirty = true
    log.info(string.format("key at %.2fs (%d keys)", t, #anim.keys))
end

function M.delete_key_at(S, t)
    local anim = M.clip(S.doc[S.sel])
    if anim == nil then
        return
    end
    for i, k in anim.keys do
        if math.abs(k.t - t) < 0.05 then
            table.remove(anim.keys, i)
            S.dirty = true
            return
        end
    end
end

local function lerp(a, b, t)
    return a + (b - a) * t
end

-- Sampled { pos, rot } at time t, or nil when the clip has no keys.
function M.sample(anim, t)
    local keys = anim.keys
    if #keys == 0 then
        return nil
    end
    if t <= keys[1].t then
        return keys[1]
    end
    if t >= keys[#keys].t then
        return keys[#keys]
    end
    for i = 2, #keys do
        local a, b = keys[i - 1], keys[i]
        if t <= b.t then
            local f = (t - a.t) / math.max(b.t - a.t, 1e-6)
            return {
                pos = {
                    lerp(a.pos[1], b.pos[1], f),
                    lerp(a.pos[2], b.pos[2], f),
                    lerp(a.pos[3], b.pos[3], f),
                },
                rot = {
                    lerp(a.rot[1], b.rot[1], f),
                    lerp(a.rot[2], b.rot[2], f),
                    lerp(a.rot[3], b.rot[3], f),
                },
            }
        end
    end
    return keys[#keys]
end

local function apply(ref, pose)
    if pose == nil or ref == nil then
        return
    end
    ref:set_position(pose.pos[1], pose.pos[2], pose.pos[3])
    ref:set_rotation_euler(pose.rot[1], pose.rot[2], pose.rot[3])
end

-- Apply the sampled pose to one node at the playhead (scrubbing).
function M.apply_playhead(S)
    local node, ref = model.selected(S)
    local anim = M.clip(node)
    if anim ~= nil then
        apply(ref, M.sample(anim, S.anim_time))
    end
end

-- Advance and apply every animated node (runs while the scene plays).
function M.update(S, dt)
    if S.playing then
        for i, node in S.doc do
            local anim = M.clip(node)
            if anim ~= nil and #anim.keys > 0 then
                local t = engine.time() * (anim.speed or 1.0)
                if anim.loop then
                    t %= anim.length
                else
                    t = math.min(t, anim.length)
                end
                apply(model.ref(S, i), M.sample(anim, t))
            end
        end
        if S.persona == "anim" then
            S.anim_time = engine.time() % (M.clip(S.doc[S.sel]) or { length = 2 }).length
        end
    end
end

return M
