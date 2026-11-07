//! Beam guns: not in the original, a mod feature (`beam` block of a gun in `guns.json`, `data::BeamDef`). A beam gun charges for
//! `charge` seconds (a glow grows at the muzzle), then holds a beam along its barrel for `duration` seconds and reloads. The beam
//! burns the first enemy ship it crosses every `tick` seconds with the gun's bullet damage, times a power that grows while it stays on
//! the same ship, so a captain who keeps the target in the beam (by turning the ship: the mount's arc is narrow) does the most harm.
//! PROVISIONAL (design, no retail reference): the beam passes through bases, rocks and scenery, and hits only ships.
use super::*;
use crate::combat::shells::{ShipTarget, ShipTargets, Surface, hit_fx, hit_ship, nearest_ship_hit};

/// Where a beam gun is in its cycle.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub enum Phase {
    #[default]
    Idle,
    Charging {
        t: f32,
    },
    Firing {
        t: f32,
        /// The ship the beam is on and for how long it has been on it (the power ramp).
        victim: Option<Entity>,
        on: f32,
        /// Seconds to the next damage tick.
        tick: f32,
        /// Beam length this tick: to the struck hull, else the gun's maximum range.
        length: f32,
    },
}

/// What a step of the cycle started.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Step {
    None,
    /// The charge began: play the gun's sound.
    Charge,
    /// The beam lit: muzzle flash.
    Fire,
    /// The beam ended: the gun reloads.
    Done,
}

/// Advances the cycle by `dt`. `ready`: loaded, aligned and on a target in range and arc; only an idle gun needs it, a charge or a
/// beam once started runs its course (the beam stays along the barrel, wherever the target goes).
pub fn advance(phase: &mut Phase, def: &BeamDef, dt: f32, ready: bool, range: f32) -> Step {
    match phase {
        Phase::Idle if ready => {
            *phase = Phase::Charging { t: 0.0 };
            Step::Charge
        }
        Phase::Idle => Step::None,
        Phase::Charging { t } => {
            *t += dt;
            if *t >= def.charge {
                *phase = Phase::Firing { t: 0.0, victim: None, on: 0.0, tick: 0.0, length: range };
                Step::Fire
            } else {
                Step::None
            }
        }
        Phase::Firing { t, .. } => {
            *t += dt;
            if *t >= def.duration {
                *phase = Phase::Idle;
                Step::Done
            } else {
                Step::None
            }
        }
    }
}

/// The beam's origin (the gun's first muzzle) and direction in the world, from the turret's current yaw and pitch.
pub fn muzzle_ray(tf: &Transform, mount: &Mount, gun: &GunDef, yaw: f32, pitch: f32) -> (Vec3, Vec3) {
    let base = tf.rotation * mount.rot * Quat::from_rotation_y(yaw);
    let barrel = base * Quat::from_rotation_x(pitch);
    let muzzle = gun.muzzles.first().copied().unwrap_or(gun.pivot);
    (tf.transform_point(mount.pos) + base * gun.pivot + barrel * (muzzle - gun.pivot), barrel * Vec3::NEG_Z)
}

/// The beam of one firing gun this tick: which ship it is on (and how long), whether a damage tick is due, and the beam's length.
/// `hit` is the struck ship and the distance to it. Returns true when a tick of damage lands now.
pub fn track(phase: &mut Phase, def: &BeamDef, dt: f32, hit: Option<(Entity, f32)>, range: f32) -> bool {
    let Phase::Firing { victim, on, tick, length, .. } = phase else { return false };
    match hit {
        Some((e, distance)) => {
            if *victim == Some(e) {
                *on += dt;
            } else {
                // A new ship: the power starts over and the first touch burns at once.
                (*victim, *on, *tick) = (Some(e), 0.0, 0.0);
            }
            *length = distance;
            *tick -= dt;
            if *tick <= 0.0 {
                *tick += def.tick;
                return true;
            }
            false
        }
        None => {
            (*victim, *on, *length) = (None, 0.0, range);
            false
        }
    }
}

/// Burns what every firing beam touches (after `fire`, which runs the cycle and turns the turrets).
pub(super) fn burn(
    mut commands: Commands,
    time: Res<Time>,
    lib: Res<Library>,
    fxl: Res<FxLib>,
    mut rng: ResMut<Rng>,
    (mut sfx, mut hits, mut scars): (MessageWriter<Sfx>, MessageWriter<Hit>, MessageWriter<crate::decals::Scar>),
    mut shooters: Query<(Entity, &Transform, &Ship, &mut GunStates)>,
    mut ships: ShipTargets,
) {
    let dt = time.delta_secs();
    for (me, tf, ship, mut states) in &mut shooters {
        let class = &lib.classes[ship.class];
        let sunk = ships.get(me).is_ok_and(|s| s.3.destroyed);
        for (mount, state) in class.mounts.iter().zip(states.0.iter_mut()) {
            let gun = &lib.guns[&state.gun];
            let Some(def) = &gun.beam else { continue };
            if sunk {
                state.beam = Phase::Idle;
                continue;
            }
            if !matches!(state.beam, Phase::Firing { .. }) {
                continue;
            }
            let (from, dir) = muzzle_ray(tf, mount, gun, state.yaw, state.pitch);
            let range = gun.range[2];
            let found = nearest_ship_hit(&ships, &lib, ship.side, from, from + dir * range);
            let victim = victim_power(&state.beam, def, dt, found.map(|f| f.0));
            if !track(&mut state.beam, def, dt, found.map(|(e, _, _, at)| (e, from.distance(at))), range) {
                continue;
            }
            let (Some((se, _, k, at)), Some(bd)) = (found, lib.bullets.get(&gun.bullet)) else { continue };
            let Ok((se, stf, victim_ship, mut hp, mut fires, mission, _)) = ships.get_mut(se) else { continue };
            let shell = Shell {
                vel: dir * gun.muzzle.max(1.0),
                age: 0.0,
                lifetime: 0.0,
                gravity: 0.0,
                bullet: gun.bullet.clone(),
                side: ship.side,
                shooter: me,
                target: Some(se),
                angular_velocity: 0.0,
                payload: bd.payload,
                aim: None,
                pd: false,
                hp: 0.0,
                harmless: false,
                base: false,
                ricochet: 0.0,
            };
            let target = ShipTarget { entity: se, transform: stf, side: victim_ship.side, class: &lib.classes[victim_ship.class], invuln: mission.map_or(0.0, |m| m.invuln) };
            hit_ship(&mut commands, &fxl, &mut rng, (&mut sfx, &mut hits, &mut scars), bd, &shell, target, &mut hp, &mut fires, k, at, victim);
            hit_fx(&mut commands, &fxl, bd, at, shell.vel, Some((se, stf)), Surface { body: Some(se), normal: None });
        }
    }
}

/// The power of the tick about to land: on the same ship as before, the ramp so far plus this step; on a new ship, 1.
fn victim_power(phase: &Phase, def: &BeamDef, dt: f32, hit: Option<Entity>) -> f32 {
    match phase {
        Phase::Firing { victim, on, .. } if hit.is_some() && *victim == hit => def.power(on + dt),
        _ => def.power(0.0),
    }
}

/// Meshes the beam is drawn with; the materials are made per gun (each fades on its own).
#[derive(Resource)]
pub(super) struct BeamMeshes {
    sphere: Handle<Mesh>,
    cylinder: Handle<Mesh>,
    ring: Handle<Mesh>,
}

pub(super) fn setup_look(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    commands.insert_resource(BeamMeshes { sphere: meshes.add(Sphere::new(1.0)), cylinder: meshes.add(Cylinder::new(1.0, 1.0)), ring: meshes.add(Torus::new(0.85, 1.0)) });
}

/// The drawn parts of one active beam gun (children of its muzzle): glow, ring, core, halo; their materials.
pub(super) struct Drawn {
    parts: [Entity; 4],
    mats: [Handle<StandardMaterial>; 4],
}

/// Draws every charging or firing beam gun, from its phase: while charging a glow swells at the muzzle (with a light and a
/// tightening ring), then a bright core inside a softer halo runs to `length`, flickering, and thins out over its last 0.3 s.
pub(super) fn draw(
    mut commands: Commands,
    time: Res<Time>,
    lib: Res<Library>,
    meshes: Option<Res<BeamMeshes>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    guns: Query<(Entity, &GunStates)>,
    mut parts: Query<(&mut Transform, &mut Visibility), Without<GunStates>>,
    mut lights: Query<&mut PointLight>,
    mut drawn: Local<bevy::platform::collections::HashMap<(Entity, usize), Drawn>>,
) {
    let Some(meshes) = meshes else { return };
    let now = time.elapsed_secs();
    let mut live = vec![];
    for (ship, states) in &guns {
        for (i, state) in states.0.iter().enumerate() {
            let Some(def) = lib.guns.get(&state.gun).and_then(|g| g.beam.as_ref()) else { continue };
            if state.beam == Phase::Idle {
                continue;
            }
            let Some(&muzzle) = state.muzzles.first() else { continue };
            live.push((ship, i));
            let d = drawn.entry((ship, i)).or_insert_with(|| {
                let c = def.color;
                let mat = |gain: f32, mats: &mut Assets<StandardMaterial>| {
                    mats.add(StandardMaterial {
                        base_color: Color::LinearRgba(LinearRgba::rgb(c[0] * gain, c[1] * gain, c[2] * gain)),
                        unlit: true,
                        alpha_mode: AlphaMode::Add,
                        ..default()
                    })
                };
                let mats_ = [mat(1.5, &mut mats), mat(1.0, &mut mats), mat(4.0, &mut mats), mat(0.6, &mut mats)];
                let mesh = [&meshes.sphere, &meshes.ring, &meshes.cylinder, &meshes.cylinder];
                let parts: [Entity; 4] = std::array::from_fn(|k| {
                    let mut e = commands.spawn((Mesh3d(mesh[k].clone()), MeshMaterial3d(mats_[k].clone()), Transform::from_scale(Vec3::ZERO), Visibility::Hidden, ChildOf(muzzle)));
                    if k == 0 {
                        e.insert(PointLight { color: Color::linear_rgb(c[0], c[1], c[2]), intensity: 0.0, range: 40.0, ..default() });
                    }
                    e.id()
                });
                Drawn { parts, mats: mats_ }
            });
            let looks = look(&state.beam, def, now);
            for (k, part) in looks.parts.iter().enumerate() {
                if let Ok((mut tf, mut vis)) = parts.get_mut(d.parts[k]) {
                    *vis = if part.is_some() { Visibility::Inherited } else { Visibility::Hidden };
                    if let Some(p) = part {
                        *tf = *p;
                    }
                }
            }
            if let Ok(mut light) = lights.get_mut(d.parts[0]) {
                light.intensity = looks.light;
            }
        }
    }
    drawn.retain(|key, d| {
        let keep = live.contains(key);
        if !keep {
            for (e, m) in d.parts.iter().zip(&d.mats) {
                commands.entity(*e).try_despawn();
                mats.remove(m);
            }
        }
        keep
    });
}

/// The pose of each drawn part in the muzzle's frame (barrel along -Z), `None` = hidden, and the glow's light (lumens).
pub struct Looks {
    pub parts: [Option<Transform>; 4],
    pub light: f32,
}

/// What a beam gun in `phase` looks like at time `now` (seconds, for the flicker): glow, ring, core, halo.
pub fn look(phase: &Phase, def: &BeamDef, now: f32) -> Looks {
    let w = def.width;
    let flicker = 1.0 + 0.12 * (now * 47.0).sin() + 0.08 * (now * 113.0).sin();
    let along = |length: f32, radius: f32| Transform {
        translation: Vec3::new(0.0, 0.0, -length / 2.0),
        rotation: Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
        scale: Vec3::new(radius, length, radius),
    };
    match *phase {
        Phase::Idle => Looks { parts: [None; 4], light: 0.0 },
        Phase::Charging { t } => {
            let f = (t / def.charge.max(0.01)).clamp(0.0, 1.0);
            // The glow swells with the charge; the ring closes in on it and spins faster.
            let glow = Transform::from_scale(Vec3::splat(w * (0.3 + 1.7 * f * f) * flicker));
            let ring = Transform { rotation: Quat::from_rotation_z(now * (2.0 + 14.0 * f)), scale: Vec3::splat(w * (5.0 - 3.0 * f)), ..default() }
                * Transform::from_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2));
            Looks { parts: [Some(glow), Some(ring), None, None], light: 300_000.0 * f * f }
        }
        Phase::Firing { t, length, .. } => {
            // Full width at once, thinning out over the last 0.3 s.
            let fade = ((def.duration - t) / 0.3).clamp(0.0, 1.0);
            let r = w * fade * flicker;
            let glow = Transform::from_scale(Vec3::splat(w * 1.6 * fade.max(0.2) * flicker));
            Looks { parts: [Some(glow), None, Some(along(length, r * 0.35)), Some(along(length, r))], light: 500_000.0 * fade }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def() -> BeamDef {
        BeamDef::from_json(&serde_json::json!({"charge": 2.0, "duration": 3.0, "tick": 0.25, "ramp": 0.6, "max_power": 2.0})).unwrap()
    }

    #[test]
    fn a_beam_charges_then_holds_for_its_duration_then_reloads() {
        let (d, mut p) = (def(), Phase::Idle);
        assert_eq!(advance(&mut p, &d, 0.1, false, 300.0), Step::None, "an idle gun waits for a target");
        assert_eq!(advance(&mut p, &d, 0.1, true, 300.0), Step::Charge);
        let mut steps = vec![];
        for _ in 0..60 {
            // the target may be lost: a started charge and beam run their course
            steps.push(advance(&mut p, &d, 0.1, false, 300.0));
        }
        let at = |s: Step| steps.iter().position(|x| *x == s).unwrap();
        assert!((19..=20).contains(&at(Step::Fire)), "2 s of charge: {}", at(Step::Fire));
        assert!((at(Step::Done) - at(Step::Fire)).abs_diff(30) <= 1, "3 s of beam");
        assert_eq!(p, Phase::Idle);
    }

    #[test]
    fn damage_ticks_ramp_while_the_beam_stays_on_one_ship_and_start_over_on_another() {
        let d = def();
        let (a, b) = (Entity::from_raw_u32(1).unwrap(), Entity::from_raw_u32(2).unwrap());
        let mut p = Phase::Firing { t: 0.0, victim: None, on: 0.0, tick: 0.0, length: 300.0 };
        assert!(track(&mut p, &d, 0.05, Some((a, 50.0)), 300.0), "the first touch burns at once");
        let ticks = (0..19).filter(|_| track(&mut p, &d, 0.05, Some((a, 50.0)), 300.0)).count();
        assert_eq!(ticks, 3, "one tick per 0.25 s: at 0.25, 0.5 and 0.75 s after the first");
        let Phase::Firing { on, length, .. } = p else { panic!() };
        assert!((on - 0.95).abs() < 1e-4 && length == 50.0);
        assert!((victim_power(&p, &d, 0.05, Some(a)) - 1.6).abs() < 1e-4, "1 + 0.6 x 1 s");
        assert_eq!(victim_power(&p, &d, 0.05, Some(b)), 1.0, "another ship starts at 1");
        assert_eq!(d.power(10.0), 2.0, "capped");
        assert!(!track(&mut p, &d, 0.05, None, 300.0));
        assert!(matches!(p, Phase::Firing { victim: None, on: 0.0, length: 300.0, .. }), "a miss runs to full range and resets the ramp");
    }

    #[test]
    fn the_beam_leaves_the_muzzle_along_the_barrel() {
        let gun = crate::data::gun_def(&serde_json::json!({
            "MuzzleSpeed": 1, "TimeBetweenBursts": 1, "BulletsPerBurst": 1, "DuringBurstReloadTime": 0, "BulletAffectedByGravity": false,
            "BulletTypeName_Primary": "B", "HorizontalRotationSpeed": 1, "VerticalRotationSpeed": 1, "VerticalMinAngle": -1, "VerticalMaxAngle": 1,
            "EffectiveRange_Range": 1, "LongRange_Range": 1, "MaximumRange_Range": 1, "EffectiveRange_AccuracyDeviation": 0,
            "LongRange_AccuracyDeviation": 0, "MaximumRange_AccuracyDeviation": 0,
            "visual": {"pivot": [0, 0, 0], "muzzles": [[0, 4, 0]]}}));
        let mount = Mount { pos: Vec3::new(0.0, 1.0, -5.0), rot: Quat::IDENTITY, arc: 0.3, gun: "G".into(), bank: 0, parent: None };
        let (from, dir) = muzzle_ray(&Transform::default(), &mount, &gun, 0.0, 0.0);
        assert!(from.distance(Vec3::new(0.0, 1.0, -9.0)) < 1e-4, "{from}");
        assert!(dir.distance(Vec3::NEG_Z) < 1e-5);
        let (_, dir) = muzzle_ray(&Transform::from_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)), &mount, &gun, 0.1, 0.0);
        assert!(dir.x < -0.99, "the ship's heading and the turret's yaw turn the beam: {dir}");
    }

    #[test]
    fn the_look_grows_while_charging_and_shows_the_beam_only_when_firing() {
        let d = def();
        let small = look(&Phase::Charging { t: 0.2 }, &d, 0.0);
        let big = look(&Phase::Charging { t: 1.9 }, &d, 0.0);
        assert!(small.parts[0].unwrap().scale.x < big.parts[0].unwrap().scale.x && small.light < big.light);
        assert!(big.parts[2].is_none() && big.parts[1].is_some(), "no beam yet, the ring is up");
        let firing = look(&Phase::Firing { t: 1.0, victim: None, on: 0.0, tick: 0.0, length: 80.0 }, &d, 0.0);
        let core = firing.parts[2].unwrap();
        assert!((core.scale.y - 80.0).abs() < 1e-4 && (core.translation.z + 40.0).abs() < 1e-4, "runs 80 units down the barrel");
        assert!(look(&Phase::Idle, &d, 0.0).parts.iter().all(Option::is_none));
    }
}
