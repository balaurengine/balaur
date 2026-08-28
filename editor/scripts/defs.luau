-- Static editor vocabulary: personas, tools, palette commands.
local M = {}

M.personas = {
    { id = "scene", name = "Scene", icon = "⌖" },
    { id = "script", name = "Script", icon = "‹›" },
    { id = "anim", name = "Animate", icon = "◷" },
    { id = "phys", name = "Physics", icon = "▣" },
    { id = "ui", name = "Interface", icon = "▤" },
}

M.tools = {
    scene = { "select", "move", "rotate", "scale", "rect", "tile", "zoom" },
    script = { "select", "code", "link", "eye", "zoom" },
    anim = { "select", "move", "bone", "key", "zoom" },
    phys = { "select", "collide", "poly", "body", "zoom" },
    ui = { "select", "layout", "anchor", "rect", "zoom" },
}

M.tool_names = {
    select = "Select", move = "Translate", rotate = "Rotate", scale = "Scale",
    rect = "Draw shape", tile = "Paint tiles", zoom = "Zoom",
    code = "Edit script", link = "Bind event", eye = "Watch value",
    bone = "Rig bone", key = "Key frame", collide = "Collider",
    poly = "Polygon", body = "Body type", layout = "Container", anchor = "Anchors",
}

M.tool_icons = {
    select = "⌖", move = "✥", rotate = "⟳", scale = "⤢", rect = "▭",
    tile = "▦", zoom = "⌕", code = "✎", link = "⚯", eye = "◉",
    bone = "⌇", key = "◆", collide = "▣", poly = "⬠", body = "●",
    layout = "▢", anchor = "⚓",
}

-- Viewport overlay chips per persona: { key, label, default_on }.
M.chips = {
    scene = { { "proj", "3D · Perspective", true }, { "snap", "Snap 8 px", false }, { "guides", "Guides", false } },
    script = {},
    anim = { { "onion", "Onion skin", true }, { "snap", "Snap 8 px", false } },
    phys = { { "colliders", "Show colliders", true }, { "sleep", "Sleep bodies", false } },
    ui = { { "safe", "Safe area", true }, { "res", "1920 × 1080", false } },
}

return M
