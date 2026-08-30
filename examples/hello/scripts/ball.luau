-- A bouncing ball: sets up its own physics in init (Godot-style _ready),
-- then watches its own fall from update.
local Ball = {}

function Ball:init()
    physics.add_body(self.node, physics.BODY_DYNAMIC)
    physics.add_ball_collider(self.node, 0.5)
    self.reported = false
end

function Ball:update(dt)
    local y = self.node:position()[2]
    if y < 0.2 and not self.reported then
        self.reported = true
        print("ball touched down at t =", string.format("%.2f", engine.time()))
    end
end

return Ball
