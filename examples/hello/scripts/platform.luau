-- A moving platform this script does not move: the engine samples the clip in
-- animations/platform.toml, and its method track calls `on_reached_end` here.
local Platform = {}

function Platform:init()
    self.turns = 0
end

function Platform:update(dt)
    -- P holds the clip where it is; P again carries on from there. `stop`
    -- would end it instead, leaving nothing to resume.
    if input.just_pressed(input.KEY_P) then
        if animation.is_playing(self.node) then
            animation.pause(self.node)
        else
            animation.resume(self.node)
        end
    end
end

-- Called by the clip's method track, not by anything here.
function Platform:on_reached_end()
    self.turns += 1
    -- A tween animates from the value the node has right now; `color/rgba`
    -- reaches a component property.
    animation.tween(self.node, {
        steps = {
            { property = "color/rgba", to = { 1.0, 0.8, 0.25, 1.0 }, duration = 0.25 },
            { property = "color/rgba", to = { 0.55, 0.35, 0.8, 1.0 }, duration = 0.75, ease = "out_quad" },
        },
    })
    print("platform turned", self.turns, "time(s)")
end

return Platform
