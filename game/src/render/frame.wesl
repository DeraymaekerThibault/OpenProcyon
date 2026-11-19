// Not retail: what every shader of a graphics mod can read about this frame (game/src/render/frame.rs, our own work under the project
// licence). One storage buffer, written once a frame by the engine and bound at @binding(99) of every material the render profile
// draws with, so a shader needs no uniform of its own for the time, the sun, the camera, the storm's last bolt or the black holes.
// A mod shader imports it: `import openprocyon::render::frame::frame;` and reads `frame.clock.x`.

struct Frame {
    // x: real seconds since start (never wrapped), y: battle seconds (stop while paused), z: real frame delta, w: battle frame delta
    clock: vec4<f32>,
    // xyz: the direction the sunlight travels (world), w: its illuminance in units of 10000 lux (0: no sun)
    sun: vec4<f32>,
    // rgb: the sun's colour (linear), w: the flat ambient light's brightness (cd/m²)
    sun_color: vec4<f32>,
    // xyz: the battle camera's position (world)
    camera: vec4<f32>,
    // xyz: where the last bolt of storm lightning struck (a hull, else the bolt's middle, or a storm's centre for sheet lightning),
    // w: real seconds since it (very large: none yet)
    bolt_at: vec4<f32>,
    // xyz: one end of its segment, w: 1 when it struck a ship
    bolt_from: vec4<f32>,
    // xyz: the other end (equal ends: sheet lightning, a point of light)
    bolt_to: vec4<f32>,
    // x: black holes in `holes` (at most 4), y: the profile's `black_holes.lensing` (0 without the block)
    holes_info: vec4<f32>,
    // per hole, xyz: where the sky bends round it (the bottom of the whirlpool's throat, else the hole), w: its shadow's radius
    holes: array<vec4<f32>, 4>,
    // per hole, x: the pull radius, y: the kill core's radius, zw: unused
    hole_reach: array<vec4<f32>, 4>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(99) var<storage, read> frame: Frame;
