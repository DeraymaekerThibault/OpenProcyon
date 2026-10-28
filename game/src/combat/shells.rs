//! Shells in flight: motion, torpedo guidance, point defence, hits on ships, bases, scenery and asteroids, and their damage.
use super::*;

#[derive(Component)]
pub struct Shell {
    pub(super) vel: Vec3,
    pub(super) age: f32,
    pub(super) lifetime: f32,
    /// Downward acceleration (units/s^2): the firing gun's `GunDef::shell_gravity`, 0 for straight-flying shells.
    pub(super) gravity: f32,
    pub(super) bullet: String,
    pub(super) side: Side,
    pub(super) shooter: Entity,
    pub(super) target: Option<Entity>,
    pub(super) angular_velocity: f32,
    pub(super) payload: Payload,
    /// Lob and mine shells: the point they were fired at (a lob bursts descending through its height; a mine settles there).
    pub(super) aim: Option<Vec3>,
    /// Fired by a point-defence gun at a torpedo or mine.
    pub(super) pd: bool,
    /// Remaining projectile hitpoints (`BulletDef::hitpoints`); point-defence bolts wear torpedoes and mines down.
    pub(super) hp: f32,
    /// A warning shot ('Set Group/Unit Warning Shot Mode'): no damage.
    pub(super) harmless: bool,
}
/// Core damage of one hit on a destroyed vital section: `roll` x (sum of core% over destroyed vital sections) / 100.
pub(super) fn core_bleed(class: &Class, hp: &Health, roll: f32) -> f32 {
    let pct: f32 = class.sections.iter().zip(&hp.sections).filter(|(s, h)| s.vital && **h <= 0.0).map(|(s, _)| s.core_pct).sum();
    (roll * pct / 100.0).round()
}

/// Native launch coast (0.5 s), 20-degree rudder saturation and lead inside 100 units; native WOT physics.
/// PROVISIONAL: uses the remake's planar integrator; the native invalidation timer and altitude tick scheduling remain approximate.
/// One point-defence bolt against a torpedo or mine: it loses the bolt's hitpoints; true once it is destroyed.
pub(super) fn wear_down(threat_hp: &mut f32, bolt_hp: f32) -> bool {
    *threat_hp -= bolt_hp;
    *threat_hp <= 0.0
}

pub(super) fn guide_torpedo(mut position: Vec3, velocity: &mut Vec3, spin: &mut f32, age: f32, target: Option<(Vec3, Vec3)>, physics: &crate::data::Physics, dt: f32) -> Vec3 {
    let steps = (dt * 60.0).ceil().max(1.0) as usize;
    let h = dt / steps as f32;
    for i in 0..steps {
        let speed = velocity.with_y(0.0).length();
        let mut heading = (-velocity.x).atan2(-velocity.z);
        let mut rudder = 0.0;
        if age + i as f32 * h >= 0.5
            && let Some((point, target_velocity)) = target
        {
            let distance = point.distance(position);
            let aim = point + if distance < 100.0 { target_velocity * (distance / speed.max(0.01)) } else { Vec3::ZERO };
            let delta = aim - position;
            let want = (-delta.x).atan2(-delta.z);
            rudder = (crate::angdiff(want, heading) / 20.0f32.to_radians()).clamp(-1.0, 1.0);
            // Native height adjustment is 0.025 every ~0.05 s, bounded to +/-3 units.
            let height = (point.y - position.y).clamp(-0.5 * h, 0.5 * h);
            position.y = (position.y + height).clamp(-3.0, 3.0);
        }
        let mut acceleration = rudder * physics.max_ang_acc * (1.0 - 1.0 / (0.1 * speed + 1.0));
        if speed > 0.67 * physics.max_speed {
            acceleration /= 3.0;
        } else if speed < 0.33 * physics.max_speed {
            acceleration *= 5.0 / 3.0;
        }
        acceleration -= spin.signum() * *spin * *spin * physics.rot_friction;
        *spin += acceleration.clamp(-60.0, 60.0) * h;
        heading += *spin * h;
        let speed = crate::approach_speed(speed, physics.max_speed, physics.thrust / physics.mass / physics.max_speed, h);
        *velocity = Vec3::new(-heading.sin(), 0.0, -heading.cos()) * speed;
        position += *velocity * h;
    }
    position
}

/// What one shell hit on a ship section did beyond the lost hit points.
pub(super) struct ShipHit {
    /// Initial strength of a fire that started.
    pub(super) fire: Option<f32>,
    /// The section went from alive to destroyed with this hit.
    pub(super) lost: bool,
}

/// Measured on retail (headless 1-vs-1 skirmishes per difficulty): the bullet table's damage is scaled by side and difficulty.
/// Player shells on computer ships: Easy x4, Normal x2, Hard x1; computer shells on the player's: x0.25, x0.5, x1.
/// PROVISIONAL: Easy x4 rests on two hits; computer-vs-computer and allied hits are not measured (1.0 here).
pub(super) fn side_damage_scale(shooter: Side, victim: Side, difficulty: u8) -> f32 {
    let k = [4.0, 2.0, 1.0][difficulty.min(2) as usize];
    match (shooter, victim) {
        (Side::Player, v) if v != Side::Player => k,
        (s, Side::Player) if s != Side::Player => 1.0 / k,
        _ => 1.0,
    }
}

pub(super) fn apply_ship_hit(class: &Class, hp: &mut Health, k: usize, damage: [f32; 5], invuln: f32, fire_free: bool, rng: &mut Rng) -> ShipHit {
    let [lo, hi, _crit, fire_p, fire_str] = damage;
    let protected = invuln >= 1.0;
    let was = hp.sections[k];
    hp.sections[k] = (was - (1.0 - invuln) * (rng.range(lo, hi))).max(0.0);
    // Retail ignition roll per hit: chance > 0 and roll <= chance starts a fire of the bullet's initial strength.
    // PROVISIONAL: one fire per section (the original's extra gate is unknown).
    let fire = (!protected && fire_p > 0.0 && rng.f() <= fire_p && hp.sections[k] > 0.0 && fire_free).then_some(fire_str);
    // Retail: every hit that leaves a vital section at 0 HP (the killing hit and later ones) takes
    // a fresh damage roll x the summed core% of all destroyed vital sections / 100 off the core (rounding provisional).
    if !protected && class.sections[k].vital && hp.sections[k] == 0.0 {
        hp.core -= core_bleed(class, hp, (1.0 - invuln) * (rng.range(lo, hi)));
    }
    ShipHit { fire, lost: was > 0.0 && hp.sections[k] == 0.0 }
}

/// The bullet's impact effect at `at`, facing back along its flight.
fn hit_fx(commands: &mut Commands, fxl: &FxLib, bd: &BulletDef, at: Vec3, vel: Vec3) {
    if let Some(n) = &bd.hit_fx {
        let t = Transform::from_translation(at).looking_to(-vel, Vec3::Y);
        fx::spawn(commands, fxl, n, None, t, t, true);
    }
}

pub(super) fn shells(
    mut commands: Commands,
    time: Res<Time>,
    lib: Res<Library>,
    fxl: Res<FxLib>,
    mut rng: ResMut<Rng>,
    mut sfx: MessageWriter<Sfx>,
    mut hits: MessageWriter<Hit>,
    mut destroyed: MessageWriter<Destroyed>,
    mut rocks: Query<(Entity, &Transform, &mut Asteroid, &crate::collision::Drift), Without<Shell>>,
    animals: Query<(Entity, &Transform, &crate::environment::SpaceAnimal), Without<Shell>>,
    mut bases: Query<(Entity, &Transform, &ArmedBase, &mut Health, &crate::campaign::MissionBase, Option<&BaseSide>), (Without<Shell>, Without<Ship>)>,
    campaign: Option<Res<crate::campaign::Mission>>,
    mut q: Query<(Entity, &mut Shell, &mut Transform), Without<Ship>>,
    // Shells pass through lifeboats (no weapon-vs-lifeboat response is registered in the original; PROVISIONAL, docs/re/lifeboats.md §2.4).
    mut ships: Query<
        (Entity, &Transform, &Ship, &mut Health, &mut Fires, Option<&crate::campaign::MissionShip>, Option<&crate::collision::Drift>),
        Without<crate::lifeboats::Lifeboat>,
    >,
    mut detonate: MessageWriter<Detonate>,
    (mut dragons, mut scars): (Query<&mut crate::dragons::Dragon>, MessageWriter<crate::decals::Scar>),
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    // Torpedoes and mines in flight, for point-defence intercepts; each is shot down at most once.
    let threats: Vec<(Entity, Vec3, Side)> =
        q.iter().filter(|(_, s, _)| s.bullet.starts_with(TORPEDO_BULLET) || matches!(s.payload, Payload::Mine { .. })).map(|(e, s, t)| (e, t.translation, s.side)).collect();
    let mut threat_hp: std::collections::HashMap<Entity, f32> = q.iter().filter(|(e, ..)| threats.iter().any(|t| t.0 == *e)).map(|(e, s, _)| (e, s.hp)).collect();
    let mut downed: Vec<Entity> = vec![];
    for (e, mut s, mut tf) in &mut q {
        if downed.contains(&e) {
            continue;
        }
        let p0 = tf.translation;
        let bd = &lib.bullets[&s.bullet];
        s.vel.y -= s.gravity * dt;
        // Mines (MineAI sight, MinePhysics max speed): home on the nearest hostile ship in sight, else coast to rest at the drop point.
        // PROVISIONAL: the original's mine steering/drag is unknown.
        if let Payload::Mine { sight, speed } = s.payload {
            let prey = ships
                .iter()
                .filter(|(_, t, sh, h, _, m, _)| {
                    !h.destroyed && sh.side != s.side && m.is_none_or(|m| m.visible) && t.translation.distance(p0) < sight + lib.classes[sh.class].radius
                })
                .map(|(_, t, ..)| t.translation)
                .min_by(|a, b| a.distance(p0).total_cmp(&b.distance(p0)));
            s.vel = match prey {
                Some(p) => s.vel.lerp((p - p0).normalize_or_zero() * speed, (3.0 * dt).min(1.0)),
                None if s.aim.is_none_or(|a| a.distance(p0) < 2.0) || s.age > 3.0 => s.vel * (-1.5 * dt).exp(),
                None => s.vel,
            };
        }
        let p1 = if let Some(physics) = &bd.physics {
            let target = s
                .target
                .and_then(|target| ships.get(target).ok())
                .filter(|(_, _, ship, health, _, mission, _)| !health.destroyed && ship.side != s.side && mission.is_none_or(|m| m.visible))
                .map(|(_, tf, ship, _, _, _, drift)| {
                    let point = tf.transform_point(lib.classes[ship.class].homing_point);
                    let velocity = Vec3::new(-ship.heading.sin(), 0.0, -ship.heading.cos()) * ship.speed + drift.map_or(Vec3::ZERO, |d| d.0);
                    (point, velocity)
                })
                .or_else(|| {
                    s.target.and_then(|target| rocks.get(target).ok()).filter(|(_, _, rock, _)| rock.hp > 0.0).map(|(_, tf, rock, drift)| (tf.translation, rock.velocity + drift.0))
                })
                .or_else(|| s.target.and_then(|target| animals.get(target).ok()).filter(|(_, _, animal)| animal.visible).map(|(_, tf, animal)| (tf.translation, animal.velocity)));
            if target.is_none() {
                s.target = None;
            }
            let mut velocity = s.vel;
            let mut spin = s.angular_velocity;
            let position = guide_torpedo(p0, &mut velocity, &mut spin, s.age, target, physics, dt);
            s.vel = velocity;
            s.angular_velocity = spin;
            position
        } else {
            p0 + s.vel * dt
        };
        s.age += dt;
        // Point defence: the bolt meets a hostile torpedo or mine on its way and is spent; the threat loses the bolt's hitpoints and
        // is destroyed at zero. PROVISIONAL: hitpoint-for-hitpoint exchange inferred from the WOT values, not verified in the original.
        if s.pd
            && let Some(&(te, tp, _)) = threats.iter().find(|t| t.2 != s.side && !downed.contains(&t.0) && segment_distance(t.1, p0, p1) < 2.5)
        {
            commands.entity(e).despawn();
            let at = Transform::from_translation(tp);
            if wear_down(threat_hp.entry(te).or_insert(1.0), s.hp) {
                downed.push(te);
                commands.entity(te).despawn();
                fx::spawn(&mut commands, &fxl, "Explosion_Light", None, at, at, true);
                sfx.write(Sfx { name: format!("Explosion_Med0{}", rng.variant(8)), at: Some(tp), vol: 0.8, max: 600.0 });
            } else if let Some(fx_name) = &bd.hit_fx {
                fx::spawn(&mut commands, &fxl, fx_name, None, at, at, true);
            }
            continue;
        }
        // Lob shells burst where they come down through the height they were aimed at.
        if let Some(a) = s.aim.filter(|a| !matches!(s.payload, Payload::Mine { .. }) && s.vel.y < 0.0 && p1.y <= a.y) {
            let t = if (p0.y - p1.y).abs() > 1e-4 { ((p0.y - a.y) / (p0.y - p1.y)).clamp(0.0, 1.0) } else { 1.0 };
            detonate.write(Detonate { bullet: s.bullet.clone(), at: p0.lerp(p1, t), shooter: Some(s.shooter) });
            commands.entity(e).despawn();
            continue;
        }
        let mut best: Option<(f32, usize, Vec3)> = None; // (t, section, class)
        let mut best_ship: Option<Entity> = None;
        for (i, stf, ship, hp, _, m, _) in ships.iter() {
            if hp.destroyed || ship.side == s.side || m.is_some_and(|m| !m.visible) {
                continue;
            }
            let class = &lib.classes[ship.class];
            if segment_distance(stf.translation, p0, p1) > class.hull_bound * stf.scale.max_element() {
                continue;
            }
            let inv = stf.to_matrix().inverse();
            let (a, b) = (inv.transform_point3(p0), inv.transform_point3(p1));
            let hit = ship_segment_hit(class, hp, a, b);
            if let Some((t, k)) = hit
                && best.as_ref().is_none_or(|x| t < x.0)
            {
                best = Some((t, k, p0 + (p1 - p0) * t));
                best_ship = Some(i);
            }
        }
        let rock = rocks
            .iter()
            .filter(|(_, _, rk, _)| rk.hp > 0.0)
            .filter_map(|(re, rtf, rk, _)| seg_sphere(p0, p1, rtf.translation, rk.radius).map(|t| (t, re)))
            .min_by(|a, b| a.0.total_cmp(&b.0));
        let animal = animals
            .iter()
            .filter(|(ae, _, a)| a.visible && !a.dead && *ae != s.shooter)
            .filter_map(|(ae, atf, a)| seg_sphere(p0, p1, atf.translation, a.radius).map(|t| (t, ae)))
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((t, ae)) = animal.filter(|a| best.as_ref().is_none_or(|b| a.0 < b.0) && rock.is_none_or(|r| a.0 < r.0)) {
            hits.write(Hit { shooter: s.shooter, victim: ae });
            // Dragons have one 'Dragon Scale' section (500 HP). PROVISIONAL: whales have no exported strength/damage model; impacts
            // consume shells but do not invent ship-style HP.
            if let Ok(mut d) = dragons.get_mut(ae)
                && !s.harmless
            {
                let [lo, hi, ..] = bd.damage["Dragon"];
                d.hurt(rng.range(lo, hi), s.shooter, now);
            }
            hit_fx(&mut commands, &fxl, bd, p0 + (p1 - p0) * t, s.vel);
            commands.entity(e).despawn();
            continue;
        }
        if let Some((t, re)) = rock.filter(|r| best.as_ref().is_none_or(|b| r.0 < b.0)) {
            let at = p0 + (p1 - p0) * t;
            let [lo, hi, ..] = bd.damage["Stone"];
            hits.write(Hit { shooter: s.shooter, victim: re });
            if let Ok((_, _, mut rk, _)) = rocks.get_mut(re) {
                if rk.damage(rng.range(lo, hi)) {
                    destroyed.write(Destroyed { by: s.shooter, victim: re });
                } else {
                    sfx.write(Sfx { name: format!("Damage_Stone0{}", rng.variant(5)), at: Some(at), vol: 1.0, max: 600.0 });
                }
            }
            hit_fx(&mut commands, &fxl, bd, at, s.vel);
            commands.entity(e).despawn();
            continue;
        }
        // Island gun towers: hit only by the side the base's owner is hostile to (neutral islands let shells pass).
        let base_hit = bases
            .iter()
            .filter(|(.., hp, mb, side)| {
                !hp.destroyed
                    && match side {
                        Some(side) => side.0 != s.side,
                        None => campaign.as_deref().is_some_and(|c| {
                            let enemy = c.script.hostile(c.script.player(), &mb.owner);
                            (if enemy { Side::Enemy } else { Side::Player }) != s.side
                        }),
                    }
            })
            .filter_map(|(be, btf, ab, bhp, ..)| {
                let class = &lib.classes[ab.class];
                let inv = btf.to_matrix().inverse();
                let (a, b) = (inv.transform_point3(p0), inv.transform_point3(p1));
                // A destroyed mission part (building, ring) no longer stops shells.
                class
                    .sections
                    .iter()
                    .enumerate()
                    .filter(|(k, sec)| !sec.mission || bhp.sections[*k] > 0.0)
                    .filter_map(|(k, sec)| seg_mesh(a, b, sec).map(|t| (t, k, be)))
                    .min_by(|x, y| x.0.total_cmp(&y.0))
            })
            .min_by(|x, y| x.0.total_cmp(&y.0));
        if let Some((t, k, be)) = base_hit.filter(|b| best.as_ref().is_none_or(|x| b.0 < x.0))
            && let Ok((_, _, ab, mut hp, ..)) = bases.get_mut(be)
        {
            let sec = &lib.classes[ab.class].sections[k];
            let key = material_key(&sec.material);
            let [lo, hi, ..] = bd.damage[key];
            let at = p0 + (p1 - p0) * t;
            hits.write(Hit { shooter: s.shooter, victim: be });
            hp.sections[k] = (hp.sections[k] - (rng.range(lo, hi))).max(0.0);
            if let Some(pool) = bd.impact_snd.get(key).filter(|p| !p.is_empty()) {
                sfx.write(Sfx { name: rng.pick(pool).clone(), at: Some(at), vol: bd.impact_vol, max: bd.impact_max });
            }
            hit_fx(&mut commands, &fxl, bd, at, s.vel);
            commands.entity(e).despawn();
            continue;
        }
        // Blast, Nova and gravity shells burst on the hull they strike; warning shots splash harmlessly.
        if let (Some((_, k, at)), true) = (best, s.harmless || matches!(s.payload, Payload::Blast { .. } | Payload::Nova | Payload::Grav { .. })) {
            if s.harmless {
                // A warning shot glances off: the bullet's ricochet sound for the struck material.
                let material = best_ship.and_then(|si| ships.get(si).ok()).map(|x| lib.classes[x.2.class].sections[k].material.clone()).unwrap_or_default();
                let key = material_key(&material);
                if let Some(name) = bd.ricochet_snd.get(key) {
                    sfx.write(Sfx { name: name.clone(), at: Some(at), vol: 1.0, max: 600.0 });
                }
                hit_fx(&mut commands, &fxl, bd, at, s.vel);
            } else {
                detonate.write(Detonate { bullet: s.bullet.clone(), at, shooter: Some(s.shooter) });
            }
            commands.entity(e).despawn();
            continue;
        }
        if let (Some((_, k, at)), Some(si)) = (best, best_ship) {
            let (se, stf, ship, mut hp, mut fires, mission, _) = ships.get_mut(si).unwrap();
            let class = &lib.classes[ship.class];
            let sec = &class.sections[k];
            let key = material_key(&sec.material);
            // Criticals are not modelled: in the original they only feed a tally and crew speech (docs/re/movement.md).
            let mut damage = bd.damage[key];
            let scale = side_damage_scale(s.side, ship.side, crate::menu::difficulty());
            damage[0] *= scale;
            damage[1] *= scale;
            hits.write(Hit { shooter: s.shooter, victim: se });
            // Script vulnerability: 'Invulnerable Percent' 1.0 takes no damage and no fire; partial values scale the damage.
            let invuln = mission.map_or(0.0, |m| m.invuln);
            if s.bullet == TORPEDO_BULLET {
                info!("Torpedo hit {} ({}){}", class.name, sec.label, if invuln >= 1.0 { " [training: invulnerable]" } else { "" });
            }
            let fire_free = !fires.list.iter().any(|f| f.sec == k && f.state == 0);
            let before = hp.sections[k];
            let outcome = apply_ship_hit(class, &mut hp, k, damage, invuln, fire_free, &mut rng);
            if let (Some(size), true) = (bd.decal, hp.sections[k] < before) {
                let inv = stf.to_matrix().inverse();
                let material = if sec.material.contains("Cloth") { "Cloth" } else { key };
                scars.write(crate::decals::Scar { ship: se, local: inv.transform_point3(at), dir: inv.transform_vector3(s.vel).normalize_or_zero(), material, size });
            }
            if let Some(fire_str) = outcome.fire {
                let local = Transform::from_translation(stf.to_matrix().inverse().transform_point3(at));
                let fx = fx::spawn(&mut commands, &fxl, "FireOnShip", Some(se), local, Transform::from_translation(at), false);
                if let Some(e) = fx {
                    let sc = fire_scale(fire_str);
                    commands.entity(e).queue(move |mut w: EntityWorldMut| {
                        if let Some(mut f) = w.get_mut::<fx::Fx>() {
                            f.rate_scale = sc;
                        }
                    });
                }
                info!("fire on {} of {}", sec.label, class.name);
                fires.list.push(Fire { sec: k, intensity: fire_str, state: 0, t: 0.0, fx });
            }
            if let Some(pool) = bd.impact_snd.get(key).filter(|p| !p.is_empty()) {
                sfx.write(Sfx { name: rng.pick(pool).clone(), at: Some(at), vol: bd.impact_vol, max: bd.impact_max });
            }
            if outcome.lost {
                // Retail pool per section material (Damage_<Wood|Metal|Sail|Stone>01-05, 600 units). PROVISIONAL: played on section loss.
                let pool = match key {
                    "Wood" | "Reinforced" => Some("Wood"),
                    "Iron" => Some("Metal"),
                    "Cloth" => Some("Sail"),
                    "Stone" => Some("Stone"),
                    _ => None,
                };
                if let Some(p) = pool {
                    sfx.write(Sfx { name: format!("Damage_{p}0{}", rng.variant(5)), at: Some(at), vol: 1.0, max: 600.0 });
                }
                info!("{} of {} destroyed", sec.label, class.name);
            }
            hit_fx(&mut commands, &fxl, bd, at, s.vel);
            commands.entity(e).despawn();
        } else if s.age > s.lifetime || p1.y < -300.0 {
            commands.entity(e).despawn();
        } else {
            tf.translation = p1;
            tf.look_to(s.vel, Vec3::Y);
        }
    }
    // Torpedoes and mines keep the wear from point-defence bolts that did not finish them this frame.
    for (e, mut s, _) in &mut q {
        if let Some(&hp) = threat_hp.get(&e) {
            s.hp = hp;
        }
    }
}
