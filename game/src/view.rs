//! Camera, world mouse/keyboard input and the F1 debug text.

use crate::Nearest;
use crate::bindings::Act;
use crate::commands::{Engine, Kind, Orders};
use crate::{
    GunStates, Health, Library, Outcome, PlayerShip, Ship, Target, audio, bindings, briefing, campaign, combat, course, docking, environment, menu, panels, rules, ship,
    toggle_pause,
};
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

#[derive(Component, Clone, Copy)]
pub enum Slot {
    Left,
    Right,
    Centre,
}

#[derive(Resource)]
pub struct Orbit {
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
    pub focus: Option<Entity>,
}

/// `input` is at Bevy's 16-parameter limit: the mouse accumulators travel together.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Pointer<'w> {
    pub motion: Res<'w, AccumulatedMouseMotion>,
    pub scroll: Res<'w, AccumulatedMouseScroll>,
    pub binds: Res<'w, crate::bindings::Bindings>,
    pub fleets: ResMut<'w, ship::Fleets>,
    pub plot: ResMut<'w, course::Plot>,
    pub free: Option<Res<'w, FreeCamera>>,
}

/// Not retail: the free camera of the bundled `free-camera` mod (on by default), present when the overlay has `data/camera.json`
/// (`{"pitch": [0.05, 1.5]}`): right-drag tilts between those pitches (radians). Without it the camera is the retail one.
#[derive(Resource, Clone, Copy, PartialEq, Debug)]
pub struct FreeCamera {
    pub pitch: (f32, f32),
}
impl FreeCamera {
    pub fn load(root: &std::path::Path) -> Option<Self> {
        let v = crate::data::try_json(&root.join("data/camera.json"))?;
        let p = |i: usize, d: f32| v["pitch"][i].as_f64().map_or(d, |x| x as f32);
        Some(FreeCamera { pitch: (p(0, 0.05), p(1, 1.5)) })
    }
}

/// Retail camera (`docs/re/camera.md`): height above the target point for a back distance `d`; the pitch follows from it.
pub fn retail_height(d: f32) -> f32 {
    0.065 * d.max(0.0).powf(1.4)
}
/// Retail zoom limits for a focus of this bounding radius: 1.2 radii (at least 2) out to 300 more.
pub fn retail_zoom_limits(radius: f32) -> (f32, f32) {
    let near = (1.2 * radius).max(2.0);
    (near, near + 300.0)
}
/// The retail wanted distance a battle starts at, raised to the focus's nearest.
pub const RETAIL_START_DIST: f32 = 20.0;
/// The original steps its camera keys once per world update; here they run on real time at this rate.
const RETAIL_STEPS_PER_SEC: f32 = 30.0;

/// Retail input: the pitch is not a control. Steps change the wanted distance (`orbit.dist`): outward by their size, inward by twice it.
fn retail_camera_input(mouse: &ButtonInput<MouseButton>, keys: &ButtonInput<KeyCode>, pointer: &Pointer, orbit: &mut Orbit, dt: f32, radius: f32) {
    let mut step = -pointer.scroll.delta.y * 10.0; // One scroll unit = one wheel notch (line units); pixel-unit touchpads zoom fast
    if mouse.pressed(MouseButton::Right) {
        orbit.yaw -= pointer.motion.delta.x * 0.005;
        step += pointer.motion.delta.y;
    }
    let per_tick = RETAIL_STEPS_PER_SEC * dt;
    if pointer.binds.pressed(keys, Act::RotateLeft) {
        orbit.yaw += 0.04 * per_tick;
    }
    if pointer.binds.pressed(keys, Act::RotateRight) {
        orbit.yaw -= 0.04 * per_tick;
    }
    if pointer.binds.pressed(keys, Act::ZoomIn) {
        step -= 3.0 * per_tick;
    }
    if pointer.binds.pressed(keys, Act::ZoomOut) {
        step += 3.0 * per_tick;
    }
    let (near, far) = retail_zoom_limits(radius);
    orbit.dist = (orbit.dist + if step > 0.0 { step } else { 2.0 * step }).clamp(near, far);
}

fn camera_input(mouse: &ButtonInput<MouseButton>, keys: &ButtonInput<KeyCode>, pointer: &Pointer, orbit: &mut Orbit, dt: f32, radius: f32) {
    // Camera controls use real time, independent of simulation speed or pause.
    let Some(free) = pointer.free.as_deref() else {
        return retail_camera_input(mouse, keys, pointer, orbit, dt, radius);
    };
    if mouse.pressed(MouseButton::Right) {
        orbit.yaw -= pointer.motion.delta.x * 0.005;
        orbit.pitch = (orbit.pitch + pointer.motion.delta.y * 0.005).clamp(free.pitch.0, free.pitch.1);
    }
    orbit.dist = (orbit.dist - pointer.scroll.delta.y * 3.0).clamp(10.0, 300.0);
    if pointer.binds.pressed(keys, Act::RotateLeft) {
        orbit.yaw += 1.5 * dt;
    }
    if pointer.binds.pressed(keys, Act::RotateRight) {
        orbit.yaw -= 1.5 * dt;
    }
    if pointer.binds.pressed(keys, Act::ZoomIn) {
        orbit.dist = (orbit.dist - 60.0 * dt).clamp(10.0, 300.0);
    }
    if pointer.binds.pressed(keys, Act::ZoomOut) {
        orbit.dist = (orbit.dist + 60.0 * dt).clamp(10.0, 300.0);
    }
}

/// The asteroids and space animals a click can pick.
type ObjectQuery<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static Transform, Option<&'static combat::Asteroid>, Option<&'static environment::SpaceAnimal>),
    Or<(With<combat::Asteroid>, With<environment::SpaceAnimal>)>,
>;

/// A ship as the click picking sees it: entity, transform, ship, destroyed, visible.
type Clickable = (Entity, Transform, Ship, bool, bool);

/// The selected ship, which this frame's input gives orders to (`commands::Orders`).
struct Controlled {
    actor: Entity,
    /// The player's own ship under direct control, rather than a captain's order.
    direct: bool,
}

/// What a click on the world looks up.
struct ClickScene<'a> {
    keys: &'a ButtonInput<KeyCode>,
    lib: &'a Library,
    sensors: &'a environment::Sensors,
    mission: Option<&'a campaign::Mission>,
    others: &'a [Clickable],
    objects: &'a ObjectQuery<'a, 'a>,
    player_pos: Vec3,
}

/// Speed, course and order keys of the selected ship.
fn keyboard_orders(c: &Controlled, keys: &ButtonInput<KeyCode>, pointer: &mut Pointer, vtime: &mut Time<Virtual>, orders: &mut Orders) {
    let b = &pointer.binds;
    let direct = c.direct;
    // Engine orders: +/- step through All Stop .. Emergency.
    if direct && (b.just_pressed(keys, Act::SpeedUp) || keys.just_pressed(KeyCode::NumpadAdd)) {
        orders.send(Kind::Engine { ship: c.actor, engine: Engine::Faster });
    }
    if direct && (b.just_pressed(keys, Act::SpeedDown) || keys.just_pressed(KeyCode::NumpadSubtract)) {
        orders.send(Kind::Engine { ship: c.actor, engine: Engine::Slower });
    }
    // Advanced movement: Enter accepts the plotted course, Backspace drops it (and nothing else).
    if direct && keys.just_pressed(KeyCode::Enter) && !pointer.plot.pending.is_empty() {
        orders.send(Kind::Course { ship: c.actor, points: std::mem::take(&mut pointer.plot.pending), release: false });
    }
    if keys.just_pressed(KeyCode::Backspace) && !pointer.plot.pending.is_empty() {
        pointer.plot.pending.clear();
    } else if keys.just_pressed(KeyCode::Backspace) {
        orders.send(Kind::Cancel { ship: c.actor });
    }
    if direct && keys.just_pressed(KeyCode::KeyC) {
        orders.send(Kind::Target { ship: c.actor, target: None, spot: None });
    }
    // Not retail: the Pause key toggles the pause too, beside the HUD pause button.
    if keys.just_pressed(KeyCode::Pause) {
        toggle_pause(vtime);
    }
}

/// The world ray under a window position (shared by every cursor pick).
pub fn cursor_ray(camera: &Camera, gt: &GlobalTransform, cursor: Vec2) -> Option<Ray3d> {
    camera.viewport_to_world(gt, cursor).ok()
}

/// Where a ray meets the sea plane (the world's y = 0 plane), if it does.
pub fn ray_on_sea(ray: Ray3d) -> Option<Vec3> {
    ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y)).map(|t| ray.get_point(t))
}

/// Window position of a world point, if the camera can project it.
pub fn project(camera: &Camera, gt: &GlobalTransform, world: Vec3) -> Option<Vec2> {
    camera.world_to_viewport(gt, world).ok()
}

/// An armed indirect bank takes this click as its shot target (a click with no ground point under it keeps the bank armed).
fn armed_shot(c: &Controlled, ray: Ray3d, target: &mut Target, lob: &combat::LobAim, sfx: &mut MessageWriter<audio::Sfx>, orders: &mut Orders) -> bool {
    let (true, Some(bank), Some(point)) = (c.direct, target.2.armed, ray_on_sea(ray)) else { return false };
    target.2.armed = None;
    orders.send(Kind::Lob { ship: c.actor, shot: Some((bank, point)) });
    // Retail plays a sound only for gravity charges (PROVISIONAL: star/nova silent as decoded, unconfirmed in play)
    if let Some(a) = lob.0.filter(|a| a.kind == crate::combat::LobKind::GravCharge) {
        sfx.write(audio::Sfx::ui(if a.solution { "HUD_LobWeapon_Accept" } else { "HUD_LobWeapon_Reject" }));
    }
    true
}

/// The nearest ship on the ray that passes `keep` (entity, distance).
fn nearest_ship(others: &[Clickable], lib: &Library, ray: Ray3d, keep: impl Fn(&Clickable) -> bool) -> Option<(Entity, f32)> {
    others.iter().filter(|o| keep(o)).filter_map(|(e, tf, s, _, _)| combat::ray_hit(&lib.classes[s.class], tf, ray).map(|t| (*e, t))).min_by(|a, b| a.1.total_cmp(&b.1))
}

/// A left click on one of the player's own ships selects it.
fn select_own(ray: Ray3d, scene: &ClickScene, fleets: &mut ship::Fleets, orbit: &mut Orbit) -> bool {
    let own = nearest_ship(scene.others, scene.lib, ray, |(e, _, _, h, m)| fleets.roster().contains(e) && !*h && *m);
    let Some((e, _)) = own else { return false };
    fleets.select(e, orbit);
    true
}

/// A left click that is not a selection: dock, pick a target or give a move order.
fn left_click(c: &Controlled, ray: Ray3d, scene: &ClickScene, pointer: &mut Pointer, docks: &docking::Dockees, orders: &mut Orders) {
    let (actor, direct) = (c.actor, c.direct);
    // Attack Only Mode (held key): a click only picks targets, never a dock or a destination.
    let attack_only = pointer.binds.pressed(scene.keys, Act::AttackOnly);
    // An allied ship or base with dock points: left-click docks when it is dockable and in sight (the cursor shows whether it can).
    if let Some((e, ok)) = docks.under(ray, actor).filter(|_| !attack_only) {
        if ok {
            // PROVISIONAL: unlike the menu's Dock, the click leaves a hold on (as before); not observed in the original.
            orders.send(Kind::Dock { ship: actor, dockee: e, release: false });
        }
        return;
    }
    let hit = nearest_ship(scene.others, scene.lib, ray, |(e, _, s, h, m)| {
        docks.table().targetable(crate::Side::Player, s.side, true) && !*h && *m && scene.sensors.player_sees(*e, scene.mission)
    });
    let object = object_hit(scene.objects.iter(), ray, scene.sensors, scene.mission, docks.table());
    let base = docks.hostile_base_hit(ray, scene.player_pos);
    let pick = [hit, object, base.map(|(e, t, _)| (e, t))].into_iter().flatten().min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((e, t)) = pick {
        // On the command ship, remember the clicked spot on the hull (ship-local) so the guns aim there; a captain just attacks.
        let at = ray.origin + *ray.direction * t;
        let spot = if direct {
            scene.others.iter().find(|o| o.0 == e).map(|o| o.1.to_matrix().inverse().transform_point3(at)).or_else(|| base.filter(|b| b.0 == e).map(|b| b.2))
        } else {
            None
        };
        orders.send(Kind::Target { ship: actor, target: Some(e), spot });
        return;
    }
    if attack_only {
        return;
    }
    let Some(point) = ray_on_sea(ray) else { return };
    move_order(c, point, scene.keys, pointer, orders);
}

/// A click on empty water: plot a course point (advanced movement) or order the ship there (Shift appends).
fn move_order(c: &Controlled, p: Vec3, keys: &ButtonInput<KeyCode>, pointer: &mut Pointer, orders: &mut Orders) {
    let (ship, direct) = (c.actor, c.direct);
    if direct && pointer.plot.advanced {
        match pointer.plot.pressed(p) {
            Some(true) => orders.send(Kind::Course { ship, points: std::mem::take(&mut pointer.plot.pending), release: true }),
            Some(false) => pointer.plot.pending.clear(),
            None => pointer.plot.pending.push(p),
        }
        return;
    }
    orders.send(Kind::Move { ship, to: p, append: direct && keys.pressed(KeyCode::ShiftLeft) });
}

pub fn input(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut pointer: Pointer,
    window: Single<&Window, With<PrimaryWindow>>,
    cam: Single<(&Camera, &GlobalTransform)>,
    mut orbit: ResMut<Orbit>,
    ships: Query<(Entity, &Ship, &GlobalTransform, &Health, Option<&campaign::MissionShip>)>,
    time: Res<Time<Real>>,
    lib: Res<Library>,
    mut target: ResMut<Target>,
    mut vtime: ResMut<Time<Virtual>>,
    focus: panels::UiFocus,
    docks: docking::Dockees,
    (sensors, mission, mut sfx, lob, mut orders): (Res<environment::Sensors>, Option<Res<campaign::Mission>>, MessageWriter<audio::Sfx>, Res<combat::LobAim>, Orders),
    objects: ObjectQuery,
) {
    let Some(actor) = pointer.fleets.selected else { return };
    let direct = pointer.fleets.direct();
    let clicking = mouse.just_pressed(MouseButton::Left) && !focus.blocks_world_click();
    let others: Vec<Clickable> =
        if clicking { ships.iter().map(|(e, s, tf, h, m)| (e, tf.compute_transform(), s.clone(), h.destroyed, m.is_none_or(|m| m.visible))).collect() } else { vec![] };
    let Ok((_, _, player_tf, ..)) = ships.get(actor) else { return };
    if !briefing::active(mission.as_deref()) && !rules::locked(mission.as_deref()) {
        let c = Controlled { actor, direct };
        // The focus's bounding radius sets the retail zoom limits (collision sphere for the mesh bounds, PROVISIONAL; others 0).
        let radius = orbit.focus.or(Some(actor)).and_then(|e| ships.get(e).ok()).map_or(0.0, |(_, s, ..)| lib.classes[s.class].sphere.1);
        camera_input(&mouse, &keys, &pointer, &mut orbit, time.delta_secs(), radius);
        keyboard_orders(&c, &keys, &mut pointer, &mut vtime, &mut orders);
        // Left click: an enemy ship selects it as target, otherwise set a destination on the plane (Shift appends).
        if clicking {
            let (camera, gt) = *cam;
            if let Some(ray) = window.cursor_position().and_then(|cursor| cursor_ray(camera, gt, cursor)) {
                let scene =
                    ClickScene { keys: &keys, lib: &lib, sensors: &sensors, mission: mission.as_deref(), others: &others, objects: &objects, player_pos: player_tf.translation() };
                if !armed_shot(&c, ray, &mut target, &lob, &mut sfx, &mut orders) && !select_own(ray, &scene, &mut pointer.fleets, &mut orbit) {
                    left_click(&c, ray, &scene, &mut pointer, &docks, &mut orders);
                }
            }
        }
    }
}

pub fn fleet_keys(
    keys: Res<ButtonInput<KeyCode>>,
    binds: Res<bindings::Bindings>,
    mut fleets: ResMut<ship::Fleets>,
    mut orbit: ResMut<Orbit>,
    mission: Option<Res<campaign::Mission>>,
    mut orders: Orders,
    mut sfx: MessageWriter<audio::Sfx>,
    focus: panels::UiFocus,
) {
    if focus.blocks_fleet_keys() {
        return;
    }
    let mut picked = None;
    if binds.just_pressed(&keys, Act::Primary) {
        picked = fleets.primary().map(|e| (e, "HUD_SelectShip_Primary"));
    }
    for n in 1..=10 {
        if binds.just_pressed(&keys, Act::Secondary(n)) {
            picked = fleets.roster().get(n as usize).copied().map(|e| (e, "HUD_SelectShip_Secondary"));
        }
    }
    if let Some((e, sound)) = picked
        && fleets.select(e, &mut orbit)
    {
        sfx.write(audio::Sfx::ui(sound));
    }
    if binds.just_pressed(&keys, Act::NextShip) {
        fleets.cycle(true, &mut orbit);
    }
    if binds.just_pressed(&keys, Act::PrevShip) {
        fleets.cycle(false, &mut orbit);
    }
    if binds.just_pressed(&keys, Act::FocusSelected) {
        orbit.focus = fleets.selected;
    }
    if binds.just_pressed(&keys, Act::ShipBar) {
        fleets.bar_open = !fleets.bar_open;
        sfx.write(audio::Sfx::ui(if fleets.bar_open { "HUD_ShipBar_Open" } else { "HUD_ShipBar_Close" }));
    }
    if binds.just_pressed(&keys, Act::ChangePrimary)
        && mission.is_none()
        && let Some(e) = fleets.selected
    {
        orders.send(Kind::Promote { ship: e });
    }
}

pub fn screenshot_key(keys: Res<ButtonInput<KeyCode>>, binds: Res<bindings::Bindings>, mut commands: Commands) {
    if !binds.just_pressed(&keys, Act::Screenshot) {
        return;
    }
    let Some(dir) = campaign::user_dir().map(|d| d.join("screenshots")) else { return };
    let _ = std::fs::create_dir_all(&dir);
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let path = dir.join(format!("procyon-{stamp}.png"));
    info!("screenshot {}", path.display());
    commands.spawn(bevy::render::view::screenshot::Screenshot::primary_window()).observe(bevy::render::view::screenshot::save_to_disk(path));
}

/// Retail 'Focus On Nearest Projectile': the camera follows the shell or torpedo in flight nearest the selected ship.
pub fn focus_projectile(
    keys: Res<ButtonInput<KeyCode>>,
    binds: Res<bindings::Bindings>,
    fleets: Res<ship::Fleets>,
    mut orbit: ResMut<Orbit>,
    ships: Query<&Transform, With<Ship>>,
    shells: Query<(Entity, &Transform), With<combat::Shell>>,
) {
    if !binds.just_pressed(&keys, Act::FocusProjectile) {
        return;
    }
    let Some(from) = fleets.selected.and_then(|e| ships.get(e).ok()).map(|t| t.translation) else { return };
    if let Some((e, _)) = shells.iter().nearest_to(from, |a| a.1.translation) {
        orbit.focus = Some(e);
    }
}

pub fn toggle_debug(keys: Res<ButtonInput<KeyCode>>, mut q: Query<&mut Visibility, With<Slot>>) {
    if keys.just_pressed(KeyCode::F1) {
        for mut v in &mut q {
            *v = if *v == Visibility::Hidden { Visibility::Inherited } else { Visibility::Hidden };
        }
    }
}

#[derive(Component)]
pub struct Sky;

pub fn sky(
    cam: Single<&Transform, (With<Camera3d>, With<crate::MainCamera>, Without<Sky>)>,
    mut sky: Single<(&mut Transform, &mut Visibility), With<Sky>>,
    settings: Option<Res<menu::Settings>>,
) {
    sky.0.translation = cam.translation;
    // Display Options 'Render Background'.
    let want = if settings.is_some_and(|s| !s.background) { Visibility::Hidden } else { Visibility::Inherited };
    sky.1.set_if_neq(want);
}

/// Not retail: a baked cube map for this retail sky (`models/sky/<sky>_cube.png`, six square faces stacked +X -X +Y -Y +Z -Z),
/// when a mod provides one: the scenario draws it instead of the sky sphere.
pub fn hd_sky_file(root: &std::path::Path, sky: &str) -> Option<String> {
    let rel = format!("models/sky/{sky}_cube.png");
    root.join(&rel).is_file().then_some(rel)
}

/// On the `Sky` entity in place of the sphere's scene.
#[derive(Component)]
pub struct HdSky(pub Handle<Image>);

/// Turns a loaded image of six square faces stacked +X -X +Y -Y +Z -Z into a cube map, once; false (with a warning) when it
/// is not that shape. Already a cube: true.
pub fn as_cube(image: &mut Image) -> bool {
    use bevy::render::render_resource::{TextureViewDescriptor, TextureViewDimension};
    if image.texture_descriptor.size.depth_or_array_layers == 1 {
        if image.height() != image.width() * 6 {
            warn_once!("baked sky is {}x{}, not six square faces stacked: drawing none", image.width(), image.height());
            return false;
        }
        image.reinterpret_stacked_2d_as_array(6).expect("six stacked faces");
        image.texture_view_descriptor = Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });
    }
    true
}

/// Puts the baked sky on the battle camera as its skybox once the image has loaded, and takes it off again when the
/// sky is hidden ('Render Background') or gone (battle over).
pub fn hd_sky(mut commands: Commands, cams: Query<(Entity, Option<&bevy::light::Skybox>), With<Camera3d>>, sky: Query<(&HdSky, &Visibility)>, mut images: ResMut<Assets<Image>>) {
    let Some(want) = sky.iter().find(|(_, v)| **v != Visibility::Hidden).map(|(s, _)| &s.0) else {
        for (cam, _) in cams.iter().filter(|(_, s)| s.is_some()) {
            commands.entity(cam).remove::<bevy::light::Skybox>();
        }
        return;
    };
    let Ok((cam, current)) = cams.single() else { return };
    if current.and_then(|s| s.image.as_ref()) == Some(want) {
        return;
    }
    let Some(mut loaded) = images.get_mut(want) else { return };
    if !as_cube(&mut loaded) {
        return;
    }
    drop(loaded);
    // Brightness that cancels the camera's default exposure, so the texels show as they are, like the unlit sphere.
    commands.entity(cam).insert(bevy::light::Skybox { image: Some(want.clone()), brightness: 1.0 / bevy::camera::Exposure::default().exposure(), ..default() });
}

/// The background stars (`docs/re/sky.md`): per group, its star count and colour (sRGB, drawn opaque).
pub const STAR_GROUPS: [(usize, [f32; 3]); 6] =
    [(2500, [0.75, 0.75, 0.75]), (600, [0.89, 0.909, 0.99]), (600, [0.804, 0.6, 0.6]), (300, [0.908, 0.909, 0.76]), (1000, [0.4235, 0.545, 0.557]), (30, [1.0, 1.0, 1.0])];
/// White stars added to the last group as a band, each near one of the `STAR_BAND_RECENT` newest.
pub const STAR_BAND: usize = 1000;
pub const STAR_BAND_RECENT: usize = 30;
/// Share of the sky sphere's radius the stars sit at: inside the low-poly sphere's facets, so the sky never hides them.
const STAR_RADIUS: f32 = 0.9;

/// The original's cosmetic linear generator (x * 0x19660d + 0x3c6ef35f), which it seeds from the clock.
struct StarRng(u32);
impl StarRng {
    fn step(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(0x19660d).wrapping_add(0x3c6ef35f);
        self.0
    }
    /// The low 23 bits as a fraction in [0, 1).
    fn unit(&mut self) -> f32 {
        crate::fx::lcg_unit(&mut self.0)
    }
    /// Uniform direction: height first, then the turn about the vertical.
    fn dir(&mut self) -> Vec3 {
        let z = self.unit() * 2.0 - 1.0;
        let phi = self.unit() * std::f32::consts::TAU;
        let r = (1.0 - z * z).max(0.0).sqrt();
        Vec3::new(phi.cos() * r, phi.sin() * r, z)
    }
}

/// The star groups of `STAR_GROUPS`, in game axes: every star a random point of the unit sphere, then the band: star i of
/// `STAR_BAND` lies a random distance up to (i + 5) / 1000 from a random one of the last group's `STAR_BAND_RECENT` newest.
pub fn starfield(seed: u32) -> Vec<Vec<Vec3>> {
    let mut rng = StarRng(seed);
    let mut groups: Vec<Vec<Vec3>> = STAR_GROUPS.iter().map(|&(n, _)| (0..n).map(|_| rng.dir()).collect()).collect();
    let band = groups.last_mut().expect("six groups");
    for i in 0..STAR_BAND {
        let start = band.len().saturating_sub(STAR_BAND_RECENT);
        let pick = start + ((rng.step() as i32) % (band.len() - start) as i32).unsigned_abs() as usize;
        let reach = rng.unit() * (i + 5) as f32 * 0.001;
        let star = band[pick] + rng.dir() * reach;
        band.push(star);
    }
    groups
}

/// One point list for the camera-centred `Sky` entity (1 pixel per star): each star in its direction from the camera.
pub fn star_mesh(groups: &[Vec<Vec3>]) -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::PrimitiveTopology;
    let (mut pos, mut col) = (vec![], vec![]);
    for (stars, (_, c)) in groups.iter().zip(STAR_GROUPS) {
        let linear = Color::srgb(c[0], c[1], c[2]).to_linear().to_f32_array();
        for s in stars {
            let d = s.normalize_or_zero() * STAR_RADIUS;
            pos.push([d.x, d.z, -d.y]);
            col.push(linear);
        }
    }
    Mesh::new(PrimitiveTopology::PointList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
}

#[expect(clippy::too_many_arguments, reason = "a Bevy system")]
pub fn camera(
    mut orbit: ResMut<Orbit>,
    ship: Query<&Transform, (With<PlayerShip>, Without<Camera3d>)>,
    tfs: Query<&Transform, Without<Camera3d>>,
    contacts: Query<(), Or<(With<Ship>, With<environment::SpaceAnimal>)>>,
    sensors: Res<environment::Sensors>,
    mut cam: Single<&mut Transform, (With<Camera3d>, With<crate::MainCamera>)>,
    mission: Option<Res<campaign::Mission>>,
    (free, time, mut shown): (Option<Res<FreeCamera>>, Option<Res<Time<Real>>>, Local<Option<f32>>),
) {
    if briefing::active(mission.as_deref()) || (rules::nis(mission.as_deref()) && !rules::nis_camera_idle(mission.as_deref())) || rules::console::free_cam() {
        return;
    }
    // `--look x,y[,z]` (game axes) aims screenshots at a world point instead of the player's ship.
    static LOOK: std::sync::OnceLock<Option<Vec3>> = std::sync::OnceLock::new();
    let look = LOOK.get_or_init(|| {
        let v: Vec<f32> = crate::cli::value("--look")?.split(',').filter_map(|x| x.parse().ok()).collect();
        Some(Vec3::new(*v.first()?, *v.get(2).unwrap_or(&0.0), -*v.get(1)?))
    });
    let tracked = orbit.focus.filter(|e| !contacts.contains(*e) || sensors.player_sees(*e, mission.as_deref()));
    let lift = if free.is_some() { 3.0 } else { 2.0 };
    let focus = look.unwrap_or_else(|| tracked.and_then(|e| tfs.get(e).ok()).or_else(|| ship.iter().next()).map_or(Vec3::ZERO, |t| t.translation)) + Vec3::Y * lift;
    let len = if free.is_some() {
        orbit.dist
    } else {
        // Retail: the shown distance eases toward the wanted one; the camera stands that far back and `retail_height` up.
        let dt = time.map_or(0.0, |t| t.delta_secs());
        let d = shown.map_or(orbit.dist, |s| s + (orbit.dist - s) * (6.5 * dt).min(1.0));
        *shown = Some(d);
        let h = retail_height(d);
        let pitch = h.atan2(d);
        if orbit.pitch != pitch {
            orbit.pitch = pitch;
        }
        d.hypot(h)
    };
    let off = Quat::from_rotation_y(orbit.yaw) * Quat::from_rotation_x(-orbit.pitch) * Vec3::new(0.0, 0.0, len);
    cam.translation = focus + off;
    cam.look_at(focus, Vec3::Y);
}

pub fn hud(
    lib: Res<Library>,
    out: Res<Outcome>,
    target: Res<Target>,
    player: Single<(&Ship, &Health, &GunStates), With<PlayerShip>>,
    others: Query<(&Ship, &Health), Without<PlayerShip>>,
    mut texts: Query<(&Slot, &mut Text)>,
) {
    let (ship, health, guns) = *player;
    let class = &lib.classes[ship.class];
    let (ready, total) = combat::guns_ready(guns, class, health);
    let living = others.iter().filter(|(_, h)| !h.destroyed).count();
    let left = format!(
        "{}\nOrder: {} (+/-)\nSpeed: {:.1} (max {:.1})\nHeading: {:03.0}\nGuns ready: {}/{}   Enemies: {}\nCore {:.0}/{:.0}\n{}\nLMB: target enemy / set course (Shift adds, X clears)\nC: clear target  R: restart  RMB drag, arrows: view",
        class.name,
        ship.order.label(),
        ship.speed,
        class.phys.max_speed * combat::speed_factor(class, health),
        (-ship.heading.to_degrees()).rem_euclid(360.0),
        ready,
        total,
        living,
        health.core.max(0.0),
        class.core_hp,
        combat::section_lines(class, health),
    );
    let right = match target.0.and_then(|e| others.get(e).ok()) {
        Some((s, h)) => {
            let c = &lib.classes[s.class];
            format!("TARGET {}{}\nCore {:.0}/{:.0}\n{}", c.name, if h.destroyed { " (destroyed)" } else { "" }, h.core.max(0.0), c.core_hp, combat::section_lines(c, h))
        }
        None => "No target: guns pick the nearest enemy\n(click an enemy ship to choose)".into(),
    };
    let centre = match *out {
        Outcome::Playing => "",
        Outcome::Victory => "VICTORY\npress R to restart",
        Outcome::Defeat => "DEFEAT\npress R to restart",
    };
    for (slot, mut t) in &mut texts {
        let text = match slot {
            Slot::Left => left.as_str(),
            Slot::Right => right.as_str(),
            Slot::Centre => centre,
        };
        if **t != text {
            **t = text.into(); // only on change: an assignment re-lays the text out
        }
    }
}

/// The nearest visible asteroid or hostile space animal on the ray (entity, distance); shared by the click and the hover cursor.
pub fn object_hit<'a>(
    objects: impl Iterator<Item = (Entity, &'a Transform, Option<&'a combat::Asteroid>, Option<&'a environment::SpaceAnimal>)>,
    ray: Ray3d,
    sensors: &environment::Sensors,
    mission: Option<&campaign::Mission>,
    table: &crate::alliance::Alliances,
) -> Option<(Entity, f32)> {
    objects
        .filter_map(|(e, tf, rock, animal)| {
            if !sensors.player_sees(e, mission) {
                return None;
            }
            let radius = if let Some(a) = animal {
                if !a.visible || !table.targetable(crate::Side::Player, table.side_or_neutral(&a.owner), true) {
                    return None;
                }
                a.radius
            } else {
                rock?.radius
            };
            combat::ray_sphere(ray, tf.translation, radius).map(|t| (e, t))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// Presentation-independent view systems that run outside the battle's ordered chain.
pub fn plugin(app: &mut App) {
    app.add_systems(Update, (hd_sky, screenshot_key, focus_projectile.run_if(in_state(menu::Screen::Battle))));
}

#[cfg(test)]
mod camera_tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use std::time::Duration;

    #[test]
    #[ignore = "needs imported assets"]
    fn focus_does_not_track_a_ship_outside_group_sight_but_allows_projectile_debug_focus() {
        let (mut world, ids) = ship::movement_tests::fleet_world();
        world.init_resource::<environment::Sensors>();
        world.insert_resource(FREE);
        let cam = world.spawn((crate::MainCamera, Camera3d::default(), Transform::default())).id();
        world.resource_mut::<Orbit>().focus = Some(ids[5]);
        world.get_mut::<Transform>(ids[5]).unwrap().translation = Vec3::X * 3000.0;
        world.run_system_once(environment::refresh_sensors).unwrap();
        world.resource_mut::<environment::Sensors>().advance_sightings(None, 0.0, 0.0);
        world.run_system_once(camera).unwrap();
        assert_eq!(world.get::<Transform>(cam).unwrap().translation, Vec3::new(0.0, 3.0, 60.0));
        world.get_mut::<Transform>(ids[5]).unwrap().translation = Vec3::X * 120.0;
        world.run_system_once(environment::refresh_sensors).unwrap();
        world.resource_mut::<environment::Sensors>().advance_sightings(None, 1.5, 1.5); // the next once-a-second re-test
        world.run_system_once(camera).unwrap();
        assert_eq!(world.get::<Transform>(cam).unwrap().translation, Vec3::new(120.0, 3.0, 60.0));
        let shell = world.spawn(Transform::from_xyz(600.0, 0.0, 0.0)).id();
        world.resource_mut::<Orbit>().focus = Some(shell);
        world.run_system_once(camera).unwrap();
        assert_eq!(world.get::<Transform>(cam).unwrap().translation, Vec3::new(600.0, 3.0, 60.0));
    }

    const FREE: FreeCamera = FreeCamera { pitch: (0.05, 1.5) };

    fn input_world() -> (World, [Entity; 6]) {
        let (mut world, ids) = ship::movement_tests::fleet_world();
        for &e in &ids {
            world.entity_mut(e).insert(GlobalTransform::default());
        }
        world.resource_mut::<ship::Fleets>().selected = Some(ids[0]);
        world.init_resource::<crate::commands::Outbox>();
        world.spawn((Window::default(), PrimaryWindow));
        world.spawn((Camera::default(), GlobalTransform::default()));
        world.init_resource::<ButtonInput<MouseButton>>();
        world.init_resource::<ButtonInput<KeyCode>>();
        world.init_resource::<AccumulatedMouseMotion>();
        world.init_resource::<AccumulatedMouseScroll>();
        world.init_resource::<crate::bindings::Bindings>();
        world.init_resource::<course::Plot>();
        world.init_resource::<panels::UiClick>();
        world.init_resource::<crate::options_menu::OptionsMenu>();
        world.init_resource::<environment::Sensors>();
        world.init_resource::<Messages<docking::DockMsg>>();
        world.init_resource::<Messages<audio::Sfx>>();
        world.init_resource::<combat::LobAim>();
        world.init_resource::<Time<Real>>();
        world.init_resource::<Time<Virtual>>();
        world.init_resource::<Time>();
        (world, ids)
    }

    #[test]
    fn retail_height_and_limits_follow_the_originals_numbers() {
        // the retail save: 100 back, 41.02 up
        assert!((retail_height(100.0) - 41.01).abs() < 0.01);
        let pitch = |d: f32| retail_height(d).atan2(d).to_degrees();
        assert!((pitch(100.0) - 22.3).abs() < 0.1 && (pitch(7.67) - 8.4).abs() < 0.1 && (pitch(307.67) - 32.7).abs() < 0.1);
        assert_eq!(retail_zoom_limits(1.0), (2.0, 302.0));
        assert_eq!(retail_zoom_limits(10.0), (12.0, 312.0));
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn retail_steps_zoom_out_by_their_size_in_by_twice_and_never_tilt() {
        let (mut world, ids) = input_world();
        let radius = world.resource::<Library>().classes[world.get::<Ship>(ids[0]).unwrap().class].sphere.1;
        let (near, far) = retail_zoom_limits(radius);
        let run = |world: &mut World, wheel: f32, drag: Vec2| {
            world.resource_mut::<AccumulatedMouseScroll>().delta = Vec2::new(0.0, wheel);
            world.resource_mut::<AccumulatedMouseMotion>().delta = drag;
            world.insert_resource(Orbit { yaw: 0.0, pitch: 0.5, dist: 100.0, focus: Some(ids[0]) });
            world.run_system_once(input).unwrap();
            let o = world.resource::<Orbit>();
            (o.dist, o.yaw, o.pitch)
        };
        assert_eq!(run(&mut world, 1.0, Vec2::ZERO).0, 80.0, "a notch in: 20 closer");
        assert_eq!(run(&mut world, -1.0, Vec2::ZERO).0, 110.0, "a notch out: 10 farther");
        world.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
        let (dist, yaw, pitch) = run(&mut world, 0.0, Vec2::new(20.0, 5.0));
        assert_eq!((dist, pitch), (105.0, 0.5), "a vertical drag zooms; it does not tilt");
        assert!((yaw + 0.1).abs() < 1e-6);
        assert_eq!(run(&mut world, 0.0, Vec2::new(0.0, -1000.0)).0, near);
        assert_eq!(run(&mut world, 0.0, Vec2::new(0.0, 1000.0)).0, far);
    }

    #[test]
    fn retail_camera_stands_back_and_up_by_the_height_rule_and_eases_to_the_wanted_distance() {
        let mut world = World::new();
        world.init_resource::<environment::Sensors>();
        world.insert_resource(Orbit { yaw: 0.0, pitch: 0.0, dist: 100.0, focus: None });
        let cam = world.spawn((crate::MainCamera, Camera3d::default(), Transform::default())).id();
        let mut system = IntoSystem::into_system(camera);
        system.initialize(&mut world);
        system.run((), &mut world).unwrap();
        let t = world.get::<Transform>(cam).unwrap().translation;
        assert!((t - Vec3::new(0.0, 2.0 + retail_height(100.0), 100.0)).length() < 1e-3, "{t}");
        assert!((world.resource::<Orbit>().pitch - retail_height(100.0).atan2(100.0)).abs() < 1e-6);
        // the wanted distance jumps; the shown one closes 6.5 x dt of the gap
        world.resource_mut::<Orbit>().dist = 200.0;
        let mut real = Time::<Real>::default();
        real.advance_by(Duration::from_secs_f32(0.1));
        world.insert_resource(real);
        system.run((), &mut world).unwrap();
        let back = world.get::<Transform>(cam).unwrap().translation.z;
        assert!((back - 165.0).abs() < 1e-3, "{back}");
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn keyboard_camera_rotation_and_zoom_stay_at_one_x_at_every_simulation_speed() {
        let (mut world, ids) = ship::movement_tests::fleet_world();
        world.insert_resource(FREE);
        for e in ids {
            world.entity_mut(e).insert(GlobalTransform::default());
        }
        world.resource_mut::<ship::Fleets>().selected = Some(ids[0]);
        world.init_resource::<crate::commands::Outbox>();
        world.spawn((Window::default(), PrimaryWindow));
        world.spawn((Camera::default(), GlobalTransform::default()));
        world.init_resource::<ButtonInput<MouseButton>>();
        world.init_resource::<ButtonInput<KeyCode>>();
        world.init_resource::<AccumulatedMouseMotion>();
        world.init_resource::<AccumulatedMouseScroll>();
        world.init_resource::<crate::bindings::Bindings>();
        world.init_resource::<course::Plot>();
        world.init_resource::<panels::UiClick>();
        world.init_resource::<crate::options_menu::OptionsMenu>();
        world.init_resource::<environment::Sensors>();
        world.init_resource::<Messages<docking::DockMsg>>();
        world.init_resource::<Messages<audio::Sfx>>();
        world.init_resource::<combat::LobAim>();
        world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ArrowLeft);
        world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ArrowUp);
        let mut real = Time::<Real>::default();
        real.advance_by(Duration::from_secs_f32(0.1));
        world.insert_resource(real);
        for speed in [0.0, 1.0, 2.0, 4.0, 8.0, 16.0] {
            let mut virtual_time = Time::<Virtual>::default();
            if speed == 0.0 {
                virtual_time.pause();
            } else {
                virtual_time.set_relative_speed(speed);
            }
            virtual_time.advance_by(Duration::from_secs_f32(0.1 * speed));
            world.insert_resource(virtual_time.as_generic());
            world.insert_resource(virtual_time);
            world.insert_resource(Orbit { yaw: 0.0, pitch: 0.5, dist: 100.0, focus: Some(ids[0]) });
            world.run_system_once(input).unwrap();
            let orbit = world.resource::<Orbit>();
            assert!((orbit.yaw - 0.15).abs() < 0.00001, "rotation at {speed}x: {}", orbit.yaw);
            assert!((orbit.dist - 94.0).abs() < 0.00001, "zoom at {speed}x: {}", orbit.dist);
        }
        world.resource_mut::<ButtonInput<KeyCode>>().reset_all();
        world.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
        world.resource_mut::<AccumulatedMouseMotion>().delta = Vec2::new(20.0, 500.0);
        world.resource_mut::<AccumulatedMouseScroll>().delta = Vec2::new(0.0, 100.0);
        world.insert_resource(Orbit { yaw: 0.0, pitch: 0.5, dist: 100.0, focus: Some(ids[0]) });
        world.run_system_once(input).unwrap();
        let orbit = world.resource::<Orbit>();
        assert!((orbit.yaw + 0.1).abs() < 0.00001);
        assert_eq!(orbit.pitch, 1.5);
        assert_eq!(orbit.dist, 10.0);
    }
}

#[cfg(test)]
mod star_tests {
    use super::*;

    #[test]
    fn six_groups_of_the_retail_counts_on_the_unit_sphere_plus_a_white_band() {
        let groups = starfield(12345);
        let counts: Vec<usize> = groups.iter().map(Vec::len).collect();
        assert_eq!(counts, [2500, 600, 600, 300, 1000, 30 + 1000]);
        assert!(groups.iter().flatten().take(5030).all(|s| (s.length() - 1.0).abs() < 1e-4), "scattered stars lie on the unit sphere");
        let band = &groups[5];
        for i in 0..STAR_BAND {
            let n = 30 + i;
            let star = band[n];
            let reach = (i + 5) as f32 * 0.001 + 1e-5;
            assert!(band[n.saturating_sub(STAR_BAND_RECENT)..n].iter().any(|s| s.distance(star) <= reach), "band star {i} grows from one of the 30 newest");
        }
        assert_ne!(starfield(1)[0][0], starfield(2)[0][0], "the seed changes the pattern");
    }

    #[test]
    fn the_generator_is_the_originals_linear_step() {
        let mut rng = StarRng(0);
        assert_eq!(rng.step(), 0x3c6ef35f);
        assert_eq!(rng.step(), 0x3c6ef35fu32.wrapping_mul(0x19660d).wrapping_add(0x3c6ef35f));
    }

    #[test]
    fn star_mesh_is_one_point_per_star_inside_the_sky_sphere() {
        let mesh = star_mesh(&starfield(7));
        assert_eq!(mesh.primitive_topology(), bevy::mesh::PrimitiveTopology::PointList);
        assert_eq!(mesh.count_vertices(), 6030);
    }
}

#[cfg(test)]
mod hd_sky_tests {
    use super::*;
    use bevy::asset::RenderAssetUsages;
    use bevy::ecs::system::RunSystemOnce;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureViewDimension};

    fn stacked(width: u32, height: u32) -> Image {
        Image::new_fill(Extent3d { width, height, depth_or_array_layers: 1 }, TextureDimension::D2, &[0, 0, 0, 255], TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default())
    }

    #[test]
    fn baked_sky_is_the_camera_skybox_while_shown_and_leaves_with_the_sky() {
        let mut world = World::new();
        world.init_resource::<Assets<Image>>();
        let handle = world.resource_mut::<Assets<Image>>().add(stacked(4, 24));
        let cam = world.spawn(Camera3d::default()).id();
        let sky = world.spawn((HdSky(handle.clone()), Visibility::Inherited)).id();
        let skybox = |world: &World| world.get::<bevy::light::Skybox>(cam).and_then(|s| s.image.clone());
        world.run_system_once(hd_sky).unwrap();
        assert_eq!(skybox(&world), Some(handle.clone()));
        let image = world.resource::<Assets<Image>>().get(&handle).unwrap();
        assert_eq!(image.texture_descriptor.size, Extent3d { width: 4, height: 4, depth_or_array_layers: 6 });
        assert_eq!(image.texture_view_descriptor.as_ref().and_then(|v| v.dimension), Some(TextureViewDimension::Cube));
        // 'Render Background' off hides it, on brings it back (the image is reinterpreted once)
        *world.get_mut::<Visibility>(sky).unwrap() = Visibility::Hidden;
        world.run_system_once(hd_sky).unwrap();
        assert_eq!(skybox(&world), None);
        *world.get_mut::<Visibility>(sky).unwrap() = Visibility::Inherited;
        world.run_system_once(hd_sky).unwrap();
        assert_eq!(skybox(&world), Some(handle));
        world.despawn(sky);
        world.run_system_once(hd_sky).unwrap();
        assert_eq!(skybox(&world), None, "the battle's sky gone, the menu camera draws none");
    }

    #[test]
    fn a_baked_sky_that_is_not_six_stacked_faces_is_not_drawn() {
        let mut world = World::new();
        world.init_resource::<Assets<Image>>();
        let handle = world.resource_mut::<Assets<Image>>().add(stacked(4, 20));
        let cam = world.spawn(Camera3d::default()).id();
        world.spawn((HdSky(handle), Visibility::Inherited));
        world.run_system_once(hd_sky).unwrap();
        assert!(world.get::<bevy::light::Skybox>(cam).is_none());
    }

    #[test]
    fn only_a_sky_with_a_baked_file_gets_one() {
        let root = std::env::temp_dir().join(format!("procyon-hd-sky-{}", std::process::id()));
        std::fs::create_dir_all(root.join("models/sky")).unwrap();
        assert_eq!(hd_sky_file(&root, "skybox_07"), None, "without the file the scenario keeps the retail sphere");
        std::fs::write(root.join("models/sky/skybox_07_cube.png"), b"").unwrap();
        assert_eq!(hd_sky_file(&root, "skybox_07").as_deref(), Some("models/sky/skybox_07_cube.png"));
        assert_eq!(hd_sky_file(&root, "skybox_17"), None);
        std::fs::remove_dir_all(root).unwrap();
    }
}
