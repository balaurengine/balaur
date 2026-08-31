-- Angry-birds gameplay: slingshot aiming, launching, impact scoring and
-- restarts. The level itself is pure scene data (2D components); this
-- script only drives it, so everything stays editable in the balaur editor.
local Game = {}

local ANCHOR = { -8, 2.2 }
local MAX_PULL = 2.6
local POWER = 9
local KILL_IMPULSE = 3.0
local BIRDS = 3

function Game:init()
    self.root = self.node:parent()
    physics2d.set_gravity(0, -9.81)
    render.set_background(0.55, 0.78, 0.92)
    render.set_camera_2d(0, 3, 52)

    self.score = 0
    self.birds_left = BIRDS
    self.aiming = false
    self.launched = false
    self.rest_timer = 0
    self.over = false

    -- Capture initial state so R can rebuild the level in place. Each entry
    -- remembers its top-level owner so popping a pig pops its whole subtree.
    self.level = {}
    self.pigs = {}
    local function capture(self, node, owner)
        for _, child in node:children() do
            local top = owner or child:name()
            if child:has_component("shape2d") then
                local x, y, z = table.unpack(child:position())
                local rx, ry, rz = table.unpack(child:rotation_euler())
                table.insert(self.level, {
                    ref = child,
                    name = child:name(),
                    owner = top,
                    pos = { x, y, z },
                    rot = { rx, ry, rz },
                    shape2d = child:get_component("shape2d"),
                    collider2d = child:get_component("collider2d"),
                    body2d = child:get_component("body2d"),
                })
            end
            capture(self, child, top)
        end
    end
    capture(self, self.root, nil)
    for _, entry in self.level do
        if string.sub(entry.owner, 1, 3) == "Pig" and entry.name == entry.owner then
            self.pigs[entry.owner] = true
        end
    end
    self.bird = self.root:get_node("Bird")
end

local function set_widget_text(self, name, text)
    local node = self.root:get_node(name)
    if node == nil then
        return
    end
    local w = node:get_component("widget")
    if w ~= nil then
        w.text = text
        node:set_component("widget", w)
    end
end

-- Remove a node's 2D components: it disappears from both the renderer and
-- the physics world without being freed, so a restart can bring it back.
local function pop(entry)
    if entry.collider2d ~= nil then
        entry.ref:remove_component("collider2d")
    end
    if entry.body2d ~= nil then
        entry.ref:remove_component("body2d")
    end
    entry.ref:remove_component("shape2d")
end

function Game:restart()
    for _, entry in self.level do
        local ref = entry.ref
        ref:set_position(entry.pos[1], entry.pos[2], entry.pos[3])
        ref:set_rotation_euler(entry.rot[1], entry.rot[2], entry.rot[3])
        ref:set_component("shape2d", entry.shape2d)
        if entry.body2d ~= nil then
            ref:set_component("body2d", entry.body2d)
        end
        if entry.collider2d ~= nil then
            ref:set_component("collider2d", entry.collider2d)
        end
        if string.sub(entry.name, 1, 3) == "Pig" then
            self.pigs[entry.name] = true
        end
    end
    self.score = 0
    self.birds_left = BIRDS
    self.aiming = false
    self.launched = false
    self.rest_timer = 0
    self.over = false
    set_widget_text(self, "Score", "Score 0")
    set_widget_text(self, "Birds", `Birds {BIRDS}`)
    set_widget_text(self, "Hint", "Drag the bird, release to launch · R restarts")
end

local function pigs_alive(self)
    local n = 0
    for _, alive in self.pigs do
        if alive then
            n += 1
        end
    end
    return n
end

-- Park the bird back on the sling as a kinematic body, ready to aim.
local function rearm_bird(self)
    self.bird:set_position(ANCHOR[1], ANCHOR[2], 0)
    self.bird:set_rotation_euler(0, 0, 0)
    self.bird:set_component("body2d", { kind = "kinematic" })
    self.launched = false
    self.rest_timer = 0
end

local function finish(self, message)
    self.over = true
    set_widget_text(self, "Hint", message)
end

function Game:update(dt)
    if input.just_pressed("R") then
        self:restart()
        return
    end
    if self.over or not self.bird:is_valid() then
        return
    end

    local mx, my = render.mouse_world_2d()
    local bx, by = table.unpack(self.bird:position())

    -- Slingshot: grab the parked bird, pull, release to launch.
    if not self.launched then
        if input.mouse_just_pressed(input.MOUSE_LEFT) then
            local d = math.sqrt((mx - bx) ^ 2 + (my - by) ^ 2)
            if d < 1.4 then
                self.aiming = true
            end
        end
        if self.aiming then
            local dx, dy = mx - ANCHOR[1], my - ANCHOR[2]
            local len = math.sqrt(dx ^ 2 + dy ^ 2)
            if len > MAX_PULL then
                dx, dy = dx / len * MAX_PULL, dy / len * MAX_PULL
            end
            self.bird:set_position(ANCHOR[1] + dx, ANCHOR[2] + dy, 0)
            if input.mouse_just_released(input.MOUSE_LEFT) then
                self.aiming = false
                local px, py = table.unpack(self.bird:position())
                local ix, iy = (ANCHOR[1] - px) * POWER, (ANCHOR[2] - py) * POWER
                if math.sqrt(ix ^ 2 + iy ^ 2) > 1.0 then
                    self.bird:set_component("body2d", { kind = physics2d.BODY_DYNAMIC })
                    physics2d.apply_impulse(self.bird, ix, iy)
                    self.launched = true
                    self.birds_left -= 1
                    set_widget_text(self, "Birds", `Birds {self.birds_left}`)
                else
                    self.bird:set_position(ANCHOR[1], ANCHOR[2], 0)
                end
            end
        end
        -- Sling bands, drawn from the post to the bird.
        local px, py = table.unpack(self.bird:position())
        render.draw_line_2d(ANCHOR[1] - 0.14, ANCHOR[2] - 0.5, px, py, 0.35, 0.2, 0.12, 4)
        render.draw_line_2d(ANCHOR[1] + 0.14, ANCHOR[2] - 0.45, px, py, 0.3, 0.16, 0.1, 4)
    end

    -- Pigs pop when anything hits them hard enough.
    for name, alive in self.pigs do
        if alive then
            local pig = self.root:get_node(name)
            if pig == nil then
                self.pigs[name] = false
            else
                local x, y = table.unpack(pig:position())
                local impulse = physics2d.max_contact_impulse(pig)
                if impulse > KILL_IMPULSE or y < -3 then
                    for _, entry in self.level do
                        if entry.owner == name then
                            pop(entry)
                        end
                    end
                    self.pigs[name] = false
                    self.score += 500
                    set_widget_text(self, "Score", `Score {self.score}`)
                    log.info(name .. " popped (impulse " .. string.format("%.1f", impulse) .. ")")
                end
            end
        end
    end

    if pigs_alive(self) == 0 then
        self.score += self.birds_left * 1000
        set_widget_text(self, "Score", `Score {self.score}`)
        finish(self, "You win! · R to restart")
        return
    end

    -- Retire a spent bird: fell off the world, or came to rest.
    if self.launched then
        local vx, vy = physics2d.linear_velocity(self.bird)
        local speed = math.sqrt(vx ^ 2 + vy ^ 2)
        local x, y = table.unpack(self.bird:position())
        local gone = y < -3 or math.abs(x) > 20
        if speed < 0.25 then
            self.rest_timer += dt
        else
            self.rest_timer = 0
        end
        if gone or self.rest_timer > 1.2 then
            if self.birds_left > 0 then
                rearm_bird(self)
            else
                finish(self, "Out of birds · R to restart")
            end
        end
    end
end

return Game
