-- Luau syntax highlighting for the script editor, following the handoff's
-- tokenizer rules (keyword/string/number/comment/type/identifier/punct).
local M = {}

local KEYWORDS = {
    ["local"] = true, ["function"] = true, ["end"] = true, ["if"] = true,
    ["then"] = true, ["else"] = true, ["elseif"] = true, ["while"] = true,
    ["for"] = true, ["in"] = true, ["do"] = true, ["return"] = true,
    ["and"] = true, ["or"] = true, ["not"] = true, ["true"] = true,
    ["false"] = true, ["nil"] = true, ["break"] = true, ["continue"] = true,
    ["repeat"] = true, ["until"] = true, ["type"] = true, ["export"] = true,
}

-- Engine modules read as builtins/types, per the design's rule. All 13 of
-- them (docs/generated/script-api.md), then Luau's own globals. The list in
-- crates/balaur_ui/src/widgets.rs highlights the same source in the built-in
-- code editor, so a new module goes in both.
local BUILTIN = {
    engine = true, scene = true, input = true, physics = true, physics2d = true,
    render = true, audio = true, rng = true, ui = true, log = true, node = true,
    fs = true, toml = true,
    math = true, string = true, table = true, self = true, require = true,
}

-- Tokenize one line into { {text, color, strong} } spans.
function M.line(text, k)
    if string.match(text, "^%s*%-%-") then
        return { { text = text, color = k.k_com } }
    end
    local spans = {}
    local i = 1
    local prev_word = nil
    local len = #text
    while i <= len do
        local rest = string.sub(text, i)
        local s
        local color = k.k_punc
        local strong = false
        local ws = string.match(rest, "^%s+")
        local str = string.match(rest, '^"[^"]*"?') or string.match(rest, "^'[^']*'?")
        local num = string.match(rest, "^%d+%.?%d*")
        local word = string.match(rest, "^[%a_][%w_]*")
        if ws then
            s = ws
            color = k.k_fn
        elseif str then
            s = str
            color = k.k_str
        elseif num then
            s = num
            color = k.k_num
        elseif word then
            s = word
            if KEYWORDS[word] then
                color = k.k_key
                strong = true
            elseif BUILTIN[word] then
                color = k.k_type
            elseif string.match(word, "^%u") then
                color = k.k_type
            else
                color = k.k_fn
                strong = prev_word == "function"
            end
            prev_word = word
        else
            s = string.sub(rest, 1, 1)
        end
        table.insert(spans, { text = s, color = color, strong = strong })
        i += #s
    end
    return spans
end

-- Split source into lines and find `function Class:hook` definitions.
function M.analyze(source)
    local lines = {}
    local hooks = {}
    for line in string.gmatch(source .. "\n", "([^\n]*)\n") do
        table.insert(lines, line)
        local hook = string.match(line, "^function%s+[%w_]+[:.]([%w_]+)")
        if hook ~= nil then
            table.insert(hooks, { name = hook, line = #lines })
        end
    end
    return lines, hooks
end

return M
