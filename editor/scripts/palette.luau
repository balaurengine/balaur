-- Command palette: the single modal overlay. ⌘K opens; Esc, scrim click, or
-- running a command closes. Substring filter, first result pre-highlighted.
local theme = require("scripts/theme")
local model = require("scripts/model")
local animlib = require("scripts/anim")

local M = {}

local function commands(S)
    local list = {
        { label = "Scene persona", icon = "⌖", key = "⌥1", run = function() S.set_persona("scene") end },
        { label = "Script persona", icon = "‹›", key = "⌥2", run = function() S.set_persona("script") end },
        { label = "Animate persona", icon = "◷", key = "⌥3", run = function() S.set_persona("anim") end },
        { label = "Physics persona", icon = "▣", key = "⌥4", run = function() S.set_persona("phys") end },
        { label = "Interface persona", icon = "▤", key = "⌥5", run = function() S.set_persona("ui") end },
        { label = "Add child node…", icon = "＋", key = "⇧A", run = function() model.add_child(S) end },
        { label = "Attach Luau script to selection", icon = "‹›", key = "⌥S", run = function() S.attach_script() end },
        { label = "Run scene", icon = "▶", key = "F5", run = function() S.toggle_play() end },
        { label = "Save scene", icon = "◇", key = "⌘S", run = function() model.save(S) end },
        { label = "Reload scripts", icon = "⟳", key = "⇧R", run = function() log.info("scripts hot reload on save") end },
        { label = "Toggle dark chrome", icon = "☾", key = "⌥L", run = function() theme.set_dark(not theme.current.dark) end },
        { label = "Open animation timeline", icon = "◷", key = "⌥3", run = function() S.dock = "timeline" end },
        { label = "Duplicate selected node", icon = "⧉", key = "", run = function() model.duplicate_selected(S) end },
        { label = "Delete selected node", icon = "✕", key = "", run = function() model.delete_selected(S) end },
        { label = "Create animation clip", icon = "◷", key = "", run = function() animlib.ensure_clip(S) end },
        { label = "Insert keyframe at playhead", icon = "◆", key = "K", run = function() animlib.add_key(S, S.anim_time) end },
        { label = "UI scale +", icon = "＋", key = "⌘=", run = function() S.set_ui_scale(S.ui_scale + 0.125) end },
        { label = "UI scale −", icon = "−", key = "⌘-", run = function() S.set_ui_scale(S.ui_scale - 0.125) end },
    }
    -- Component commands come from the engine's registry, so third-party
    -- plugin components are addable without touching the editor.
    local _, ref = model.selected(S)
    if ref ~= nil then
        local present = {}
        for _, name in ref:component_names() do
            present[name] = true
        end
        for _, name in scene.component_types() do
            if present[name] then
                table.insert(list, {
                    label = `Remove {name} component`, icon = "✕", key = "",
                    run = function() model.remove_component(S, name) end,
                })
            else
                table.insert(list, {
                    label = `Add {name} component`, icon = "＋", key = "",
                    run = function() model.add_component(S, name) end,
                })
            end
        end
    end
    return list
end

function M.draw(S)
    if not S.palette then
        return
    end
    local k = theme.current
    local scrim_clicked = ui.modal("palette", {
        width = 560, top = 0.11, fill = k.panel, stroke = k.line,
        scrim = "#17191c6b",
    }, function()
        -- Search row.
        ui.frame({ padding_x = 16, padding_y = 12 }, function()
            ui.horizontal({ height = 24 }, function()
                ui.label("⌕", { size = 16, color = k.accent })
                ui.add_space(4)
                local text, _, submitted = ui.text_field("palette-query", "Nodes, commands, personas…", {
                    size = 15, width = 440, autofocus = true, color = k.text,
                })
                S.palette_query = text
                ui.right(function()
                    ui.pill("esc", { height = 20, size = 11, strong = true, fill = k.sunken, color = k.faint })
                end)
                if submitted then
                    S.palette_submit = true
                end
            end)
        end)
        ui.separator(k.line_soft)
        -- Results.
        ui.scroll("palette-results", { max_height = 326 }, function()
            ui.spacing(4, 4)
            ui.add_space(8)
            local query = string.lower(S.palette_query or "")
            local first = true
            local matched = 0
            for _, cmd in commands(S) do
                if query == "" or string.find(string.lower(cmd.label), query, 1, true) ~= nil then
                    matched += 1
                    local run = false
                    ui.horizontal({ height = 40 }, function()
                        ui.add_space(8)
                        if ui.pill(cmd.label, {
                            height = 40, size = 13.5, align = "left",
                            min_width = 430,
                            icon = cmd.icon, icon_size = 14, icon_color = k.accent,
                            fill = if first then k.sunken else "#00000000",
                            hover_fill = k.sunken,
                            color = k.text,
                            tooltip = cmd.key,
                        }) then
                            run = true
                        end
                        ui.right(function()
                            ui.add_space(10)
                            ui.label(cmd.key, { size = 11, font = "mono", color = k.faint })
                        end)
                    end)
                    if run or (S.palette_submit and first) then
                        cmd.run()
                        S.close_palette()
                    end
                    first = false
                end
            end
            if matched == 0 then
                ui.horizontal({ height = 40 }, function()
                    ui.add_space(16)
                    ui.label("no matches", { size = 12, font = "mono", color = k.faint })
                end)
            end
            ui.add_space(8)
        end)
    end)
    S.palette_submit = false
    if scrim_clicked or ui.shortcut("none", "Escape") then
        S.close_palette()
    end
end

return M
