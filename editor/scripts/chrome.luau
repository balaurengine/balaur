-- Persona bar (56 px) and status bar (28 px).
local theme = require("scripts/theme")
local defs = require("scripts/defs")
local model = require("scripts/model")

local M = {}

local function plural(n, word)
    return if n == 1 then word else word .. "s"
end

function M.persona_bar(S)
    local k = theme.current
    ui.top_panel("persona", { height = 56, fill = k.panel, padding_x = 12 }, function()
        ui.horizontal({ height = 56 }, function()
            -- Brand: the balaur mark + wordmark.
            ui.image("assets/balaur-logo.png", { height = 24, radius = 12, bg = "#ffffff", padding = 3 })
            ui.add_space(5)
            ui.label("balaur", { size = 17, font = "heading", strong = true, color = k.text })
            ui.add_space(12)

            -- Persona group: sunken pill container.
            ui.frame({ fill = k.sunken, stroke = k.line_soft, radius = 19, padding_x = 3, padding_y = 3 }, function()
                ui.horizontal({ height = 32, tight = true }, function()
                    ui.spacing(2, 0)
                    for _, p in defs.personas do
                        local active = S.persona == p.id
                        local label = if S.compact then "" else p.name
                        if ui.pill(label, {
                            height = 32, size = 13, font = "heading",
                            icon = p.icon, tooltip = p.name,
                            fill = if active then k.accent_fill else "#00000000",
                            color = if active then k.on_accent else k.dim,
                        }) then
                            S.set_persona(p.id)
                        end
                    end
                end)
            end)

            ui.right(function()
                -- Theme button.
                if ui.circle_button(if k.dark then "☾" else "☀", {
                    d = 32, size = 15, fill = k.sunken, stroke = k.line_soft,
                    color = k.dim, tooltip = "Toggle theme",
                }) then
                    theme.set_dark(not k.dark)
                end
                ui.add_space(8)
                -- Command pill.
                if ui.pill(if S.compact then "" else "Run a command", {
                    height = 32, size = 12, icon = "⌕", fill = k.sunken,
                    stroke = k.line_soft, color = k.faint, tooltip = "⌘K",
                }) then
                    S.open_palette()
                end
                ui.add_space(12)
                -- Transport: stop, pause, play (declared right to left).
                ui.frame({ fill = k.sunken, stroke = k.line_soft, radius = 19, padding_x = 3, padding_y = 3 }, function()
                    ui.horizontal({ height = 32, width = 106 }, function()
                        ui.spacing(2, 0)
                        if ui.circle_button("▶", {
                            d = 32, size = 14,
                            fill = if S.playing then k.accent_fill else "#00000000",
                            color = if S.playing then k.on_accent else k.dim,
                            tooltip = "Run scene",
                        }) then
                            S.toggle_play()
                        end
                        local paused = S.playing and physics.is_paused()
                        if ui.circle_button("⏸", {
                            d = 32, size = 14,
                            fill = if paused then k.accent_fill else "#00000000",
                            color = if paused then k.on_accent else k.dim,
                            tooltip = "Pause simulation",
                        }) then
                            physics.set_paused(not physics.is_paused())
                        end
                        if ui.circle_button("■", { d = 32, size = 12, color = k.dim, tooltip = "Stop" }) then
                            S.stop_play()
                        end
                    end)
                end)
            end)
        end)
    end)
end

function M.status_bar(S)
    local k = theme.current
    ui.bottom_panel("status", { height = 28, fill = k.panel, padding_x = 12 }, function()
        ui.horizontal({ height = 28 }, function()
            ui.spacing(16, 0)
            ui.dot(if S.paused ~= nil then k.accent elseif S.playing then k.sage else k.faint, 6)
            -- Measured, not asserted: the bar used to claim 60 fps whatever
            -- the frame actually cost.
            local mode = if S.paused ~= nil then `paused · {S.paused_rel}:{S.paused.line}`
                elseif S.playing then `playing · {math.floor(S.fps or 0)} fps`
                else "editing"
            ui.label(mode, { size = 11, font = "mono", color = k.dim })
            local nodes, scripts = model.stats(S)
            ui.label(`{nodes} {plural(nodes, "node")} · {scripts} {plural(scripts, "script")}`,
                { size = 11, font = "mono", color = k.dim })
            ui.dot(k.sage, 6)
            ui.label("Luau VM warm", { size = 11, font = "mono", color = k.dim })
            ui.label("kiss3d · wgpu", { size = 11, font = "mono", color = k.dim })
            ui.right(function()
                local pname
                for _, p in defs.personas do
                    if p.id == S.persona then
                        pname = p.name
                    end
                end
                ui.label(`{pname} persona · {defs.tool_names[S.tool] or S.tool}`, { size = 11, font = "mono", color = k.dim })
            end)
        end)
    end)
end

return M
