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

/// A star-class blast (StarMortarCustomInfoFactory: MaxRadius over Duration), read from the original's code: a ring spreading from
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

/// A gravity charge's well (GravChargeCustomInfoFactory: Radius, Duration): ships inside are held.
#[derive(Component)]
pub struct GravWell {
    radius: f32,
    left: f32,
}

/// What a Nova mortar splits into at the top of its arc (`split_nova`).
pub(super) const NOVA_SECONDARY: &str = "Bullet_NovaMortar_Secondary";

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
                ));
            }
            Payload::Grav { radius, duration } => {
                commands.spawn((GravWell { radius, left: duration }, Transform::from_translation(d.at), Transient, crate::Scenario));
            }
            // A Nova splits in flight (`split_nova`); one that lands or strikes first is just spent.
            Payload::Nova | Payload::Shell | Payload::Mine { .. } => {}
        }
    }
}

/// The five secondaries' velocities of a Nova mortar splitting with velocity `v`: one straight on, four turned 0.1 rad either way
/// about the shell's up and right axes (read from the original's code). PROVISIONAL: the pattern is not rolled with the shell's spin.
pub(super) fn nova_split(v: Vec3) -> [Vec3; 5] {
    let right = v.cross(Vec3::Y).normalize_or(Vec3::X);
    let up = right.cross(v).normalize_or(Vec3::Y);
    [(0.0, 0.0), (-0.1, -0.1), (0.1, -0.1), (-0.1, 0.1), (0.1, 0.1)].map(|(yaw, pitch)| Quat::from_axis_angle(up, yaw) * Quat::from_axis_angle(right, pitch) * v)
}

/// Spreads each blast and strikes what its ring passes, read from the original's code: every ship and base section, rock and shell
/// in flight is struck once, when the ring (radius MaxRadius x age / Duration) reaches its centre, for the bullet's damage roll for
/// the material times max(0.1, 1 - distance / MaxRadius). Ship sections take the difficulty factor and spill; no fires start. Nobody is
/// spared, the shooter's own ships included; star-class shells are immune. PROVISIONAL: the push the blast gives what is inside it
/// (the bullet's `Magnitude`) is not ported (its scale to ship motion is unmeasured); lifeboats are not struck.
#[allow(clippy::too_many_arguments)]
pub fn blasts(
    mut commands: Commands,
    time: Res<Time>,
    lib: Res<Library>,
    fxl: Res<FxLib>,
    mut rng: ResMut<Rng>,
    (mut hits, mut destroyed, mut sfx): (MessageWriter<Hit>, MessageWriter<Destroyed>, MessageWriter<Sfx>),
    q: Query<(Entity, &mut Blast, &Transform), (Without<Ship>, Without<Shell>)>,
    mut ships: Query<(Entity, &Transform, &Ship, &mut Health, Option<&crate::campaign::MissionShip>), Without<crate::lifeboats::Lifeboat>>,
    mut bases: Query<(Entity, &Transform, &ArmedBase, &mut Health), (Without<Ship>, Without<Blast>)>,
    mut rocks: Query<(Entity, &Transform, &mut Asteroid), (Without<Ship>, Without<Blast>)>,
    mut shells: Query<(Entity, &mut Shell, &Transform), (Without<Ship>, Without<Blast>)>,
) {
    let dt = time.delta_secs();
    let mut q = q;
    for (e, mut b, tf) in &mut q {
        let before = b.radius * (b.age / b.duration).min(1.0);
        b.age += dt;
        let now = b.radius * (b.age / b.duration).min(1.0);
        let (c, max) = (tf.translation, b.radius);
        let passed = |p: Vec3| {
            let r = p.distance(c);
            (r >= before && r < now && r < max).then(|| (1.0 - r / max).max(0.1))
        };
        let Some(bd) = lib.bullets.get(&b.bullet) else {
            commands.entity(e).despawn();
            continue;
        };
        let by = b.shooter.unwrap_or(e);
        for (se, stf, ship, mut hp, ms) in &mut ships {
            if hp.destroyed {
                continue;
            }
            let class = &lib.classes[ship.class];
            let scale = b.owner.map_or(1.0, |(side, base)| side_damage_scale(side, ship.side, crate::menu::difficulty(), base));
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
                base_parts_burst(&mut commands, &fxl, &mut sfx, &mut rng, class, btf, fallen);
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
                    hit_fx(&mut commands, &fxl, pd, ptf.translation, shell.vel);
                }
                commands.entity(pe).despawn();
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

/// Contact damage (collision.rs `Ram`, read from the original's code): one hit on the victim's section nearest the contact,
/// through the normal hit path, scaled by side and difficulty as shells are when a ship rammed it (rocks are not scaled).
/// The struck section's damage sound plays at the contact, once for the pair when both struck sections share a material.
pub fn ram_damage(
    lib: Res<Library>,
    mut rng: ResMut<Rng>,
    mut msgs: MessageReader<crate::collision::Ram>,
    mut ships: Query<(&Ship, &Transform, &mut Health, Option<&crate::campaign::MissionShip>)>,
    mut sfx: Option<MessageWriter<crate::audio::Sfx>>,
) {
    let mut played: Vec<(Vec3, &str)> = vec![];
    for r in msgs.read() {
        let rammer = ships.get(r.b).ok().map(|(s, ..)| s.side);
        let Ok((ship, tf, mut hp, ms)) = ships.get_mut(r.a) else { continue };
        if hp.destroyed {
            continue;
        }
        let class = &lib.classes[ship.class];
        let scale = rammer.map_or(1.0, |side| super::shells::side_damage_scale(side, ship.side, crate::menu::difficulty(), false));
        let Some(k) = crate::collision::nearest_section(class, tf, r.at).filter(|k| *k < hp.sections.len()) else { continue };
        let d = r.damage * scale;
        apply_ship_hit(class, &mut hp, k, [d, d, 0.0, 0.0, 0.0], ms.map_or(0.0, |m| m.invuln), false, &mut rng);
        let pool = match super::material_key(&class.sections[k].material) {
            "Wood" | "Reinforced" => "Wood",
            "Iron" => "Metal",
            "Cloth" => "Sail",
            _ => "Stone",
        };
        if let Some(sfx) = sfx.as_mut().filter(|_| rammer.is_some() && !played.contains(&(r.at, pool))) {
            played.push((r.at, pool));
            sfx.write(crate::audio::Sfx { name: format!("Damage_{pool}0{}", rng.variant(5)), at: Some(r.at), vol: 1.0, max: 600.0 });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nova_splits_into_one_straight_and_four_turned_secondaries() {
        let v = Vec3::new(30.0, 0.0, -40.0);
        let out = nova_split(v);
        assert_eq!(out[0], v);
        for w in &out[1..] {
            assert!((w.length() - 50.0).abs() < 1e-3);
            // Turned 0.1 rad about two perpendicular axes: about 0.141 rad off the straight one.
            assert!((w.angle_between(v) - 0.1414).abs() < 2e-3, "{}", w.angle_between(v));
        }
        assert!(out[1].y < 0.0 && out[4].y > 0.0, "two dip and two rise");
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn a_star_blast_strikes_every_section_once_as_its_ring_passes_and_spares_nobody() {
        let lib = Library::load(crate::assets_dir());
        let class = lib.index("Ship_Navy_Cutter");
        let ship = Ship {
            class,
            side: Side::Enemy,
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
            avoid: 1.0,
        };
        let data = Library::load(crate::assets_dir());
        let c = &data.classes[class];
        let bullet = "Bullet_StarMortar";
        let Payload::Blast { radius, .. } = lib.bullets[bullet].payload else { panic!("star mortar is a blast") };
        let damage = lib.bullets[bullet].damage.clone();
        let mut app = App::new();
        app.insert_resource(lib)
            .insert_resource(Rng(5))
            .init_resource::<Time>()
            .init_resource::<FxLib>()
            .add_message::<Hit>()
            .add_message::<Destroyed>()
            .add_message::<Sfx>()
            .add_systems(Update, blasts);
        let e = app.world_mut().spawn((Transform::default(), ship, Health::new(c))).id();
        // Fired by the ship's own side: it is struck all the same (same side: no difficulty factor).
        let at = Vec3::new(20.0, 0.0, 0.0);
        let blast = Blast { bullet: bullet.into(), shooter: None, owner: Some((Side::Enemy, false)), radius, duration: 3.0, age: 0.0 };
        app.world_mut().spawn((blast, Transform::from_translation(at)));
        for _ in 0..100 {
            app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(1.0 / 30.0));
            app.update();
        }
        let hp = &app.world().get::<Health>(e).unwrap().sections;
        for (k, sec) in c.sections.iter().enumerate() {
            let r = ((sec.min + sec.max) / 2.0).distance(at);
            let [lo, hi, ..] = damage[material_key(&sec.material)];
            let d = (1.0 - r / radius).max(0.1);
            let lost = sec.hp - hp[k];
            if r < radius {
                assert!(lost >= lo * d - 1e-3 && lost <= hi * d + 1e-3, "{}: lost {lost} at {r}, want {lo}-{hi} x {d}", sec.label);
            } else {
                assert_eq!(lost, 0.0, "{} is out of reach", sec.label);
            }
        }
    }

    #[test]
    #[ignore = "needs imported assets"]
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
            avoid: 1.0,
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
    #[ignore = "needs imported assets"]
    fn a_ram_hits_only_the_section_nearest_the_contact() {
        let lib = Library::load(crate::assets_dir());
        let mut app = App::new();
        let ship = |class, side| Ship {
            class,
            side,
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
            avoid: 1.0,
        };
        let [cutter, mow] = ["Ship_Navy_Cutter", "Ship_Navy_ManOWar"].map(|n| lib.index(n));
        // Two computer sides: no difficulty factor.
        let a = app.world_mut().spawn((ship(cutter, crate::Side(1)), Transform::default(), Health::new(&lib.classes[cutter]))).id();
        let b = app.world_mut().spawn((ship(mow, crate::Side(2)), Transform::default(), Health::new(&lib.classes[mow]))).id();
        let full = Health::new(&lib.classes[cutter]).sections;
        let s = &lib.classes[cutter].sections[0];
        let at = (s.min + s.max) * 0.5;
        // A Cutter rammed by an enemy Man-O-War at 9 units/s: 0.0004 x 40000 x 9 = 144 hit points.
        let damage = 0.0004 * lib.classes[mow].phys.mass * 9.0;
        app.insert_resource(lib).insert_resource(Rng(7)).add_message::<crate::collision::Ram>().add_systems(Update, ram_damage);
        app.world_mut().write_message(crate::collision::Ram { a, b, damage, at });
        app.update();
        let hp = &app.world().get::<Health>(a).unwrap().sections;
        assert_eq!(hp[0], (full[0] - damage).max(0.0), "the nearest section takes it all");
        // It kills the 120-point bow, which spills 0.4 of the hit to its neighbours only.
        let lib = app.world().resource::<Library>();
        let adjacent = &lib.classes[cutter].sections[0].adjacent;
        for n in 1..hp.len() {
            let want = if adjacent.contains(&n) { (full[n] - 0.4 * damage).max(0.0) } else { full[n] };
            assert!((hp[n] - want).abs() < 1e-3, "section {n}: {} vs {want}", hp[n]);
        }
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn a_hit_on_a_dead_section_spills_four_tenths_to_its_neighbours() {
        let lib = Library::load(crate::assets_dir());
        let c = lib.index("Ship_Navy_Cutter");
        let class = &lib.classes[c];
        let k = class.sections.iter().position(|s| s.label == "bow_wood").unwrap();
        let mut hp = Health::new(class);
        hp.sections[k] = 0.0;
        let full = hp.sections.clone();
        apply_ship_hit(class, &mut hp, k, [10.0, 10.0, 0.0, 0.0, 0.0], 0.0, false, &mut Rng(7));
        for (n, (now, was)) in hp.sections.iter().zip(&full).enumerate() {
            let want = if class.sections[k].adjacent.contains(&n) { was - 4.0 } else { *was };
            assert!((now - want).abs() < 1e-4, "{}: {now} vs {want}", class.sections[n].label);
        }
        assert!(!class.sections[k].adjacent.is_empty(), "the bow has neighbours");
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
