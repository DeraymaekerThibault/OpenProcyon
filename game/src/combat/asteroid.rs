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
            let speed = setup.speed[0] + rng.f() * (setup.speed[1] - setup.speed[0]);
            rock.velocity = rng.direction() * speed;
            rock.speed = speed;
            rock.axis = rng.direction();
            rock.tumble = setup.tumble[0] + rng.f() * (setup.tumble[1] - setup.tumble[0]);
            rock.path = setup.path.clone();
            rock.next = if setup.closest {
                rock.path.iter().enumerate().min_by(|a, b| a.1.distance_squared(tf.translation).total_cmp(&b.1.distance_squared(tf.translation))).map_or(0, |p| p.0)
            } else {
                0
            };
            rock.mode = setup.mode;
            rock.forward = true;
        }
    }
    let dt = time.delta_secs().min(crate::MAX_SIM_STEP);
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
            let mut entity = commands.spawn((
                crate::Scenario,
                child,
                child_tf,
                WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(crate::data::model(&format!("{}.glb", d.scene))))),
            ));
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
