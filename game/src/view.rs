//! Camera, world mouse/keyboard input and the F1 debug text.

use crate::Nearest;
use crate::bindings::Act;
use crate::*;

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
}

fn camera_input(mouse: &ButtonInput<MouseButton>, keys: &ButtonInput<KeyCode>, pointer: &Pointer, orbit: &mut Orbit, dt: f32) {
    // Camera controls use real time, independent of simulation speed or pause.
    if mouse.pressed(MouseButton::Right) {
        orbit.yaw -= pointer.motion.delta.x * 0.005;
        orbit.pitch = (orbit.pitch + pointer.motion.delta.y * 0.005).clamp(0.05, 1.5);
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

pub fn input(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut pointer: Pointer,
    window: Single<&Window, With<PrimaryWindow>>,
    cam: Single<(&Camera, &GlobalTransform)>,
    mut orbit: ResMut<Orbit>,
    ships: Query<(Entity, &Ship, &GlobalTransform, &Health, Option<&campaign::MissionShip>, Option<&ship::FleetPilot>)>,
    time: Res<Time<Real>>,
    lib: Res<Library>,
    mut target: ResMut<Target>,
    mut vtime: ResMut<Time<Virtual>>,
    ui: Res<crate::panels::UiClick>,
    mut docks: docking::Dockees,
    (sensors, mission): (Res<environment::Sensors>, Option<Res<campaign::Mission>>),
    objects: Query<(Entity, &Transform, Option<&combat::Asteroid>, Option<&environment::SpaceAnimal>), Or<(With<combat::Asteroid>, With<environment::SpaceAnimal>)>>,
    mut commands: Commands,
) {
    let Some(actor) = pointer.fleets.selected else { return };
    let direct = pointer.fleets.direct();
    let others: Vec<_> = if mouse.just_pressed(MouseButton::Left) && !ui.0 {
        ships.iter().map(|(e, s, tf, h, m, _)| (e, tf.compute_transform(), s.clone(), h.destroyed, m.is_none_or(|m| m.visible))).collect()
    } else {
        vec![]
    };
    let Ok((_, source, player_tf, _, _, source_pilot)) = ships.get(actor) else { return };
    let mut ship = source.clone();
    let mut pilot = source_pilot.cloned();
    (|| {
        if briefing::active(mission.as_deref()) || rules::locked(mission.as_deref()) {
            return;
        }
        camera_input(&mouse, &keys, &pointer, &mut orbit, time.delta_secs());
        let b = &pointer.binds;

        // Engine orders: +/- step through All Stop .. Emergency.
        if direct && (b.just_pressed(&keys, Act::SpeedUp) || keys.just_pressed(KeyCode::NumpadAdd)) {
            ship.throttle = None;
            ship.order = (ship.order + 1).min(ORDERS.len() - 1);
        }
        if direct && (b.just_pressed(&keys, Act::SpeedDown) || keys.just_pressed(KeyCode::NumpadSubtract)) {
            ship.throttle = None;
            ship.order = ship.order.saturating_sub(1);
        }
        // Advanced movement: Enter accepts the plotted course, Backspace drops it (and nothing else).
        if direct && keys.just_pressed(KeyCode::Enter) && !pointer.plot.pending.is_empty() {
            commit_plot(&mut ship, &mut pointer.plot);
        }
        if keys.just_pressed(KeyCode::Backspace) && !pointer.plot.pending.is_empty() {
            pointer.plot.pending.clear();
        } else if keys.just_pressed(KeyCode::Backspace) {
            commands.entity(actor).remove::<boarding::GrappleOrder>();
            ship.waypoints.clear();
            if direct {
                target.2.armed = None;
                target.2.shot = None;
            } else {
                if let Some(p) = pilot.as_mut() {
                    p.destination = None;
                    p.target = None;
                    p.hold = false;
                }
                ship.order = 0;
            }
        }
        if direct && keys.just_pressed(KeyCode::KeyC) {
            target.0 = None;
            target.1 = None;
            target.2.panic = false;
        }
        if keys.just_pressed(KeyCode::Pause) {
            toggle_pause(&mut vtime);
        }

        // Left click: an enemy ship selects it as target, otherwise set a destination on the plane (Shift appends).
        if mouse.just_pressed(MouseButton::Left) && !ui.0 && !docks.menu_swallows_click() {
            let Some(cursor) = window.cursor_position() else { return };
            let (camera, gt) = *cam;
            let Ok(ray) = camera.viewport_to_world(gt, cursor) else { return };
            // An armed indirect bank takes this click as its shot target.
            let own = others
                .iter()
                .filter(|(e, _, _, h, m)| pointer.fleets.roster().contains(e) && !*h && *m)
                .filter_map(|(e, tf, s, _, _)| combat::ray_hit(&lib.classes[s.class], tf, ray).map(|t| (*e, t)))
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((e, _)) = own {
                pointer.fleets.select(e, &mut orbit);
                return;
            }
            if let Some(bank) = direct.then(|| target.2.armed.take()).flatten() {
                if let Some(t) = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y)) {
                    target.2.shot = Some((bank, ray.get_point(t)));
                }
                return;
            }
            // Attack Only Mode (held key): a click only picks targets, never a dock or a destination.
            let attack_only = pointer.binds.pressed(&keys, Act::AttackOnly);
            // An allied ship or base with dock points: left-click docks when it is dockable and in sight (the cursor shows whether it can).
            if let Some((e, ok)) = docks.under(ray, actor).filter(|_| !attack_only) {
                if ok {
                    docks.dock_as(actor, e);
                }
                return;
            }
            let hit = others
                .iter()
                .filter(|(e, _, s, h, m)| s.side != Side::Player && !*h && *m && sensors.player_sees(*e, mission.as_deref()))
                .filter_map(|(e, tf, s, _, _)| combat::ray_hit(&lib.classes[s.class], tf, ray).map(|t| (*e, t)))
                .min_by(|a, b| a.1.total_cmp(&b.1));
            let object = objects
                .iter()
                .filter_map(|(e, tf, rock, animal)| {
                    if !sensors.player_sees(e, mission.as_deref()) {
                        return None;
                    }
                    let radius = if let Some(a) = animal {
                        if !a.visible || !mission.as_deref().is_some_and(|m| m.script.hostile(m.script.player(), &a.owner)) {
                            return None;
                        }
                        a.radius
                    } else {
                        rock?.radius
                    };
                    combat::seg_sphere(ray.origin, ray.origin + *ray.direction * 3000.0, tf.translation, radius).map(|t| (e, t * 3000.0))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1));
            let base = docks.hostile_base_hit(ray, player_tf.translation());
            let pick = [hit, object, base.map(|(e, t, _)| (e, t))].into_iter().flatten().min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((e, t)) = pick {
                if !direct {
                    if let Some(p) = pilot.as_mut() {
                        p.attack(e);
                    }
                    return;
                }
                target.0 = Some(e);
                target.2.panic = false;
                // Remember the clicked spot on the hull (ship-local) so the guns aim there.
                let at = ray.origin + *ray.direction * t;
                target.1 = others.iter().find(|o| o.0 == e).map(|o| o.1.to_matrix().inverse().transform_point3(at)).or_else(|| base.filter(|b| b.0 == e).map(|b| b.2));
                return;
            }
            if attack_only {
                return;
            }
            let Some(t) = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y)) else { return };
            let p = ray.get_point(t);
            if direct && pointer.plot.advanced {
                match pointer.plot.pressed(p) {
                    Some(true) => {
                        docks.cancel_as(actor);
                        commit_plot(&mut ship, &mut pointer.plot);
                    }
                    Some(false) => pointer.plot.pending.clear(),
                    None => pointer.plot.pending.push(p),
                }
                return;
            }
            // A move order cancels a dock routine in progress and gives control back (original behaviour).
            docks.cancel_as(actor);
            commands.entity(actor).remove::<ship::HoldPosition>();
            if !direct {
                if let Some(pilot) = pilot.as_mut() {
                    pilot.move_to(p);
                }
                return;
            }
            if !keys.pressed(KeyCode::ShiftLeft) {
                ship.waypoints.clear();
            }
            ship.waypoints.push(p);
            if ship.order == 0 || ship.throttle == Some(0.0) {
                ship.throttle = None;
                ship.order = 2;
            } // light green 'Full', as in the original
        }
    })();
    if ship != *source {
        commands.entity(actor).insert(ship);
    }
    if pilot.as_ref() != source_pilot
        && let Some(pilot) = pilot
    {
        commands.entity(actor).insert(pilot);
    }
}

pub fn fleet_keys(
    keys: Res<ButtonInput<KeyCode>>,
    binds: Res<bindings::Bindings>,
    mut fleets: ResMut<ship::Fleets>,
    mut orbit: ResMut<Orbit>,
    mission: Option<Res<campaign::Mission>>,
    mut commands: Commands,
    mut sfx: MessageWriter<audio::Sfx>,
    maps: Query<(), With<crate::starmap::Starmap>>,
    menu: Option<Res<options_menu::OptionsMenu>>,
) {
    if menu.as_ref().is_some_and(|m| m.is_open()) || !maps.is_empty() || panels::cinematic(mission.as_deref()) || rules::locked(mission.as_deref()) {
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
        commands.queue(move |w: &mut World| {
            ship::promote(w, e, false);
        });
    }
}

/// The plotted course becomes the ship's (light-green Full if it was stopped, as a basic click does).
fn commit_plot(ship: &mut Ship, plot: &mut course::Plot) {
    ship.waypoints = std::mem::take(&mut plot.pending);
    if ship.order == 0 || ship.throttle == Some(0.0) {
        ship.throttle = None;
        ship.order = 2;
    }
}

/// Retail 'Take ScreenShot' (Scroll Lock): a PNG in the user folder's `screenshots/`.
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

pub fn sky(cam: Single<&Transform, (With<Camera3d>, Without<Sky>)>, mut sky: Single<(&mut Transform, &mut Visibility), With<Sky>>, settings: Option<Res<menu::Settings>>) {
    sky.0.translation = cam.translation;
    // Display Options 'Render Background'.
    let want = if settings.is_some_and(|s| !s.background) { Visibility::Hidden } else { Visibility::Inherited };
    sky.1.set_if_neq(want);
}

pub fn camera(
    orbit: Res<Orbit>,
    ship: Query<&Transform, (With<PlayerShip>, Without<Camera3d>)>,
    tfs: Query<&Transform, Without<Camera3d>>,
    contacts: Query<(), Or<(With<Ship>, With<environment::SpaceAnimal>)>>,
    sensors: Res<environment::Sensors>,
    mut cam: Single<&mut Transform, With<Camera3d>>,
    mission: Option<Res<campaign::Mission>>,
) {
    if briefing::active(mission.as_deref()) || rules::nis(mission.as_deref()) {
        return;
    }
    // `--look x,y[,z]` (game axes) aims screenshots at a world point instead of the player's ship.
    static LOOK: std::sync::OnceLock<Option<Vec3>> = std::sync::OnceLock::new();
    let look = LOOK.get_or_init(|| {
        let a: Vec<String> = std::env::args().collect();
        let v: Vec<f32> = a.windows(2).find(|w| w[0] == "--look")?.get(1)?.split(',').filter_map(|x| x.parse().ok()).collect();
        Some(Vec3::new(*v.first()?, *v.get(2).unwrap_or(&0.0), -*v.get(1)?))
    });
    let tracked = orbit.focus.filter(|e| !contacts.contains(*e) || sensors.player_sees(*e, mission.as_deref()));
    let focus = look.unwrap_or_else(|| tracked.and_then(|e| tfs.get(e).ok()).or_else(|| ship.iter().next()).map_or(Vec3::ZERO, |t| t.translation)) + Vec3::Y * 3.0;
    let off = Quat::from_rotation_y(orbit.yaw) * Quat::from_rotation_x(-orbit.pitch) * Vec3::new(0.0, 0.0, orbit.dist);
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
        ORDERS[ship.order].0,
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
        let cam = world.spawn((Camera3d::default(), Transform::default())).id();
        world.resource_mut::<Orbit>().focus = Some(ids[5]);
        world.get_mut::<Transform>(ids[5]).unwrap().translation = Vec3::X * 3000.0;
        world.run_system_once(environment::refresh_sensors).unwrap();
        world.run_system_once(camera).unwrap();
        assert_eq!(world.get::<Transform>(cam).unwrap().translation, Vec3::new(0.0, 3.0, 60.0));
        world.get_mut::<Transform>(ids[5]).unwrap().translation = Vec3::X * 120.0;
        world.run_system_once(environment::refresh_sensors).unwrap();
        world.run_system_once(camera).unwrap();
        assert_eq!(world.get::<Transform>(cam).unwrap().translation, Vec3::new(120.0, 3.0, 60.0));
        let shell = world.spawn(Transform::from_xyz(600.0, 0.0, 0.0)).id();
        world.resource_mut::<Orbit>().focus = Some(shell);
        world.run_system_once(camera).unwrap();
        assert_eq!(world.get::<Transform>(cam).unwrap().translation, Vec3::new(600.0, 3.0, 60.0));
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn keyboard_camera_rotation_and_zoom_stay_at_one_x_at_every_simulation_speed() {
        let (mut world, ids) = ship::movement_tests::fleet_world();
        for e in ids {
            world.entity_mut(e).insert(GlobalTransform::default());
        }
        world.resource_mut::<ship::Fleets>().selected = Some(ids[0]);
        world.spawn((Window::default(), PrimaryWindow));
        world.spawn((Camera::default(), GlobalTransform::default()));
        world.init_resource::<ButtonInput<MouseButton>>();
        world.init_resource::<ButtonInput<KeyCode>>();
        world.init_resource::<AccumulatedMouseMotion>();
        world.init_resource::<AccumulatedMouseScroll>();
        world.init_resource::<crate::bindings::Bindings>();
        world.init_resource::<course::Plot>();
        world.init_resource::<panels::UiClick>();
        world.init_resource::<options_menu::OptionsMenu>();
        world.init_resource::<environment::Sensors>();
        world.init_resource::<Messages<docking::DockMsg>>();
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
