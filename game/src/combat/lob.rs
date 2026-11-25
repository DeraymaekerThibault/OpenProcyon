//! Area weapons: lob-shell detonations (star/nova mortars, gravity charges), the blast spheres and gravity wells they leave,
//! mission 9's dark-matter blast, and hull damage from hard collisions (rams).
//! Data: the bullets' custom-info fields (`Payload`, exported by tools/export_combat.py). PROVISIONAL behaviour is marked below.
use super::shells::Surface;
use super::*;

/// A bullet bursts at `at` (a landed lob shell, or a scripted blast with no shooter).
#[derive(Message)]
pub struct Detonate {
    pub bullet: String,
    pub at: Vec3,
    pub shooter: Option<Entity>,
    /// The shooter's side and whether it is a base, for the difficulty factor (`side_damage_scale`); None for scripted blasts.
    pub owner: Option<(Side, bool)>,
}

/// A shell spawned without a gun mount (a dragon's breath, a Nova mortar's secondaries): an ordinary shell of `bullet`.
#[derive(Message)]
pub struct Shoot {
    pub bullet: String,
    pub from: Vec3,
    pub vel: Vec3,
    pub side: Side,
    pub shooter: Entity,
    pub target: Option<Entity>,
    /// Downward acceleration on the shell (units/s^2).
    pub gravity: f32,
    /// A lob's aim point (it bursts coming down through its height).
    pub aim: Option<Vec3>,
    pub ricochet: f32,
}

pub fn shoot(mut commands: Commands, lib: Res<Library>, fxl: Res<FxLib>, mut msgs: MessageReader<Shoot>) {
    for m in msgs.read() {
        let Some(b) = lib.bullets.get(&m.bullet) else { continue };
        let stf = Transform::from_translation(m.from).looking_to(m.vel, Vec3::Y);
        let shell = commands
            .spawn((
                Shell {
                    vel: m.vel,
                    age: 0.0,
                    lifetime: b.lifetime.min(30.0),
                    gravity: m.gravity,
                    bullet: m.bullet.clone(),
                    torpedo: b.torpedo,
                    side: m.side,
                    shooter: m.shooter,
                    target: m.target,
                    angular_velocity: 0.0,
                    payload: b.payload,
                    aim: m.aim,
                    pd: false,
                    hp: b.hitpoints,
                    harmless: false,
                    base: false,
                    ricochet: m.ricochet,
                    mine_scan: Default::default(),
                    lost_target: false,
                },
                Transient,
                crate::Joining,
                stf,
                Visibility::default(),
            ))
            .id();
        if let Some(n) = &b.bullet_fx {
            fx::spawn_with(&mut commands, &fxl, n, Some(shell), Transform::default(), stf, false, fx::Origin { shell: Some(shell), ..default() });
        }
    }
}

/// A star-class blast (StarMortarCustomInfoFactory: MaxRadius over Duration): a ring spreading from
/// the burst at MaxRadius / Duration that strikes everything once as it passes its centre (`blasts`).
#[derive(Component)]
pub struct Blast {
    bullet: String,
    shooter: Option<Entity>,
    owner: Option<(Side, bool)>,
    radius: f32,
    duration: f32,
    age: f32,
}

/// A gravity charge's well (GravChargeCustomInfoFactory: Magnitude, Radius, Duration): for
/// `Duration` seconds everything within `radius` (flat distance) is pulled straight toward its centre, with no friend/foe filter.
/// The ship pull itself is `blackhole::pull`, which reads `force`.
#[derive(Component)]
pub struct GravWell {
    pub radius: f32,
    magnitude: f32,
    left: f32,
}

impl GravWell {
    /// Force at flat distance `d` from the centre, in Magnitude units: 0 at the centre and at the rim, `magnitude` at half the
    /// radius (profile 4 s² for s < 0.5, 4 (1 - s)² beyond, s = (radius - d) / radius).
    pub fn force(&self, d: f32) -> f32 {
        if d >= self.radius {
            return 0.0;
        }
        let s = (self.radius - d) / self.radius;
        self.magnitude * 4.0 * if s < 0.5 { s * s } else { (1.0 - s) * (1.0 - s) }
    }
}

/// What a Nova mortar splits into at the top of its arc (`split_nova`).
pub(super) const NOVA_SECONDARY: &str = "Bullet_NovaMortar_Secondary";

pub fn detonate(
    mut commands: Commands,
    lib: Res<Library>,
    fxl: Res<FxLib>,
    mut msgs: MessageReader<Detonate>,
    mut sfx: MessageWriter<Sfx>,
    mut rng: ResMut<SimRng>,
    mission: Option<ResMut<crate::campaign::Mission>>,
) {
    let mut mission = mission;
    for d in msgs.read() {
        let Some(b) = lib.bullets.get(&d.bullet) else { continue };
        if let Some(m) = mission.as_mut() {
            m.script.mortar_bursts.push(d.at);
        }
        sfx.write_batch(b.impact_sound("Wood", d.at, &mut rng));
        if let Some(n) = &b.hit_fx {
            let t = Transform::from_translation(d.at);
            fx::spawn(&mut commands, &fxl, n, None, t, t, true);
        }
        match b.payload {
            Payload::Blast { radius, duration } => {
                commands.spawn((
                    Blast { bullet: d.bullet.clone(), shooter: d.shooter, owner: d.owner, radius, duration: duration.max(0.1), age: 0.0 },
                    Transform::from_translation(d.at),
                    Transient,
                    crate::Scenario,
                    crate::Joining,
                ));
            }
            Payload::Grav { radius, duration, magnitude } => {
                commands.spawn((GravWell { radius, magnitude, left: duration }, Transform::from_translation(d.at), Transient, crate::Scenario, crate::Joining));
            }
            // A Nova splits in flight (`split_nova`); one that lands or strikes first is just spent.
            Payload::Nova | Payload::Shell | Payload::Mine { .. } => {}
        }
    }
}

/// The five secondaries' velocities of a Nova mortar splitting with velocity `v`: one straight on, four turned 0.1 rad either way
/// about the shell's up and right axes. PROVISIONAL: the pattern is not rolled with the shell's spin.
pub(super) fn nova_split(v: Vec3) -> [Vec3; 5] {
    let right = v.cross(Vec3::Y).normalize_or(Vec3::X);
    let up = right.cross(v).normalize_or(Vec3::Y);
    [(0.0, 0.0), (-0.1, -0.1), (0.1, -0.1), (-0.1, 0.1), (0.1, 0.1)].map(|(yaw, pitch)| Quat::from_axis_angle(up, yaw) * Quat::from_axis_angle(right, pitch) * v)
}

/// Spreads each blast and strikes what its ring passes: every ship and base section, rock and shell
/// in flight is struck once, when the ring (radius MaxRadius x age / Duration) reaches its centre, for the bullet's damage roll for
/// the material times max(0.1, 1 - distance / MaxRadius). Ship sections take the difficulty factor and spill; no fires start. Nobody is
/// spared, the shooter's own ships included; star-class shells are immune. PROVISIONAL: the push the blast gives what is inside it
/// (the bullet's `Magnitude`) is not ported (its scale to ship motion is unmeasured); lifeboats are not struck.
#[allow(clippy::too_many_arguments)]
pub fn blasts(
    spawn_order: Query<&crate::scenario::SpawnIndex>,
    mut commands: Commands,
    time: Res<Time>,
    lib: Res<Library>,
    fxl: Res<FxLib>,
    mut rng: ResMut<SimRng>,
    difficulty: Res<crate::scenario::Difficulty>,
    (mut hits, mut destroyed, mut splashes): (MessageWriter<Hit>, MessageWriter<Destroyed>, MessageWriter<super::Splashed>),
    q: Query<(Entity, &mut Blast, &Transform), (Without<Ship>, Without<Shell>, Without<crate::Joining>)>,
    mut ships: Query<(Entity, &Transform, &Ship, &mut Health, Option<&crate::campaign::MissionShip>), Without<crate::lifeboats::Lifeboat>>,
    mut bases: Query<(Entity, &Transform, &ArmedBase, &mut Health), (Without<Ship>, Without<Blast>)>,
    mut rocks: Query<(Entity, &Transform, &mut Asteroid), (Without<Ship>, Without<Blast>)>,
    mut shells: Query<(Entity, &mut Shell, &Transform), (Without<Ship>, Without<Blast>)>,
) {
    let dt = time.delta_secs();
    let mut q = q;
    for (e, mut b, tf) in crate::scenario::in_spawn_order(&spawn_order, q.iter_mut(), |x| x.0) {
        let before = b.radius * (b.age / b.duration).min(1.0);
        b.age += dt;
        let now = b.radius * (b.age / b.duration).min(1.0);
        let (c, max) = (tf.translation, b.radius);
        let passed = |p: Vec3| {
            let r = p.distance(c);
            (r >= before && r < now && r < max).then(|| (1.0 - r / max).max(0.1))
        };
        let Some(bd) = lib.bullets.get(&b.bullet) else {
            crate::queue_removal(&mut commands, e);
            continue;
        };
        let by = b.shooter.unwrap_or(e);
        for (se, stf, ship, mut hp, ms) in &mut ships {
            if hp.destroyed {
                continue;
            }
            let class = &lib.classes[ship.class];
            let scale = b.owner.map_or(1.0, |(side, base)| side_damage_scale(side, ship.side, difficulty.0, base));
            let mut struck = false;
            for k in 0..class.sections.len() {
                let sec = &class.sections[k];
                let Some(f) = passed(stf.transform_point((sec.min + sec.max) / 2.0)) else { continue };
                let [lo, hi, ..] = bd.damage[material_key(&sec.material)];
                let f = f * scale;
                apply_ship_hit(class, &mut hp, k, [lo * f, hi * f, 0.0, 0.0, 0.0], ms.map_or(0.0, |m| m.invuln), false, &mut rng);
                struck = true;
            }
            if struck {
                hits.write(Hit { shooter: by, victim: se });
                splashes.write(super::Splashed(se));
            }
        }
        for (be, btf, ab, mut hp) in &mut bases {
            if hp.destroyed {
                continue;
            }
            let class = &lib.classes[ab.class];
            let mut struck = false;
            for k in 0..class.sections.len() {
                let sec = &class.sections[k];
                let Some(f) = passed(btf.transform_point((sec.min + sec.max) / 2.0)) else { continue };
                let [lo, hi, ..] = bd.damage[material_key(&sec.material)];
                let fallen = apply_base_hit(class, &mut hp, k, rng.range(lo, hi) * f);
                base_parts_burst(&mut commands, &fxl, class, btf, fallen);
                struck = true;
            }
            if struck {
                hits.write(Hit { shooter: by, victim: be });
            }
        }
        for (re, rtf, mut rock) in &mut rocks {
            let Some(f) = passed(rtf.translation).filter(|_| rock.hp > 0.0) else { continue };
            let [lo, hi, ..] = bd.damage["Stone"];
            if rock.damage(rng.range(lo, hi) * f) {
                destroyed.write(Destroyed { by, victim: re });
            }
        }
        for (pe, mut shell, ptf) in &mut shells {
            if matches!(shell.payload, Payload::Blast { .. }) {
                continue;
            }
            let Some(f) = passed(ptf.translation) else { continue };
            let [lo, hi, ..] = bd.damage["Iron"];
            shell.hp -= rng.range(lo, hi) * f;
            if shell.hp <= 0.0 {
                if let Some(pd) = lib.bullets.get(&shell.bullet) {
                    hit_fx(&mut commands, &fxl, pd, ptf.translation, shell.vel, None, Surface::default());
                }
                crate::queue_removal(&mut commands, pe);
            }
        }
        if b.age >= b.duration {
            crate::queue_removal(&mut commands, e);
        }
    }
}

/// Wells expire after their duration; the pull on ships is `blackhole::pull`.
pub fn grav_wells(mut commands: Commands, time: Res<Time>, mut wells: Query<(Entity, &mut GravWell), Without<crate::Joining>>) {
    for (e, mut w) in &mut wells {
        w.left -= time.delta_secs();
        if w.left <= 0.0 {
            crate::queue_removal(&mut commands, e);
        }
    }
}

/// Environmental damage to one selected section using its material's weather potential.
#[derive(Message)]
pub struct Strike {
    pub victim: Entity,
    pub damage: std::collections::HashMap<String, [f32; 5]>,
    pub section: usize,
}

pub fn strikes(
    lib: Res<Library>,
    mut rng: ResMut<SimRng>,
    difficulty: Res<crate::scenario::Difficulty>,
    mut msgs: MessageReader<Strike>,
    mut ships: Query<(&Ship, &mut Health, Option<&crate::campaign::MissionShip>)>,
) {
    for m in msgs.read() {
        let Ok((ship, mut hp, ms)) = ships.get_mut(m.victim) else { continue };
        if hp.destroyed {
            continue;
        }
        let class = &lib.classes[ship.class];
        let k = m.section;
        let Some(section) = class.sections.get(k).filter(|_| k < hp.sections.len()) else { continue };
        let key = material_key(&section.material);
        let Some(damage) = m.damage.get(key) else { continue };
        // The hit carries no attacker, so only the human's own ships take the difficulty factor (as fires do).
        let f = super::shells::side_damage_scale(Side::Enemy, ship.side, difficulty.0, false);
        let damage = [damage[0] * f, damage[1] * f, damage[2], damage[3], damage[4]];
        apply_ship_hit(class, &mut hp, k, damage, ms.map_or(0.0, |m| m.invuln), false, &mut rng);
    }
}

/// Difficulty factor of a ram by ownership: the struck ship's owner is the human (incoming factor), else the rammer's is (outgoing).
fn ram_scale(victim_human: bool, rammer_human: bool, difficulty: u8) -> f32 {
    let as_side = |h: bool| if h { Side::Player } else { Side::Enemy };
    super::shells::side_damage_scale(as_side(rammer_human), as_side(victim_human), difficulty, false)
}

/// Contact damage (collision.rs `Ram`): one hit on the victim's section nearest the contact,
/// through the normal hit path. The difficulty factor follows who owns the two ships, not their side: a human-owned victim takes
/// the incoming factor, else a human-owned rammer gives the outgoing one, else x1 (a rock is a computer attacker; an allied
/// computer ship is not the human's). A damaging ram also plays the struck section's `Damage_` sound at the contact, but only once
/// when both ships' struck sections share a sound pool.
#[allow(clippy::too_many_arguments)]
pub fn ram_damage(
    lib: Res<Library>,
    mut rng: ResMut<SimRng>,
    difficulty: Res<crate::scenario::Difficulty>,
    mut msgs: MessageReader<crate::collision::Ram>,
    mut ships: Query<(&Ship, &Transform, &mut Health, Option<&crate::campaign::MissionShip>, Option<&crate::ship::FleetMember>)>,
    fleets: Option<Res<crate::ship::Fleets>>,
    mut sfx: MessageWriter<crate::audio::Sfx>,
    mut last: ResMut<super::LastDamager>,
) {
    let human = |s: &Ship, m: Option<&crate::ship::FleetMember>| crate::ship::is_human_ship(s.side, m, fleets.as_deref());
    let mut sounded: Vec<(Entity, Entity, &'static str)> = vec![];
    for r in msgs.read() {
        let rammer = ships.get(r.b).ok().map(|(s, _, _, _, m)| human(s, m));
        let Ok((ship, tf, mut hp, ms, member)) = ships.get_mut(r.a) else { continue };
        if hp.destroyed {
            continue;
        }
        let class = &lib.classes[ship.class];
        let scale = ram_scale(human(ship, member), rammer.unwrap_or(false), difficulty.0);
        let Some(k) = crate::collision::nearest_section(class, tf, r.at).filter(|k| *k < hp.sections.len()) else { continue };
        let d = r.damage * scale;
        apply_ship_hit(class, &mut hp, k, [d, d, 0.0, 0.0, 0.0], ms.map_or(0.0, |m| m.invuln), false, &mut rng);
        if rammer.is_some() {
            last.0.insert(r.a, r.b); // a rock's touch has no owner to credit
        }
        if let Some(pool) = class.sections.get(k).and_then(|s| super::shells::damage_pool(material_key(&s.material))) {
            if !sounded.contains(&(r.b, r.a, pool)) {
                sfx.write(super::shells::damage_sfx(pool, r.at, &mut rng));
            }
            sounded.push((r.a, r.b, pool));
        }
    }
}

#[cfg(test)]
mod tests;
