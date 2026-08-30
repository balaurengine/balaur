-- Inspector (308 px): live node properties, editable, per-persona sections.
local theme = require("scripts/theme")
local model = require("scripts/model")
local highlight = require("scripts/highlight")
local animlib = require("scripts/anim")

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
            setter(node, ref, v)
            S.dirty = true
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

-- Real clip properties (scripts/anim.luau owns the data model).
local function animation_section(S, k)
    local node = model.selected(S)
    local anim = animlib.clip(node)
    section(k, "Animation")
    if anim == nil then
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
    select_row(k, "Clip", anim.clip)
    row(k, "Speed", function()
        local v, ch = ui.slider(anim.speed or 1.0, 0.1, 3.0, {
            width = CONTROL_W - 44,
            rail = k.sunken, fill = k.accent_fill, knob = k.bg,
        })
        ui.add_space(6)
        ui.label(string.format("%.2f", v), { size = 11.5, font = "mono", color = k.text })
        if ch then
            anim.speed = v
            S.dirty = true
        end
    end)
    row(k, "Length", function()
        local v, ch = value_field(k, anim.length, { speed = 0.01, suffix = " s" })
        if ch then
            anim.length = math.max(0.1, v)
            S.dirty = true
        end
    end)
    row(k, "Loop", function()
        local on, ch = ui.toggle(anim.loop == true, {
            on_fill = k.accent_fill, off_fill = k.sunken,
            on_knob = k.on_accent, off_knob = k.faint,
        })
        if ch then
            anim.loop = on
            S.dirty = true
        end
    end)
    row(k, "Keys", function()
        ui.label(`{#anim.keys}`, { size = 11.5, font = "mono", color = k.text })
    end)
end

-- Interface persona: the real surface is the `ui` scripting module.
local function interface_section(S, k)
    local node = model.selected(S)
    section(k, "Interface")
    local draws = false
    if node.script ~= nil then
        local src = fs.read(S.game_root .. "/" .. node.script) or ""
        draws = string.find(src, "draw_ui", 1, true) ~= nil
    end
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
    else -- string
        row(k, prop, function()
            local id = `comp:{comp}:{prop}:{S.sel}`
            local text, _, submitted = ui.text_field(id, tostring(value or ""), {
                size = 12, width = CONTROL_W, color = k.text,
            })
            if submitted then
                model.set_component_prop(S, comp, prop, text)
            end
        end)
    end
end

-- One auto-generated section per component present on the node.
-- `filter` limits which components a persona shows (nil = all).
local function component_sections(S, k, filter)
    local _, ref = model.selected(S)
    if ref == nil then
        return
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
            ui.add_space(8)
            local schema = scene.component_schema(comp) or {}
            local data = ref:get_component(comp) or {}
            for _, prop in sorted_keys(schema) do
                -- `readonly` props are engine output (widget.clicked): the
                -- component declares them so they round-trip, but editing
                -- one would be a write the next frame overwrites.
                if not schema[prop].readonly then
                    prop_row(S, k, comp, prop, schema[prop], data)
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
    local src = fs.read(S.game_root .. "/" .. node.script) or ""
    local _, hooks = highlight.analyze(src)
    for _, hook in hooks do
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
                script_section(S, k)
            elseif S.persona == "script" then
                script_section(S, k, "Attached script")
                select_row(k, "Language", "Luau")
                select_row(k, "Hot reload", "on save")
            elseif S.persona == "anim" then
                animation_section(S, k)
                transform_section(S, k)
                script_section(S, k)
            elseif S.persona == "ui" then
                component_sections(S, k, { widget = true })
                interface_section(S, k)
                script_section(S, k)
            else
                transform_section(S, k)
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
