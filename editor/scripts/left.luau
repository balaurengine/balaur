-- Left column (262 px): node tree + per-persona secondary panel.
local theme = require("scripts/theme")
local model = require("scripts/model")

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
            local path = model.node_path(S.doc, node)
            local open = not S.collapsed[path]
            if ui.pill(if open then "▾" else "▸", {
                height = 18, size = 9, color = k.faint, tooltip = "Collapse",
            }) then
                S.collapsed[path] = if open then true else nil
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

local function tree(S, k)
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
    ui.scroll("tree", { max_height = ui.available_height() - 200 }, function()
        ui.spacing(4, 1)
        local function walk(parent_path, depth, rails)
            local children = kids[parent_path] or {}
            for j, i in children do
                local node = S.doc[i]
                local path = model.node_path(S.doc, node)
                local is_last = j == #children
                local has_kids = kids[path] ~= nil
                tree_row(S, k, i, depth, rails, is_last, has_kids)
                if has_kids and not S.collapsed[path] then
                    local extend = if depth == 0 then ""
                        elseif is_last then rails .. "  "
                        else rails .. "│ "
                    walk(path, depth + 1, extend)
                end
            end
        end
        walk("", 0, "")
    end)
end

-- row = { icon, name, meta, on_click? }
local function secondary_rows(S, k, title, rows)
    ui.separator(k.line_soft)
    section_label(title, k)
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
end

-- Per-persona secondary contents, from real project data where it exists.
local function secondary(S, k)
    if S.persona == "script" then
        local rows = {}
        for _, f in fs.list(S.game_root .. "/scripts") do
            if not f.is_dir and string.match(f.name, "%.luau$") then
                local src = fs.read(S.game_root .. "/scripts/" .. f.name) or ""
                local _, n = string.gsub(src, "\n", "")
                local rel = "scripts/" .. f.name
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
        secondary_rows(S, k, "Luau modules", rows)
    elseif S.persona == "phys" then
        local rows = {}
        for i, node in S.doc do
            if node.body ~= nil then
                local idx = i
                table.insert(rows, { "▣", node.name, node.body, function()
                    S.select(idx)
                end })
            end
        end
        secondary_rows(S, k, "Collision", rows)
    elseif S.persona == "anim" then
        local rows = {}
        for i, node in S.doc do
            if node.animation ~= nil then
                local idx = i
                table.insert(rows, {
                    "◷", node.animation.clip,
                    string.format("%.1f s", node.animation.length),
                    function()
                        S.select(idx)
                    end,
                })
            end
        end
        if #rows == 0 then
            rows = { { "◷", "no clips — ＋ Key in Timeline", "" } }
        end
        secondary_rows(S, k, "Clips", rows)
    elseif S.persona == "ui" then
        local rows = {}
        for i, node in S.doc do
            if node.script ~= nil then
                local src = fs.read(S.game_root .. "/" .. node.script) or ""
                if string.find(src, "draw_ui", 1, true) ~= nil then
                    local idx = i
                    table.insert(rows, { "▤", node.name, "draw_ui", function()
                        S.select(idx)
                    end })
                end
            end
        end
        if #rows == 0 then
            rows = { { "▤", "no UI scripts", "" } }
        end
        secondary_rows(S, k, "UI scripts", rows)
    else
        local rows = {}
        for _, f in fs.list(S.game_root .. "/scenes") do
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
        secondary_rows(S, k, "Scenes", rows)
    end
end

function M.draw(S)
    local k = theme.current
    ui.left_panel("tree", { width = 262, fill = k.bg, padding_x = 8 }, function()
        tree(S, k)
        secondary(S, k)
    end)
end

return M
