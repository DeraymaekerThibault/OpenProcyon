//! Asteroids: scripted path motion, breakup on destruction and the demo field.
use super::*;

/// Ordinary asteroid state; authored strength, breakup and rule-driven physics (docs/re/asteroids.md).
#[derive(Component)]
#[require(crate::collision::Drift)]
pub struct Asteroid {
    pub kind: String,
    /// TWT group from the map export; also the rules' owner of the rock. `id`: TWT world object id (fragments keep their parent's).
    pub group: String,
    pub id: i64,
    pub radius: f32,
    pub mass: f32,
    pub hp: f32,
    pub velocity: Vec3,
    pub speed: f32,
    pub axis: Vec3,
    pub tumble: f32,
    pub path: Vec<Vec3>,
    pub next: usize,
    pub mode: PathMode,
    pub forward: bool, // EndToEnd direction
}

impl Asteroid {
    pub fn angular_velocity(&self) -> Vec3 {
        self.axis * self.tumble
    }
    pub fn add_contact_spin(&mut self, delta: f32) {
        let spin = self.angular_velocity() + Vec3::Y * delta;
        self.tumble = spin.length();
        self.axis = spin.try_normalize().unwrap_or(Vec3::X);
    }
    pub fn new(kind: &str, group: &str, defs: &crate::data::AsteroidDefs) -> Option<Self> {
        let d = defs.0.get(kind)?;
        Some(Self {
            kind: kind.into(),
            group: group.into(),
            id: -1,
            radius: d.radius,
            mass: d.mass,
            hp: d.hp,
            velocity: Vec3::ZERO,
            speed: 0.0,
            axis: Vec3::X,
            tumble: 0.0,
            path: vec![],
            next: 0,
            mode: PathMode::ToEnd,
            forward: true,
        })
    }
    pub fn damage(&mut self, amount: f32) -> bool {
        if self.hp <= 0.0 {
            return false;
        }
        self.hp = (self.hp - amount).max(0.0);
        self.hp == 0.0
    }
    pub fn unit(&self, e: Entity, tf: &Transform) -> crate::rules::Unit {
        crate::rules::Unit {
            e,
            id: self.id,
            name: self.kind.clone(),
            owner: self.group.clone(),
            owner_is_named_group: false,
            groups: vec![],
            pos: tf.translation,
            alive: self.hp > 0.0,
            visible: true,
            primary: false,
            damage: 0.0,
            flag: String::new(),
            attack: vec![],
        }
    }
}

#[derive(Message)]
pub struct AsteroidSetup {
    pub units: Vec<Entity>,
    pub speed: [f32; 2],
    pub tumble: [f32; 2],
    pub path: Vec<Vec3>,
    pub closest: bool,
    pub mode: PathMode,
}

/// Rule 'Follow Mode' (values: to end 0, end to end 1, loop 2, teleport loop 3).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum PathMode {
    #[default]
    ToEnd,
    EndToEnd,
    Loop,
    /// At the last point the object jumps to the first and carries on (an open conveyor path).
    Teleport,
}

impl PathMode {
    pub fn parse(s: &str) -> Self {
        match s {
            "end to end" => Self::EndToEnd,
            "loop" => Self::Loop,
            "teleport loop" => Self::Teleport,
            _ => Self::ToEnd,
        }
    }
}

pub fn asteroid_motion(
    time: Res<Time>,
    mut setups: MessageReader<AsteroidSetup>,
    mut rng: ResMut<Rng>,
    mut rocks: Query<(Entity, &mut Asteroid, &mut Transform, &mut crate::collision::Drift)>,
) {
    for setup in setups.read() {
        assert!(
            setup.speed.iter().chain(&setup.tumble).all(|v| v.is_finite() && *v >= 0.0) && setup.speed[0] <= setup.speed[1] && setup.tumble[0] <= setup.tumble[1],
            "Invalid asteroid setup intervals"
        );
        for &e in &setup.units {
            let Ok((_, mut rock, tf, mut drift)) = rocks.get_mut(e) else { continue };
            drift.0 = Vec3::ZERO; // A new rule-controlled velocity replaces previous contact motion.
            let speed = rng.range(setup.speed[0], setup.speed[1]);
            rock.velocity = rng.direction() * speed;
            rock.speed = speed;
            rock.axis = rng.direction();
            rock.tumble = rng.range(setup.tumble[0], setup.tumble[1]);
            rock.path = rock_path(&setup.path, tf.translation);
            rock.next = if setup.closest { closest_point(&rock.path, tf.translation) } else { 0 };
            rock.mode = setup.mode;
            rock.forward = true;
        }
    }
    let dt = time.delta_secs();
    for (_, mut rock, mut tf, _) in &mut rocks {
        if rock.hp <= 0.0 {
            continue;
        }
        if rock.path.get(rock.next).is_some() {
            let r = &mut *rock;
            let (velocity, stopped) = follow_path(&mut tf.translation, &r.path, &mut r.next, &mut r.forward, r.mode, r.speed, dt);
            r.velocity = velocity;
            if stopped {
                r.speed = 0.0;
            }
        } else {
            tf.translation += rock.velocity * dt;
        }
        // PROVISIONAL world-axis multiplication order; native axis-angle rate is recovered.
        tf.rotation = (Quat::from_axis_angle(rock.axis, rock.tumble * dt) * tf.rotation).normalize();
    }
}

/// The rock's own copy of a belt path (docs/re/asteroids.md): every point moved sideways by the rock's signed horizontal offset
/// from the path line nearest to it, and raised by the rock's height, so a belt keeps its cross-section instead of every rock
/// flying to the same points. Sideways is the horizontal normal: the segment's perpendicular at the ends, the corner bisector
/// inside. PROVISIONAL: the original's side test signs are inferred; a path of fewer than two points is left as is.
pub fn rock_path(path: &[Vec3], at: Vec3) -> Vec<Vec3> {
    if path.len() < 2 {
        return path.to_vec();
    }
    let flat = |v: Vec3| v.with_y(0.0);
    let last = path.len() - 1;
    let normal = |i: usize| {
        let d = flat(path[(i + 1).min(last)] - path[i.saturating_sub(1)]).normalize_or_zero();
        Vec3::new(-d.z, 0.0, d.x) // the path direction turned 90 degrees (to starboard of travel)
    };
    let near = (0..path.len()).min_by(|&a, &b| path[a].distance_squared(at).total_cmp(&path[b].distance_squared(at))).unwrap_or(0);
    let lateral = flat(at - path[near]).dot(normal(near));
    (0..path.len()).map(|i| path[i] + normal(i) * lateral + Vec3::Y * at.y).collect()
}

/// Find Closest Point: the nearest path point that lies within 90 degrees of the bearing to the path's last point, as seen from
/// the rock (so a rock never starts by flying back along the belt); point 0 when none does.
pub fn closest_point(path: &[Vec3], at: Vec3) -> usize {
    let Some(&end) = path.last() else { return 0 };
    let bearing = |p: Vec3| (p - at).with_y(0.0);
    let to_end = bearing(end);
    (0..path.len())
        .filter(|&i| bearing(path[i]).angle_between(to_end) < std::f32::consts::FRAC_PI_2)
        .min_by(|&a, &b| path[a].distance_squared(at).total_cmp(&path[b].distance_squared(at)))
        .unwrap_or(0)
}

/// Waypoint following shared by asteroid belts and meteor emitters. PROVISIONAL: straight legs replace the native
/// SpaceObjectAI path controller. Moves `pos` toward `path[*next]` by `speed * dt` and, on reaching it, applies
/// `mode` at the path end. Returns the velocity and whether the object has stopped for good (`to end` at the last point).
/// Callers must pass a non-empty path with `*next` in range.
pub fn follow_path(pos: &mut Vec3, path: &[Vec3], next: &mut usize, forward: &mut bool, mode: PathMode, speed: f32, dt: f32) -> (Vec3, bool) {
    let Some(&point) = path.get(*next) else { return (Vec3::ZERO, true) };
    let delta = point - *pos;
    let velocity = delta.normalize_or_zero() * speed;
    *pos += delta.normalize_or_zero() * (speed * dt).min(delta.length());
    if delta.length() > speed * dt {
        return (velocity, false);
    }
    let last = path.len() - 1;
    match (mode, *forward) {
        (_, true) if *next < last => *next += 1,
        (PathMode::Loop, true) => *next = 0,
        (PathMode::Teleport, true) => {
            *pos = path[0];
            *next = 1.min(last);
        }
        (PathMode::EndToEnd, true) => {
            *forward = false;
            *next = last.saturating_sub(1);
        }
        (PathMode::EndToEnd, false) if *next > 0 => *next -= 1,
        (PathMode::EndToEnd, false) => {
            *forward = true;
            *next = 1.min(last);
        }
        _ => return (Vec3::ZERO, true),
    }
    (velocity, false)
}

pub fn asteroid_deaths(
    mut commands: Commands,
    mut killed: MessageReader<Destroyed>,
    rocks: Query<(&Asteroid, &Transform, &crate::collision::Drift)>,
    defs: Res<crate::data::AsteroidDefs>,
    shapes: Res<crate::collision::Shapes>,
    assets: Res<AssetServer>,
    fxl: Res<FxLib>,
    mut rng: ResMut<Rng>,
    mut sfx: MessageWriter<Sfx>,
    mut target: ResMut<Target>,
) {
    for death in killed.read() {
        let Ok((parent, tf, drift)) = rocks.get(death.victim) else { continue };
        let def = &defs.0[&parent.kind];
        // PROVISIONAL: default effect scale and existing explosion pool until the native caller is fully traced.
        fx::spawn(&mut commands, &fxl, &def.effect, None, Transform::from_translation(tf.translation), Transform::from_translation(tf.translation), true);
        sfx.write(Sfx { name: "Explosion_Med01".into(), at: Some(tf.translation), vol: 1.0, max: 6000.0 });
        for (slot, kind) in def.children.iter().enumerate().filter(|(_, kind)| !kind.is_empty()) {
            let d = &defs.0[kind];
            let direction = [Vec3::X, Vec3::NEG_X, Vec3::NEG_Z, Vec3::Z, Vec3::Y, Vec3::NEG_Y, Vec3::ZERO, Vec3::ZERO, Vec3::ZERO, Vec3::ZERO][slot];
            // PROVISIONAL: interpret the native collision-size field as radius; fragment orientation starts at identity.
            let child_tf = Transform::from_translation(tf.translation + direction * d.radius * 1.1);
            // PROVISIONAL: children inherit the named rule group; native owner inheritance is confirmed, group insertion is not.
            let mut child = Asteroid::new(kind, &parent.group, &defs).unwrap();
            child.id = parent.id;
            // Native stationary-parent ejection range; moving-parent velocity inheritance remains provisional.
            child.velocity = parent.velocity + drift.0 + direction * (0.8 + rng.f() * d.mass / 60000.0 * 2.2);
            child.speed = child.velocity.length();
            child.axis = rng.direction();
            child.tumble = rng.f() * std::f32::consts::FRAC_PI_2;
            let mut entity = commands.spawn((crate::Scenario, child, child_tf, crate::data::scene(&assets, &format!("{}.glb", d.scene))));
            if let Some(body) = shapes.obstacle(kind, &child_tf) {
                entity.insert(body);
            }
        }
        if target.0 == Some(death.victim) {
            target.0 = None;
            target.1 = None;
        }
        commands.entity(death.victim).despawn();
    }
}

/// `--script asteroid`: frame a belt rock, then destroy it to smoke-test breakup and effects.
pub fn asteroid_demo(
    cfg: Res<crate::Config>,
    mut commands: Commands,
    mut orbit: ResMut<crate::Orbit>,
    player: Single<Entity, With<crate::PlayerShip>>,
    mut rocks: Query<(Entity, &mut Asteroid, &Transform)>,
    mut selected: Local<Option<Entity>>,
    mut killed: MessageWriter<Destroyed>,
) {
    if cfg.script.as_deref() != Some("asteroid") {
        return;
    }
    if cfg.frame == 90
        && let Some((e, _, tf)) = rocks.iter().find(|(_, a, _)| a.group == "Asteroid Belt" && a.kind == "Asteroid_Large")
    {
        *selected = Some(e);
        orbit.focus = Some(commands.spawn((crate::Scenario, *tf)).id());
        orbit.dist = 95.0;
        orbit.pitch = 0.35;
    }
    if cfg.frame == 180
        && let Some(e) = *selected
        && let Ok((_, mut rock, _)) = rocks.get_mut(e)
        && rock.damage(f32::MAX)
    {
        killed.write(Destroyed { by: *player, victim: e });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_rock_follows_the_belt_path_shifted_by_its_own_offset_and_height() {
        // A path along +x (Bevy: -z is forward, x is starboard); a rock 6 units to one side of it and 3 up.
        let path = [Vec3::ZERO, Vec3::X * 50.0, Vec3::X * 100.0];
        let own = rock_path(&path, Vec3::new(40.0, 3.0, 6.0));
        for (p, q) in path.iter().zip(&own) {
            assert!((*q - *p - Vec3::new(0.0, 3.0, 6.0)).length() < 1e-4, "{q} vs {p}");
        }
        // The other side shifts the other way: two rocks never share their points.
        assert!((rock_path(&path, Vec3::new(40.0, 0.0, -6.0))[1] - Vec3::new(50.0, 0.0, -6.0)).length() < 1e-4);
    }

    #[test]
    fn find_closest_point_skips_points_behind_the_rock() {
        let path = [Vec3::ZERO, Vec3::X * 10.0, Vec3::X * 20.0, Vec3::X * 30.0];
        // Nearest is point 1 (behind), but only points ahead toward the end count.
        assert_eq!(closest_point(&path, Vec3::new(13.0, 0.0, 0.0)), 2);
        assert_eq!(closest_point(&path, Vec3::new(-5.0, 0.0, 0.0)), 0);
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn asteroid_tumble_obeys_group_ranges_without_drift() {
        let mut app = App::new();
        app.init_resource::<Time>().init_resource::<AsteroidDefs>().insert_resource(Rng(123)).add_message::<AsteroidSetup>().add_systems(Update, asteroid_motion);
        let defs = app.world().resource::<AsteroidDefs>();
        let a = Asteroid::new("Asteroid_Large", "Asteroid Belt", defs).unwrap();
        let b = Asteroid::new("Asteroid_Med", "Montressor Asteroids", defs).unwrap();
        let a = app.world_mut().spawn((a, Transform::from_xyz(20.0, 0.0, 0.0))).id();
        let b = app.world_mut().spawn((b, Transform::default())).id();
        for (entity, upper) in [(a, 0.9), (b, 0.75)] {
            app.world_mut().write_message(AsteroidSetup { units: vec![entity], speed: [0.0, 0.0], tumble: [0.5, upper], path: vec![], closest: false, mode: PathMode::ToEnd });
        }
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.1));
        app.update();
        for (entity, upper) in [(a, 0.9), (b, 0.75)] {
            let rock = app.world().get::<Asteroid>(entity).unwrap();
            assert!((0.5..=upper).contains(&rock.tumble));
            assert!((rock.axis.length() - 1.0).abs() < 1e-5);
            let tf = app.world().get::<Transform>(entity).unwrap();
            assert!((tf.rotation.angle_between(Quat::IDENTITY) - rock.tumble * 0.1).abs() < 1e-4);
            assert_eq!(tf.translation, if entity == a { Vec3::new(20.0, 0.0, 0.0) } else { Vec3::ZERO });
        }
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn asteroid_path_tracks_velocity_and_stops_at_end() {
        let mut app = App::new();
        app.init_resource::<Time>().init_resource::<AsteroidDefs>().insert_resource(Rng(123)).add_message::<AsteroidSetup>().add_systems(Update, asteroid_motion);
        let rock = Asteroid::new("Asteroid_Med", "rocks", app.world().resource::<AsteroidDefs>()).unwrap();
        let e = app.world_mut().spawn((rock, Transform::default())).id();
        app.world_mut().write_message(AsteroidSetup { units: vec![e], speed: [2.0, 2.0], tumble: [0.0, 0.0], path: vec![Vec3::X], closest: true, mode: PathMode::ToEnd });
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.1));
        app.update();
        assert_eq!(app.world().get::<Asteroid>(e).unwrap().velocity, Vec3::X * 2.0);
        for _ in 0..10 {
            app.update();
        }
        assert_eq!(app.world().get::<Transform>(e).unwrap().translation, Vec3::X);
        assert_eq!(app.world().get::<Asteroid>(e).unwrap().velocity, Vec3::ZERO);
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn asteroid_path_modes_teleport_loop_and_ping_pong() {
        let run = |mode| {
            let mut app = App::new();
            app.init_resource::<Time>().init_resource::<AsteroidDefs>().insert_resource(Rng(1)).add_message::<AsteroidSetup>().add_systems(Update, asteroid_motion);
            let rock = Asteroid::new("Asteroid_Med", "rocks", app.world().resource::<AsteroidDefs>()).unwrap();
            let e = app.world_mut().spawn((rock, Transform::default())).id();
            let path = vec![Vec3::ZERO, Vec3::X * 10.0, Vec3::X * 20.0];
            app.world_mut().write_message(AsteroidSetup { units: vec![e], speed: [10.0, 10.0], tumble: [0.0, 0.0], path, closest: true, mode });
            let mut xs = vec![];
            for _ in 0..40 {
                app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.1));
                app.update();
                xs.push(app.world().get::<Transform>(e).unwrap().translation.x);
            }
            xs
        };
        let tele = run(PathMode::Teleport);
        assert!(tele.windows(2).any(|w| w[1] < w[0] - 15.0), "jumps back to the first point");
        assert!(tele.iter().all(|x| *x <= 20.001));
        let pong = run(PathMode::EndToEnd);
        assert!(pong.iter().any(|x| *x > 19.0) && pong.windows(2).any(|w| w[1] < w[0]) && pong.windows(2).all(|w| w[0] - w[1] < 1.5), "reverses smoothly");
        assert!(run(PathMode::ToEnd).iter().all(|x| *x <= 20.001));
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn asteroid_setup_replaces_contact_drift_without_erasing_it_each_tick() {
        let mut app = App::new();
        app.init_resource::<Time>().init_resource::<AsteroidDefs>().insert_resource(Rng(123)).add_message::<AsteroidSetup>().add_systems(Update, asteroid_motion);
        let rock = Asteroid::new("Asteroid_Small", "rocks", app.world().resource::<AsteroidDefs>()).unwrap();
        let e = app.world_mut().spawn((rock, Transform::default())).id();
        app.world_mut().get_mut::<crate::collision::Drift>(e).unwrap().0 = Vec3::X * 6.0;
        app.update();
        assert_eq!(app.world().get::<crate::collision::Drift>(e).unwrap().0, Vec3::X * 6.0);
        app.world_mut().write_message(AsteroidSetup { units: vec![e], speed: [0.0, 0.0], tumble: [0.0, 0.0], path: vec![], closest: false, mode: PathMode::ToEnd });
        app.update();
        assert_eq!(app.world().get::<crate::collision::Drift>(e).unwrap().0, Vec3::ZERO);
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn asteroid_breakup_is_once_only_recursive_and_cleans_target() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<WorldAsset>()
            .init_resource::<AsteroidDefs>()
            .init_resource::<crate::collision::Shapes>()
            .init_resource::<FxLib>()
            .init_resource::<Target>()
            .insert_resource(Rng(123))
            .add_message::<Destroyed>()
            .add_message::<Detonate>()
            .add_message::<Shoot>()
            .add_message::<Strike>()
            .add_message::<Sfx>()
            .add_systems(Update, asteroid_deaths);
        let shooter = app.world_mut().spawn_empty().id();
        let rock = Asteroid::new("Asteroid_Large", "Asteroid Belt", app.world().resource::<AsteroidDefs>()).unwrap();
        let parent = app.world_mut().spawn((rock, Transform::default())).id();
        app.world_mut().get_mut::<crate::collision::Drift>(parent).unwrap().0 = Vec3::X * 6.0;
        app.world_mut().resource_mut::<Target>().0 = Some(parent);
        let mut rock = app.world_mut().get_mut::<Asteroid>(parent).unwrap();
        assert!(!rock.damage(49.0));
        assert!(rock.damage(1.0));
        assert!(!rock.damage(1000.0), "multiple shells must not repeat death");
        app.world_mut().write_message(Destroyed { by: shooter, victim: parent });
        app.update();
        assert!(app.world().get_entity(parent).is_err());
        assert_eq!(app.world().resource::<Target>().0, None);
        let children: Vec<_> = app
            .world_mut()
            .query::<(Entity, &Asteroid, &Transform)>()
            .iter(app.world())
            .map(|(e, a, t)| {
                assert_eq!((a.kind.as_str(), a.group.as_str(), a.hp), ("Asteroid_Med", "Asteroid Belt", 30.0));
                assert!((0.8..=0.8 + 2.2 / 3.0).contains(&(a.velocity - Vec3::X * 6.0).length()), "fragments inherit the parent's contact velocity");
                assert!((0.0..=std::f32::consts::FRAC_PI_2).contains(&a.tumble));
                assert!((t.translation.length() - 6.6).abs() < 1e-4);
                assert!(app.world().get::<crate::collision::Obstacle>(e).is_some());
                e
            })
            .collect();
        assert_eq!(children.len(), 2);
        assert_eq!(app.world_mut().query::<&fx::Fx>().iter(app.world()).count(), 1);
        app.update();
        assert_eq!(app.world_mut().query::<&Asteroid>().iter(app.world()).count(), 2);
        for child in children {
            assert!(app.world_mut().get_mut::<Asteroid>(child).unwrap().damage(30.0));
            app.world_mut().write_message(Destroyed { by: shooter, victim: child });
        }
        app.update();
        let small: Vec<_> = app
            .world_mut()
            .query::<(Entity, &Asteroid)>()
            .iter(app.world())
            .map(|(e, a)| {
                assert_eq!(a.hp, 10.0);
                e
            })
            .collect();
        assert_eq!(small.len(), 6);
        for child in small {
            assert!(app.world_mut().get_mut::<Asteroid>(child).unwrap().damage(10.0));
            app.world_mut().write_message(Destroyed { by: shooter, victim: child });
        }
        app.update();
        assert_eq!(app.world_mut().query::<&Asteroid>().iter(app.world()).count(), 0);
        assert_eq!(app.world_mut().query::<&crate::collision::Obstacle>().iter(app.world()).count(), 0);
    }
}
