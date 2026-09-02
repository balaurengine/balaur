-- Left column (262 px): node tree + per-persona secondary panel.
local theme = require("scripts/theme")
local model = require("scripts/model")
local animlib = require("scripts/anim")

local M = {}

local function section_label(text_, k)
    ui.add_space(11)
    ui.horizontal(nil, function()
        ui.add_space(10)
        ui.label(string.upper(text_), { size = 10, font = "heading", strong = true, color = k.dim })
    end)
    ui.add_space(9)
end

-- One tree row: connector rails, collapse arrow, colored type icon, name,
-- script glyph; right-click opens the node's context menu.
local function tree_row(S, k, i, depth, rails, is_last, has_kids)
    local node = S.doc[i]
    local selected = i == S.sel
    ui.horizontal({ height = 27 }, function()
        ui.add_space(4)
        if depth > 0 then
            local connector = rails .. (if is_last then "└─" else "├─")
            ui.label(connector, { size = 11, font = "mono", color = k.line })
        end
        if has_kids then
            -- Keyed by id, which is what the walk buckets on and what survives
            -- a rename: collapsing a node should not reopen it when it is
            -- renamed.
            local id = node.id or ""
            local open = not S.collapsed[id]
            if ui.pill(if open then "▾" else "▸", {
                height = 18, size = 9, color = k.faint, tooltip = "Collapse",
            }) then
                S.collapsed[id] = if open then true else nil
            end
        else
            ui.add_space(22)
        end
        local icon, role = model.node_icon(node)
        local icon_color = if role == "sage" then k.sage
            elseif role == "accent" then k.accent
            elseif role == "dim" then k.dim
            else k.faint
        -- Row spans the rest of the panel, content left-aligned.
        local used = 12 + (if depth > 0 then depth * 13 + 14 else 0) + 26
        if ui.pill(node.name, {
            height = 27, size = 12, align = "left",
            strong = depth == 0,
            min_width = math.max(120, 246 - used),
            icon = icon, icon_size = 12,
            icon_color = if selected then k.on_accent else icon_color,
            fill = if selected then k.accent_fill else "#00000000",
            hover_fill = k.raised,
            color = if selected then k.on_accent else k.text,
            trailing = if node.script then "‹›" else nil,
            trailing_color = if selected then k.on_accent else k.sage,
            menu = function()
                if ui.menu_item("＋  Add child", { color = k.text }) then
                    S.select(i)
                    model.add_child(S)
                end
                if ui.menu_item("‹›  Attach script", { color = k.text }) then
                    S.select(i)
                    S.attach_script()
                end
                if ui.menu_item("⧉  Duplicate", { color = k.text }) then
                    S.select(i)
                    model.duplicate_selected(S)
                end
                if ui.menu_item("✕  Delete", { color = k.accent }) then
                    S.select(i)
                    model.delete_selected(S)
                end
            end,
        }) then
            S.select(i)
        end
    end)
end

local function tree(S, k, reserve)
    ui.horizontal({ height = 36 }, function()
        ui.add_space(10)
        ui.label("NODE TREE", { size = 10, font = "heading", strong = true, color = k.dim })
        ui.right(function()
            ui.add_space(9)
            if ui.circle_button("＋", { d = 21, size = 12, fill = k.accent_soft, color = k.accent, tooltip = "Add child node" }) then
                model.add_child(S)
            end
        end)
    end)
    -- Children in document order, drawn depth-first with connector rails.
    local kids = {}
    for i, node in S.doc do
        local parent = node.parent or ""
        kids[parent] = kids[parent] or {}
        table.insert(kids[parent], i)
    end
    ui.scroll("tree", { max_height = ui.available_height() - reserve }, function()
        ui.spacing(4, 1)
        -- `kids` is bucketed by the parent's **id**, so the recursion carries
        -- an id too. Descending by name path is what used to make every nested
        -- node invisible: no bucket is ever keyed by a path.
        local function walk(parent_id, depth, rails)
            local children = kids[parent_id] or {}
            for j, i in children do
                local node = S.doc[i]
                local id = node.id or ""
                local is_last = j == #children
                local has_kids = kids[id] ~= nil
                tree_row(S, k, i, depth, rails, is_last, has_kids)
                if has_kids and not S.collapsed[id] then
                    local extend = if depth == 0 then ""
                        elseif is_last then rails .. "  "
                        else rails .. "│ "
                    walk(id, depth + 1, extend)
                end
            end
        end
        walk("", 0, "")
    end)
end

-- row = { icon, name, meta, on_click? }
local function secondary_rows(S, k, title, rows, height)
    ui.separator(k.line_soft)
    section_label(title, k)
    ui.scroll("secondary", { max_height = height }, function()
        ui.spacing(4, 4)
        for _, row in rows do
            ui.horizontal({ height = 29 }, function()
                ui.add_space(8)
                if ui.pill(row[2], {
                    height = 29, size = 12, min_width = 238, align = "left",
                    icon = row[1], icon_size = 12, icon_color = k.dim,
                    fill = k.sunken, color = k.text,
                    trailing = row[3], trailing_color = k.faint,
                }) and row[4] ~= nil then
                    row[4]()
                end
            end)
        end
    end)
end

-- Per-persona secondary contents, from real project data where it exists.
-- Returns the section title and its rows; drawing is the caller's, so the
-- tree above can size itself to what this actually holds.
local function secondary_content(S)
    if S.persona == "script" then
        local rows = {}
        for _, f in model.list(S, "scripts") do
            if not f.is_dir and string.match(f.name, "%.luau$") then
                local rel = "scripts/" .. f.name
                local _, n = string.gsub(model.source(S, rel) or "", "\n", "")
                table.insert(rows, { "‹›", f.name, `{n + 1} L`, function()
                    -- Jump to the node using this script.
                    for i, node in S.doc do
                        if node.script == rel then
                            S.select(i)
                            break
                        end
                    end
                end })
            end
        end
        return "Luau modules", rows
    elseif S.persona == "phys" then
        -- A 2D scene carries `body2d`/`collider2d`; reading `body` alone left
        -- this panel empty for every 2D project.
        local rows = {}
        for i, node in S.doc do
            local kind = model.body_kind(node) or model.body2d_kind(node)
            local collider = node.collider3d ~= nil or node.collider2d ~= nil
            if kind ~= nil or collider then
                local idx = i
                local dim = if node.body2d ~= nil or node.collider2d ~= nil then "2D" else "3D"
                table.insert(rows, { "▣", node.name, `{kind or "collider3d"} · {dim}`, function()
                    S.select(idx)
                end })
            end
        end
        if #rows == 0 then
            rows = { { "▣", "no bodies or colliders", "" } }
        end
        return "Collision", rows
    elseif S.persona == "anim" then
        local rows = {}
        for i, node in S.doc do
            if animlib.component(node) ~= nil then
                local idx = i
                local name = animlib.clip_name(node)
                table.insert(rows, {
                    "◷", if name == "" then node.name else name,
                    animlib.library_path(node) or "inline",
                    function()
                        S.select(idx)
                    end,
                })
            end
        end
        if #rows == 0 then
            rows = { { "◷", "no clips — ＋ Key in Timeline", "" } }
        end
        return "Clips", rows
    elseif S.persona == "ui" then
        -- Two ways a node puts something on screen: a `widget` component in the
        -- scene, or a script with a `draw_ui` hook. Listing only the second
        -- said "no UI scripts" over a viewport full of widgets.
        local rows = {}
        for i, node in S.doc do
            local idx = i
            if node.widget ~= nil then
                local kind = if type(node.widget) == "table" then node.widget.kind else node.widget
                table.insert(rows, { "▤", node.name, tostring(kind or "widget"), function()
                    S.select(idx)
                end })
            elseif node.script ~= nil and model.draws_ui(S, node) then
                table.insert(rows, { "‹›", node.name, "draw_ui", function()
                    S.select(idx)
                end })
            end
        end
        if #rows == 0 then
            rows = { { "▤", "no widgets or draw_ui scripts", "" } }
        end
        return "Interface", rows
    else
        local rows = {}
        for _, f in model.list(S, "scenes") do
            if not f.is_dir then
                local rel = "scenes/" .. f.name
                table.insert(rows, { "◇", f.name,
                    if S.scene_rel == rel then "open" else "",
                    function()
                        if S.scene_rel ~= rel then
                            model.open_scene(S, rel)
                        end
                    end })
            end
        end
        return "Scenes", rows
    end
end

function M.draw(S)
    local k = theme.current
    ui.left_panel("tree", { width = 262, fill = k.bg, padding_x = 8 }, function()
        local title, rows = secondary_content(S)
        -- The secondary panel takes what it needs, capped: a flat 200 px left
        -- a band of dead space under a one-row list and clipped a long one.
        local reserve = math.clamp(50 + #rows * 33, 83, 280)
        tree(S, k, reserve)
        secondary_rows(S, k, title, rows, reserve - 50)
    end)
end

return M
