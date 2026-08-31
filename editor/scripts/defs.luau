-- Static editor vocabulary: personas, tools, palette commands.
local M = {}

M.personas = {
    { id = "scene", name = "Scene", icon = "⌖" },
    { id = "script", name = "Script", icon = "‹›" },
    { id = "anim", name = "Animate", icon = "◷" },
    { id = "phys", name = "Physics", icon = "▣" },
    { id = "ui", name = "Interface", icon = "▤" },
}

-- Only tools something reads: a button that sets `S.tool` and is then ignored
-- advertises an editor that does not exist. Re-add each on its `gizmo` handler.
M.tools = {
    scene = { "select", "move", "rotate", "scale", "zoom" },
    script = {},
    anim = { "select", "move", "key", "zoom" },
    phys = { "select", "move", "zoom" },
    ui = { "select", "move", "zoom" },
}

M.tool_names = {
    select = "Select", move = "Translate", rotate = "Rotate", scale = "Scale",
    zoom = "Zoom", key = "Key frame",
}

M.tool_icons = {
    select = "⌖", move = "✥", rotate = "⟳", scale = "⤢", zoom = "⌕", key = "◆",
}

-- Viewport overlay chips per persona: { key, label, default_on }. A chip that
-- omits `default_on` is engine-backed: `S.chip_value` reads it live each frame.
M.chips = {
    scene = { { "proj", "3D · Perspective", true }, { "snap", "Snap 8 px", false }, { "guides", "Guides", false } },
    script = {},
    anim = { { "path", "Motion path", true }, { "snap", "Snap 8 px", false } },
    phys = { { "colliders", "Show colliders", true }, { "sleep", "Sleep bodies" } },
    ui = { { "safe", "Safe area", true }, { "res", "1920 × 1080", false } },
}

return M
