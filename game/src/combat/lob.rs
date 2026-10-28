//! Area weapons: lob-shell detonations (star/nova mortars, gravity charges), the blast spheres and gravity wells they leave,
//! mission 9's dark-matter blast, and hull damage from hard collisions (rams).
//! Data: the bullets' custom-info fields (`Payload`, exported by tools/export_combat.py). PROVISIONAL behaviour is marked below.
use super::*;

/// A bullet bursts at `at` (a landed lob shell, or a scripted blast with no shooter).
#[derive(Message)]
pub struct Detonate {
    pub bullet: String,
    pub at: Vec3,
    pub shooter: Option<Entity>,
}

/// A shot from a shooter that has no gun mounts (a dragon's breath): spawned as an ordinary shell of `bullet`.
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
                    side: m.side,
                    shooter: m.shooter,
                    target: m.target,
                    angular_velocity: 0.0,
                    payload: b.payload,
                    aim: None,
                    pd: false,
                    hp: b.hitpoints,
                    harmless: false,
                },
                Transient,
                stf,
                Visibility::default(),
            ))
            .id();
        if let Some(n) = &b.bullet_fx {
            fx::spawn(&mut commands, &fxl, n, Some(shell), Transform::default(), stf, false);
        }
    }
}

/// A growing blast sphere (StarMortarCustomInfoFactory: MaxRadius over Duration). Every unit it reaches is struck once.
#[derive(Component)]
pub struct Blast {
    bullet: String,
    shooter: Option<Entity>,
    radius: f32,
    duration: f32,
    age: f32,
    struck: Vec<Entity>,
}

/// A gravity charge's well (GravChargeCustomInfoFactory: Radius, Duration): ships inside are held.
#[derive(Component)]
pub struct GravWell {
    radius: f32,
    left: f32,
}

/// The Nova mortar bursts into secondary star-mortar blasts. PROVISIONAL layout: one at the landing point and four around it at
/// half the secondary's radius; the original spawns the secondaries itself and their count/placement is unknown.
const NOVA_SECONDARY: &str = "Bullet_NovaMortar_Secondary";

pub fn detonate(
    mut commands: Commands,
    lib: Res<Library>,
    fxl: Res<FxLib>,
    mut msgs: MessageReader<Detonate>,
    mut sfx: MessageWriter<Sfx>,
    mut rng: ResMut<Rng>,
    mission: Option<ResMut<crate::campaign::Mission>>,
) {
    let mut mission = mission;
    for d in msgs.read() {
        let Some(b) = lib.bullets.get(&d.bullet) else { continue };
        if let Some(m) = mission.as_mut() {
            m.script.mortar_bursts.push(d.at);
        }
        if let Some(pool) = b.impact_snd.get("Wood").filter(|p| !p.is_empty()) {
            sfx.write(Sfx { name: rng.pick(pool).clone(), at: Some(d.at), vol: b.impact_vol, max: b.impact_max });
        }
        let effect = |commands: &mut Commands, name: &Option<String>, at: Vec3| {
            if let Some(n) = name {
                let t = Transform::from_translation(at);
                fx::spawn(commands, &fxl, n, None, t, t, true);
            }
        };
        let blast = |commands: &mut Commands, bullet: &str, at: Vec3, radius: f32, duration: f32| {
            commands.spawn((
                Blast { bullet: bullet.into(), shooter: d.shooter, radius, duration: duration.max(0.1), age: 0.0, struck: vec![] },
                Transform::from_translation(at),
                Transient,
                crate::Scenario,
            ));
        };
        effect(&mut commands, &b.hit_fx, d.at);
        match b.payload {
            Payload::Blast { radius, duration } => blast(&mut commands, &d.bullet, d.at, radius, duration),
            Payload::Nova => {
                if let Some(sec) = lib.bullets.get(NOVA_SECONDARY)
                    && let Payload::Blast { radius, duration } = sec.payload
                {
                    for k in 0..5 {
                        let off = if k == 0 { Vec3::ZERO } else { Quat::from_rotation_y(k as f32 * std::f32::consts::FRAC_PI_2) * Vec3::X * radius * 0.5 };
                        effect(&mut commands, &sec.hit_fx, d.at + off);
                        blast(&mut commands, NOVA_SECONDARY, d.at + off, radius, duration);
                    }
                }
            }
            Payload::Grav { radius, duration } => {
                commands.spawn((GravWell { radius, left: duration }, Transform::from_translation(d.at), Transient, crate::Scenario));
            }
            Payload::Shell | Payload::Mine { .. } => {}
        }
    }
}

/// Grows each blast and strikes every ship, base section and rock it reaches with the bullet's damage for that material.
/// PROVISIONAL: the struck section is the one nearest the blast centre; no fires are started; friend and foe alike are hit
/// (only the shooter itself is spared).
pub fn blasts(
    mut commands: Commands,
    time: Res<Time>,
    lib: Res<Library>,
    mut rng: ResMut<Rng>,
    mut hits: MessageWriter<Hit>,
    mut destroyed: MessageWriter<Destroyed>,
    mut q: Query<(Entity, &mut Blast, &Transform), Without<Ship>>,
    mut ships: Query<(Entity, &Transform, &Ship, &mut Health, Option<&crate::campaign::MissionShip>), Without<crate::lifeboats::Lifeboat>>,
    mut rocks: Query<(Entity, &Transform, &mut Asteroid), Without<Ship>>,
) {
    let dt = time.delta_secs();
    for (e, mut b, tf) in &mut q {
        b.age += dt;
        let r = b.radius * (b.age / b.duration).min(1.0);
        let c = tf.translation;
        let Some(bd) = lib.bullets.get(&b.bullet) else {
            commands.entity(e).despawn();
            continue;
        };
        for (se, stf, ship, mut hp, ms) in &mut ships {
            if hp.destroyed || Some(se) == b.shooter || b.struck.contains(&se) {
                continue;
            }
            let class = &lib.classes[ship.class];
            if stf.translation.distance(c) - class.radius > r {
                continue;
            }
            b.struck.push(se);
            hits.write(Hit { shooter: b.shooter.unwrap_or(e), victim: se });
            let Some(k) = nearest_section(class, stf.to_matrix().inverse().transform_point3(c)) else { continue };
            let key = material_key(&class.sections[k].material);
            apply_ship_hit(class, &mut hp, k, bd.damage[key], ms.map_or(0.0, |m| m.invuln), false, &mut rng);
        }
        for (re, rtf, mut rock) in &mut rocks {
            if rock.hp <= 0.0 || b.struck.contains(&re) || rtf.translation.distance(c) - rock.radius > r {
                continue;
            }
            b.struck.push(re);
            let [lo, hi, ..] = bd.damage["Stone"];
            if rock.damage(rng.range(lo, hi)) {
                destroyed.write(Destroyed { by: b.shooter.unwrap_or(e), victim: re });
            }
        }
        if b.age >= b.duration {
            commands.entity(e).despawn();
        }
    }
}

/// Ships inside a gravity well lose their way. PROVISIONAL rate (the original's well force is unknown); the data only says the
/// well holds ships, so there is no pull towards its centre (an earlier guessed 4 units/s^2 pull dragged ships across it).
pub fn grav_wells(
    mut commands: Commands,
    time: Res<Time>,
    mut wells: Query<(Entity, &mut GravWell, &Transform), Without<Ship>>,
    mut ships: Query<(&Transform, &mut Ship, &Health)>,
) {
    let dt = time.delta_secs();
    for (e, mut w, tf) in &mut wells {
        w.left -= dt;
        if w.left <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        for (stf, mut ship, h) in &mut ships {
            if !h.destroyed && (tf.translation - stf.translation).with_y(0.0).length() <= w.radius {
                ship.speed *= (-1.5 * dt).exp();
            }
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

pub fn strikes(lib: Res<Library>, mut rng: ResMut<Rng>, mut msgs: MessageReader<Strike>, mut ships: Query<(&Ship, &mut Health, Option<&crate::campaign::MissionShip>)>) {
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
        apply_ship_hit(class, &mut hp, k, *damage, ms.map_or(0.0, |m| m.invuln), false, &mut rng);
    }
}

/// The section whose box centre is nearest `local` (a point in the ship's frame).
fn nearest_section(class: &Class, local: Vec3) -> Option<usize> {
    (0..class.sections.len()).min_by(|x, y| {
        let d = |i: usize| ((class.sections[i].min + class.sections[i].max) / 2.0).distance(local);
        d(*x).total_cmp(&d(*y))
    })
}

/// Hull damage of a hard contact (collision.rs `Ram`): both ships lose hit points on the section nearest the contact.
/// PROVISIONAL: (impulse - threshold) / 400 hit points, a guess sized so a full-speed ram costs a frigate section about a third.
pub fn ram_damage(
    lib: Res<Library>,
    mut rng: ResMut<Rng>,
    mut msgs: MessageReader<crate::collision::Ram>,
    mut ships: Query<(&Transform, &Ship, &mut Health, Option<&crate::campaign::MissionShip>)>,
) {
    for r in msgs.read() {
        let amount = (r.impulse - crate::collision::RAM_IMPULSE) / 400.0;
        for e in if r.a == r.b { vec![r.a] } else { vec![r.a, r.b] } {
            let Ok((tf, ship, mut hp, ms)) = ships.get_mut(e) else { continue };
            if hp.destroyed {
                continue;
            }
            let class = &lib.classes[ship.class];
            let Some(k) = nearest_section(class, tf.to_matrix().inverse().transform_point3(r.at)) else { continue };
            apply_ship_hit(class, &mut hp, k, [amount, amount, 0.0, 0.0, 0.0], ms.map_or(0.0, |m| m.invuln), false, &mut rng);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storm_wind_uses_weather_damage_and_respects_mission_protection() {
        let lib = Library::load(crate::assets_dir());
        let class = lib.index("Ship_Navy_Sloop");
        let k = lib.classes[class].sections.iter().position(|s| s.material.contains("Cloth")).unwrap();
        let hp = Health::new(&lib.classes[class]);
        let before = hp.sections[k];
        let core = hp.core;
        let ship = Ship {
            class,
            side: crate::Side::Player,
            heading: 0.0,
            speed: 0.0,
            ang_vel: 0.0,
            order: 0,
            waypoints: vec![],
            goal: None,
            cap: 1.0,
            throttle: None,
            sail_energy: 1.0,
            crew_speed: 1.0,
            crew_turn: 1.0,
        };
        let tables: std::collections::HashMap<String, std::collections::HashMap<String, [f32; 5]>> =
            serde_json::from_value(crate::data::read_json(&crate::assets_dir().join("data/nebula.json"))).unwrap();
        assert_eq!(&tables["wind"]["Cloth"][..2], &[5.0, 7.0]);
        let mut app = App::new();
        app.insert_resource(lib).insert_resource(Rng(123)).add_message::<Strike>().add_systems(Update, strikes);
        let e = app.world_mut().spawn((Transform::default(), ship, hp, crate::campaign::MissionShip::default())).id();
        for _ in 0..3 {
            app.world_mut().write_message(Strike { victim: e, damage: tables["wind"].clone(), section: k });
        }
        app.world_mut().write_message(Strike { victim: e, damage: tables["meteor"].clone(), section: usize::MAX });
        app.update();
        let hp = app.world().get::<Health>(e).unwrap();
        assert!((15.0..=21.0).contains(&(before - hp.sections[k])));
        assert_eq!(hp.core, core);
        let after = hp.sections[k];
        app.world_mut().get_mut::<crate::campaign::MissionShip>(e).unwrap().invuln = 1.0;
        app.world_mut().write_message(Strike { victim: e, damage: tables["meteor"].clone(), section: k });
        app.update();
        assert_eq!(app.world().get::<Health>(e).unwrap().sections[k], after);
    }

    #[test]
    fn lob_arc_lands_on_the_aim_point() {
        let d = Vec3::new(300.0, -2.0, -150.0);
        const G: f32 = 1.263; // retail star mortar
        let v = super::super::lob_velocity(d, std::f32::consts::FRAC_PI_4, G).unwrap();
        // Fly the ballistic arc and find where it comes back down to the target height.
        let (mut p, mut vel, dt) = (Vec3::ZERO, v, 0.001);
        while !(vel.y < 0.0 && p.y <= d.y) {
            vel.y -= G * dt;
            p += vel * dt;
        }
        assert!(p.distance(d) < 1.5, "landed {p} for {d}");
        assert!(super::super::lob_velocity(Vec3::new(10.0, 50.0, 0.0), 0.3, G).is_none(), "point above the arc");
    }
}
