-- Design tokens from the balaur editor handoff ("Organic" palette).
-- Two complete sets; #f9f4ed is the fixed on-accent text in both.
local M = {}

M.dark = {
    dark = true,
    bg = "#17191c", panel = "#20242a", sunken = "#101215", raised = "#2b3037",
    line = "#343a42", line_soft = "#2b3037",
    text = "#eef1f4", dim = "#b0b8c0", faint = "#767e88",
    accent = "#f0a273", accent_fill = "#d5814e", accent_soft = "#3d2415",
    sage = "#8fbcae", sage_soft = "#22352f",
    grid = "#1e2126", grid_major = "#2b3037", code_bg = "#131518",
    on_accent = "#f9f4ed",
    k_key = "#f0a273", k_str = "#8fbcae", k_num = "#ffc7a8", k_com = "#767e88",
    k_fn = "#eef1f4", k_type = "#b6d8cc", k_punc = "#98a1aa",
}

M.light = {
    dark = false,
    bg = "#eef0f1", panel = "#e2e5e7", sunken = "#d6dadc", raised = "#f9fafb",
    line = "#c2c8cb", line_soft = "#d8dcdf",
    text = "#1d2124", dim = "#586065", faint = "#8f979c",
    accent = "#a85a2c", accent_fill = "#c06a34", accent_soft = "#f3e0d2",
    sage = "#5f7f74", sage_soft = "#dde8e4",
    grid = "#e3e6e8", grid_major = "#cfd4d7", code_bg = "#f9fafb",
    on_accent = "#f9f4ed",
    k_key = "#8a4718", k_str = "#3f6459", k_num = "#a85a2c", k_com = "#8f979c",
    k_fn = "#22272b", k_type = "#5f7f74", k_punc = "#767e83",
}

M.current = M.dark

local function hex_to_rgb(hex)
    local r = tonumber(string.sub(hex, 2, 3), 16) / 255
    local g = tonumber(string.sub(hex, 4, 5), 16) / 255
    local b = tonumber(string.sub(hex, 6, 7), 16) / 255
    return r, g, b
end

-- Push the active token set into egui and the 3D viewport.
function M.apply()
    local t = M.current
    ui.set_theme(t)
    render.set_background(hex_to_rgb(t.bg))
    local mr, mg, mb = hex_to_rgb(t.grid)
    local jr, jg, jb = hex_to_rgb(t.grid_major)
    render.set_grid_colors(mr, mg, mb, jr, jg, jb)
end

function M.set_dark(dark)
    M.current = if dark then M.dark else M.light
    M.apply()
end

M.hex_to_rgb = hex_to_rgb

return M
