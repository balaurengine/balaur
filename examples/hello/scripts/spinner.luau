-- Spins in place; press Space (windowed runs) to reverse direction. Edit
-- SPEED while `balaur run` is going: the change applies the moment you save,
-- without losing the current angle.
local Spinner = {}

local SPEED = 2.0

function Spinner:init()
    self.angle = 0
    self.direction = 1
end

function Spinner:update(dt)
    if input.just_pressed(input.KEY_SPACE) then
        self.direction = -self.direction
    end
    self.angle += dt * SPEED * self.direction
    self.node:set_rotation_euler(0, self.angle, 0)
end

return Spinner
