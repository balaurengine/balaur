-- Inspector (308 px): live node properties, editable, per-persona sections.
local theme = require("scripts/theme")
local model = require("scripts/model")
local animlib = require("scripts/anim")
local riglib = require("scripts/rig")
local polygonlib = require("scripts/polygon")
local history = require("scripts/history")

local M = {}

local LABEL_W = 84
-- Control area width: 308 panel − 24 padding − 84 label − spacing.
local CONTROL_W = 186

local function section(k, title)
    ui.add_space(12)
    ui.horizontal(nil, function()
        ui.label(string.upper(title), { size = 10, font = "heading", strong = true, color = k.dim })
        ui.add_space(8)
        ui.separator(k.line_soft)
    end)
    ui.add_space(8)
end

local function row(k, label, body)
    ui.horizontal({ height = 28 }, function()
        ui.spacing(8, 0)
        -- Fixed label column so control widths stay inside the panel
        -- regardless of the label's length.
        ui.horizontal({ height = 28, width = LABEL_W }, function()
            ui.label(label, { size = 12, color = k.dim })
        end)
        body()
    end)
    ui.add_space(4)
end

-- A draggable vector field writing straight back to doc + live node.
local function vec3_row(S, k, label, getter, setter)
    local node, ref = model.selected(S)
    local v = getter(node, ref)
    row(k, label, function()
        local axes = { "x", "y", "z" }
        local w = CONTROL_W / 3 - 5
        local changed = false
        for i = 1, 3 do
            local nv, ch = ui.drag_value(v[i], {
                width = w, speed = 0.02, decimals = 2,
                prefix = axes[i], prefix_color = k.accent,
                fill = k.sunken, stroke = k.line_soft, color = k.text,
            })
            if ch then
                v[i] = nv
                changed = true
            end
        end
        if changed then
            history.record(S, `set {string.lower(label)}`, `xform:{label}:{S.sel}`)
            setter(node, ref, v)
        end
    end)
end

local function value_field(k, value, opts)
    opts = opts or {}
    return ui.drag_value(value, {
        width = opts.width or CONTROL_W,
        speed = opts.speed or 0.02,
        decimals = opts.decimals or 2,
        suffix = opts.suffix,
        fill = k.sunken, stroke = k.line_soft, color = k.text,
    })
end

-- A select shell; when `on_click` is given, clicking cycles the value.
local function select_row(k, label, value, on_click)
    local clicked = false
    row(k, label, function()
        if ui.pill(value, {
            height = 28, size = 12, align = "left",
            min_width = CONTROL_W,
            fill = k.sunken, stroke = k.line_soft, color = k.text,
            trailing = "▼", trailing_color = k.faint, trailing_size = 10,
        }) then
            clicked = true
        end
    end)
    if clicked and on_click ~= nil then
        on_click()
    end
end

local function transform_section(S, k)
    section(k, "Transform")
    vec3_row(S, k, "Position", function(node, ref)
        if ref then
            local x, y, z = table.unpack(ref:position())
            return { x, y, z }
        end
        return node.position or { 0, 0, 0 }
    end, function(node, ref, v)
        node.position = { v[1], v[2], v[3] }
        if ref then
            ref:set_position(v[1], v[2], v[3])
        end
    end)
    -- The row is in degrees and the engine takes them directly. The scene doc
    -- still stores radians, which is the scene-file unit, so the doc write is
    -- the only conversion left.
    vec3_row(S, k, "Rotation", function(node, ref)
        if ref then
            local x, y, z = table.unpack(ref:rotation_degrees())
            return { x, y, z }
        end
        local r = node.rotation_euler or { 0, 0, 0 }
        return { math.deg(r[1]), math.deg(r[2]), math.deg(r[3]) }
    end, function(node, ref, v)
        node.rotation_euler = { math.rad(v[1]), math.rad(v[2]), math.rad(v[3]) }
        if ref then
            ref:set_rotation_degrees(v[1], v[2], v[3])
        end
    end)
    vec3_row(S, k, "Scale", function(node, ref)
        if ref then
            local x, y, z = table.unpack(ref:scale())
            return { x, y, z }
        end
        return node.scale or { 1, 1, 1 }
    end, function(node, ref, v)
        node.scale = { v[1], v[2], v[3] }
        if ref then
            ref:set_scale(v[1], v[2], v[3])
        end
    end)
end

local function script_section(S, k, title)
    local node = model.selected(S)
    section(k, title or "Script")
    row(k, "Script", function()
        ui.frame({ fill = k.sage_soft, stroke = k.sage, radius = 14, padding_x = 11 }, function()
            ui.horizontal({ height = 28 }, function()
                local name = node.script or "— none —"
                ui.label(name, { size = 11.5, font = "mono", color = k.sage })
                ui.right(function()
                    local action = if node.script then "open" else "add"
                    if ui.pill(action, {
                        height = 20, size = 10, strong = true,
                        fill = k.panel, color = k.dim,
                    }) then
                        if node.script then
                            S.set_persona("script")
                        else
                            S.attach_script()
                        end
                    end
                end)
            end)
        end)
    end)
end

-- Save as file promotes the inline definition to `animations/<node>.toml`,
-- Make inline copies a file back onto the node; neither touches the clip itself.
local function clip_source_row(S, k, node)
    local path = animlib.library_path(node)
    row(k, "Source", function()
        ui.label(path or "— inline —", { size = 11.5, font = "mono", color = k.faint })
        ui.right(function()
            local label = if path == nil then "save as file" else "make inline"
            if ui.pill(label, {
                height = 20, size = 10, strong = true,
                tooltip = if path == nil
                    then "Write this clip to animations/ and reference it"
                    else "Copy this file's clip onto the node",
                fill = k.sage_soft, color = k.sage,
            }) then
                if path == nil then
                    animlib.promote(S)
                else
                    animlib.make_inline(S)
                end
            end
        end)
    end)
end

-- Godot's two rest-pose verbs, for a node with bones under it (or a bone).
local function skeleton_section(S, k)
    local node, ref = model.selected(S)
    if ref == nil then
        return
    end
    local count = riglib.bone_count(S, S.sel)
    if count == 0 and S.mode2d then
        return
    end
    section(k, "Skeleton")
    row(k, "Bones", function()
        local what = if riglib.is_bone(node) then "this bone and below" else `under {node.name}`
        ui.label(`{count} · {what}`, { size = 11.5, font = "mono", color = k.text })
        ui.right(function()
            if ui.pill("＋ bone", {
                height = 20, size = 10, strong = true,
                tooltip = "Grow a bone under this node (in 2D, the Rig bone tool places one by clicking)",
                fill = k.accent_soft, color = k.accent,
            }) then
                local _, ref = model.selected(S)
                if ref ~= nil then
                    local x, y = table.unpack(ref:global_position())
                    riglib.add_bone(S, S.sel, x, y + 1)
                end
            end
        end)
    end)
    if count == 0 then
        return
    end
    row(k, "Rest pose", function()
        if ui.pill("reset", {
            height = 24, size = 11, strong = true,
            tooltip = "Reset to Rest Pose: pose every bone here at its rest",
            fill = k.sunken, color = k.dim,
        }) then
            riglib.apply_rest(S)
        end
        ui.add_space(6)
        if ui.pill("overwrite", {
            height = 24, size = 11, strong = true,
            tooltip = "Overwrite Rest Pose: the pose every bone is in now becomes its rest",
            fill = k.sage_soft, color = k.sage,
        }) then
            riglib.overwrite_rest(S)
        end
    end)
end

-- What the Polygon tool works on: the vertex and polygon counts, the rig's
-- bones to paint with, and the brush.
local function polygon_section(S, k)
    local node = model.selected(S)
    if polygonlib.component(node) == nil then
        return
    end
    section(k, "Polygon")
    local mesh = polygonlib.mesh(S)
    if mesh == nil then
        row(k, "Mesh", function()
            local text = if polygonlib.editable(S) then "empty — trace it with the Polygon tool"
                else "a file; make it inline to edit"
            ui.label(text, { size = 11, font = "mono", color = k.faint })
        end)
        return
    end
    local positions = mesh.positions or {}
    local internal = math.floor(mesh.internal or 0)
    row(k, "Vertices", function()
        ui.label(`{#positions} · {internal} internal`, { size = 11.5, font = "mono", color = k.text })
    end)
    row(k, "Polygons", function()
        local count = if type(mesh.polygons) == "table" then #mesh.polygons else 0
        ui.label(if count == 0 then "outline, automatic" else `{count} drawn`,
            { size = 11.5, font = "mono", color = k.text })
        if count > 0 then
            ui.right(function()
                if ui.pill("clear", {
                    height = 20, size = 10, strong = true,
                    tooltip = "Back to filling the outline automatically",
                    fill = k.sunken, color = k.dim,
                }) then
                    polygonlib.clear_polygons(S)
                end
            end)
        end
    end)
    row(k, "Bones", function()
        if ui.pill("sync to rig", {
            height = 24, size = 11, strong = true,
            tooltip = "Sync Bones to Polygon: list the rig's bones, keeping weights already painted",
            fill = k.sage_soft, color = k.sage,
        }) then
            polygonlib.sync_bones(S)
        end
    end)
    for i, bone in polygonlib.bones(S) do
        local active = S.paint_bone == i
        ui.horizontal({ height = 24 }, function()
            ui.spacing(8, 0)
            ui.add_space(LABEL_W)
            if ui.pill(bone.path, {
                height = 22, size = 11, align = "left", min_width = CONTROL_W,
                icon = "⋈", icon_size = 11,
                fill = if active then k.accent_soft else k.sunken,
                color = if active then k.accent else k.text,
                tooltip = "Paint this bone's weights in the Weights mode",
            }) then
                S.paint_bone = i
            end
        end)
    end
    row(k, "Brush", function()
        local w = CONTROL_W / 2 - 5
        local radius, ch = ui.drag_value(S.paint_radius or 40, {
            width = w, speed = 0.5, decimals = 0, prefix = "px", prefix_color = k.accent,
            fill = k.sunken, stroke = k.line_soft, color = k.text,
        })
        if ch then
            S.paint_radius = math.clamp(radius, 2, 400)
        end
        local strength, ch2 = ui.drag_value(S.paint_strength or 0.5, {
            width = w, speed = 0.01, decimals = 2, prefix = "×", prefix_color = k.accent,
            fill = k.sunken, stroke = k.line_soft, color = k.text,
        })
        if ch2 then
            S.paint_strength = math.clamp(strength, 0.01, 2)
        end
    end)
end

-- The clip belongs to the player: the selection or its nearest animated
-- ancestor, so a bone shows the character's clip, not one of its own.
local function animation_section(S, k)
    local node = animlib.player(S)
    local def = animlib.definition(S)
    section(k, "Animation")
    if def == nil then
        row(k, "Clip", function()
            if ui.pill("＋ Create clip", {
                height = 28, size = 12, strong = true,
                min_width = CONTROL_W,
                fill = k.accent_soft, color = k.accent,
            }) then
                animlib.ensure_clip(S)
            end
        end)
        return
    end
    local comp = animlib.component(node)
    row(k, "Clip", function()
        local id = `clipname:{S.sel}`
        local text, _, submitted = ui.text_field(id, "unnamed", {
            value = animlib.clip_name(node),
            size = 11.5, font = "mono", width = CONTROL_W, height = 28,
            fill = k.sunken, stroke = k.line_soft, color = k.text,
        })
        if submitted then
            -- The name is `autoplay`: what the scene starts is what the
            -- Timeline edits, so there is one clip to get wrong instead of two.
            history.record(S, "rename clip")
            comp.autoplay = text
            S.clip = nil
            animlib.arm(S)
        end
    end)
    clip_source_row(S, k, node)
    row(k, "Speed", function()
        local v, ch = ui.slider(comp.speed or 1.0, 0.1, 3.0, {
            width = CONTROL_W - 44,
            rail = k.sunken, fill = k.accent_fill, knob = k.bg,
        })
        ui.add_space(6)
        ui.label(string.format("%.2f", v), { size = 11.5, font = "mono", color = k.text })
        if ch then
            comp.speed = v
            animlib.touch(S)
        end
    end)
    row(k, "Length", function()
        local v, ch = value_field(k, animlib.length(def), { speed = 0.01, suffix = " s" })
        if ch then
            def.length = math.max(0.1, v)
            animlib.touch(S)
        end
    end)
    row(k, "Loop", function()
        -- The engine's own vocabulary, not a boolean: pingpong is a third
        -- answer and a toggle cannot say it.
        local v, ch = ui.select(`loop:{S.sel}`, def.loop or "none", { "none", "loop", "pingpong" }, {
            width = CONTROL_W, size = 12, height = 28, color = k.text,
        })
        if ch then
            def.loop = v
            animlib.touch(S)
        end
    end)
    row(k, "Keys", function()
        ui.label(`{animlib.key_count(def)} on {#animlib.tracks(def)} tracks`,
            { size = 11.5, font = "mono", color = k.text })
    end)
end

-- Interface persona: the real surface is the `ui` scripting module.
local function interface_section(S, k)
    local node = model.selected(S)
    section(k, "Interface")
    local draws = model.draws_ui(S, node)
    row(k, "Draws UI", function()
        ui.label(if draws then "yes · draw_ui()" else "no", { size = 11.5, font = "mono", color = if draws then k.sage else k.faint })
    end)
    row(k, "Safe area", function()
        local chips = S.chip_state.ui or {}
        S.chip_state.ui = chips
        local on, ch = ui.toggle(chips.safe == true, {
            on_fill = k.accent_fill, off_fill = k.sunken,
            on_knob = k.on_accent, off_knob = k.faint,
        })
        if ch then
            chips.safe = on
        end
    end)
end

-- Schema-driven component sections: rows are generated from each
-- registered component's property specs, so third-party plugin components
-- are editable with zero editor changes.
local function sorted_keys(t)
    local keys = {}
    for key in t do
        table.insert(keys, key)
    end
    table.sort(keys)
    return keys
end

-- Write an inline definition to its type's directory and point the property at
-- the project-relative path — an absolute one would not survive another machine.
local function promote_asset(S, comp, prop, asset_type, dir, reference)
    local node = S.doc[S.sel]
    local ok, definition = pcall(assets.load, reference)
    if not ok or type(definition) ~= "table" then
        log.warn(`cannot read the inline {asset_type}: {tostring(definition)}`)
        return
    end
    definition.type = asset_type
    local rel = `{dir}/{string.lower(node.name)}.toml`
    local written, err = pcall(assets.save, S.game_root .. "/" .. rel, definition)
    if not written then
        log.warn(`cannot write {rel}: {tostring(err)}`)
        return
    end
    model.set_component_prop(S, comp, prop, rel)
    log.info(`saved {rel}`)
end

-- Copy a file's definition onto the node, so the scene owns it outright.
-- `duplicate` reads past the cache, which is what makes the copy private
-- rather than a second name for the shared one.
local function inline_asset(S, comp, prop, reference)
    local ok, copy = pcall(assets.duplicate, S.game_root .. "/" .. reference)
    if not ok or type(copy) ~= "table" then
        log.warn(`cannot inline {reference}: {tostring(copy)}`)
        return
    end
    model.set_component_prop(S, comp, prop, copy)
    log.info(`{reference} is now inline on {S.doc[S.sel].name}`)
end

local function prop_row(S, k, comp, prop, spec, data)
    local value = data[prop]
    if value == nil then
        value = spec.default
    end
    local datatype = spec.type
    if datatype == "float" then
        row(k, prop, function()
            local v, ch = value_field(k, value or 0, {
                speed = spec.step or 0.02,
                decimals = spec.decimals or 2,
            })
            if ch then
                if spec.min ~= nil then
                    v = math.max(spec.min, v)
                end
                if spec.max ~= nil then
                    v = math.min(spec.max, v)
                end
                model.set_component_prop(S, comp, prop, v)
            end
        end)
    elseif datatype == "bool" then
        row(k, prop, function()
            local on, ch = ui.toggle(value == true, {
                on_fill = k.accent_fill, off_fill = k.sunken,
                on_knob = k.on_accent, off_knob = k.faint,
            })
            if ch then
                model.set_component_prop(S, comp, prop, on)
            end
        end)
    elseif datatype == "enum" then
        row(k, prop, function()
            local v, changed = ui.select(`sel:{comp}:{prop}:{S.sel}`, tostring(value), spec.options or {}, {
                width = CONTROL_W, size = 12, height = 28, color = k.text,
            })
            if changed then
                model.set_component_prop(S, comp, prop, v)
            end
        end)
    elseif datatype == "vec2" or datatype == "vec3" or datatype == "color" then
        row(k, prop, function()
            local v = value or {}
            local count = if datatype == "color" then 4 elseif datatype == "vec2" then 2 else 3
            local letters = if datatype == "color" then { "r", "g", "b", "a" } else { "x", "y", "z" }
            local w = CONTROL_W / count - 5
            local changed = false
            local out = {}
            for i = 1, count do
                out[i] = v[i] or (if datatype == "color" then 1 else 0)
                local nv, ch = ui.drag_value(out[i], {
                    width = w, speed = if datatype == "color" then 0.005 else 0.02,
                    decimals = 2,
                    prefix = letters[i], prefix_color = k.accent,
                    fill = k.sunken, stroke = k.line_soft, color = k.text,
                })
                if ch then
                    out[i] = if datatype == "color" then math.clamp(nv, 0, 1) else nv
                    changed = true
                end
            end
            if changed then
                model.set_component_prop(S, comp, prop, out)
            end
        end)
    elseif datatype == "asset" then
        -- A reference, or an inline definition the engine reports as a `#!` digest.
        -- A type that declares no directory cannot be promoted, so it gets no button.
        row(k, prop, function()
            local reference = tostring(value or "")
            local inline = string.sub(reference, 1, 2) == "#!"
            local dir = if spec.asset ~= nil then assets.directory(spec.asset) else ""
            if inline then
                ui.label("— inline —", { size = 11.5, font = "mono", color = k.faint })
            else
                local id = `asset:{comp}:{prop}:{S.sel}`
                local width = if dir == "" then CONTROL_W else CONTROL_W - 78
                local text, _, submitted = ui.text_field(id, "— none —", {
                    value = reference,
                    size = 11.5, font = "mono", width = width, height = 28,
                    fill = k.sunken, stroke = k.line_soft, color = k.text,
                })
                if submitted then
                    model.set_component_prop(S, comp, prop, text)
                end
            end
            if dir == "" then
                return
            end
            ui.right(function()
                local label = if inline then "save as file" else "make inline"
                if ui.pill(label, {
                    height = 20, size = 10, strong = true,
                    tooltip = if inline
                        then `Write this {spec.asset} to {dir}/ and reference it`
                        else "Copy this file's definition onto the node",
                    fill = k.sage_soft, color = k.sage,
                }) then
                    if inline then
                        promote_asset(S, comp, prop, spec.asset, dir, reference)
                    else
                        inline_asset(S, comp, prop, reference)
                    end
                end
            end)
        end)
    else -- string
        row(k, prop, function()
            local id = `comp:{comp}:{prop}:{S.sel}`
            local text, _, submitted = ui.text_field(id, "— empty —", {
                value = tostring(value or ""),
                size = 11.5, font = "mono", width = CONTROL_W, height = 28,
                fill = k.sunken, stroke = k.line_soft, color = k.text,
            })
            if submitted then
                model.set_component_prop(S, comp, prop, text)
            end
        end)
    end
end

-- A `readonly` property is engine output (widget.clicked): a write here would
-- be overwritten next frame, so it renders as a plain value with nothing to focus.
local function readonly_text(spec, value)
    local datatype = spec.type
    if datatype == "float" then
        return string.format(`%.{math.floor(spec.decimals or 2)}f`, value or 0)
    elseif datatype == "bool" then
        return if value == true then "true" else "false"
    elseif datatype == "vec2" or datatype == "vec3" or datatype == "color" then
        local count = if datatype == "color" then 4 elseif datatype == "vec2" then 2 else 3
        local v = value or {}
        local parts = {}
        for i = 1, count do
            parts[i] = string.format("%.2f", v[i] or (if datatype == "color" then 1 else 0))
        end
        return table.concat(parts, ", ")
    end
    return tostring(value or "")
end

local function readonly_row(k, prop, spec, data)
    local value = data[prop]
    if value == nil then
        value = spec.default
    end
    row(k, prop, function()
        ui.label(readonly_text(spec, value), { size = 11.5, font = "mono", color = k.faint })
        ui.right(function()
            ui.label("read-only", { size = 10, color = k.faint })
        end)
    end)
end

-- One auto-generated section per component present on the node.
-- `filter` limits which components a persona shows (nil = all).
local function component_sections(S, k, filter)
    local _, ref = model.selected(S)
    if ref == nil then
        return
    end
    -- Advisory only: the component is applied and working as far as the
    -- engine is concerned, but it has nothing to act on yet.
    local unmet = {}
    for _, warning in scene.unmet_expectations(ref) do
        unmet[warning.component] = table.concat(warning.expects, ", ")
    end
    for _, comp in ref:component_names() do
        if filter == nil or filter[comp] then
            -- Section header with a right-aligned remove button.
            ui.add_space(12)
            ui.horizontal({ height = 20 }, function()
                ui.label(string.upper(comp), { size = 10, font = "heading", strong = true, color = k.dim })
                ui.right(function()
                    if ui.pill("✕", {
                        height = 18, size = 10, strong = true, tooltip = "Remove component",
                        fill = k.sunken, color = k.faint,
                    }) then
                        model.remove_component(S, comp)
                    end
                end)
            end)
            ui.separator(k.line_soft)
            if unmet[comp] ~= nil then
                ui.add_space(4)
                ui.label(`⚠ needs one of: {unmet[comp]}`, {
                    size = 11, color = k.warn or k.faint, wrap = true,
                })
            end
            ui.add_space(8)
            local schema = scene.component_schema(comp) or {}
            local data = ref:get_component(comp) or {}
            for _, prop in sorted_keys(schema) do
                local spec = schema[prop]
                if spec.readonly then
                    readonly_row(k, prop, spec, data)
                else
                    prop_row(S, k, comp, prop, spec, data)
                end
            end
        end
    end
end

-- Hooks defined by the attached script, shown as the events surface.
local function events_section(S, k)
    local node = model.selected(S)
    section(k, "Events")
    if node.script == nil then
        ui.horizontal(nil, function()
            ui.add_space(8)
            ui.label("no script attached", { size = 11, font = "mono", color = k.faint })
        end)
        return
    end
    for _, hook in model.hooks(S, node.script) do
        ui.horizontal({ height = 30 }, function()
            ui.add_space(8)
            ui.frame({ fill = k.sunken, radius = 15, padding_x = 11 }, function()
                ui.horizontal({ height = 30 }, function()
                    local color = if hook.name == "init" or hook.name == "update" then k.accent else k.sage
                    ui.dot(color, 6)
                    ui.add_space(4)
                    ui.label(hook.name .. "()", { size = 11.5, font = "mono", color = k.text })
                    ui.right(function()
                        ui.label("→ engine", { size = 11, font = "mono", color = k.faint })
                    end)
                end)
            end)
        end)
    end
end

function M.draw(S)
    local k = theme.current
    local node = model.selected(S)
    ui.right_panel("inspector", { width = 308, fill = k.bg, padding_x = 12 }, function()
        -- Header.
        ui.add_space(12)
        ui.horizontal({ height = 34 }, function()
            local icon = model.node_icon(node)
            ui.circle_button(icon, { d = 30, size = 15, fill = k.accent_soft, color = k.accent })
            ui.add_space(6)
            ui.vertical(function()
                ui.spacing(4, 1)
                ui.label(node.name, { size = 16, font = "heading", strong = true, color = k.text })
                ui.label(model.node_type(node), { size = 11, font = "mono", color = k.faint })
            end)
        end)

        ui.scroll("inspector-scroll", { max_height = ui.available_height() - 50 }, function()
            if S.persona == "phys" then
                component_sections(S, k, { body = true, collider = true, body2d = true, collider2d = true })
                polygon_section(S, k)
                script_section(S, k)
            elseif S.persona == "script" then
                script_section(S, k, "Attached script")
                select_row(k, "Language", "Luau")
                select_row(k, "Hot reload", "on save")
            elseif S.persona == "anim" then
                skeleton_section(S, k)
                polygon_section(S, k)
                animation_section(S, k)
                transform_section(S, k)
                component_sections(S, k, { bone2d = true, bone3d = true, polygon = true })
                script_section(S, k)
            elseif S.persona == "ui" then
                component_sections(S, k, { widget = true })
                interface_section(S, k)
                script_section(S, k)
            else
                transform_section(S, k)
                skeleton_section(S, k)
                polygon_section(S, k)
                component_sections(S, k)
                script_section(S, k)
            end
            events_section(S, k)
            ui.add_space(12)
        end)

        -- Footer: full-width accent button.
        ui.separator(k.line_soft)
        ui.add_space(6)
        if ui.pill("＋  Add component", {
            height = 34, size = 13, font = "heading", strong = true,
            min_width = 282,
            fill = k.accent_fill, color = k.on_accent,
        }) then
            S.open_palette("add ")
        end
        ui.add_space(6)
    end)
end

return M
