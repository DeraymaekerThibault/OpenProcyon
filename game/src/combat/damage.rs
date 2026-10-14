//! Damage presentation: smoke/fire emitters, shipboard fires and swapped damage meshes.
use super::*;

/// Per damage section: current smoke/fire level (0 none .. 4 destroyed) and its effect entity.
#[derive(Component)]
pub struct DamageFx(Vec<(u8, Option<Entity>)>);

impl DamageFx {
    pub fn new(c: &Class) -> Self {
        DamageFx(vec![(0, None); c.sections.len()])
    }
}

/// Fires burning on a ship (exe FireKeeper; see docs/re/effects.md).
#[derive(Component, Default)]
pub struct Fires {
    pub list: Vec<Fire>,
    pub tick: f32,
}

pub struct Fire {
    pub sec: usize,
    pub intensity: f32,
    pub state: u8,
    pub t: f32,
    pub fx: Option<Entity>,
}

/// Fire tick (s), growth per tick, intensity at which the effect is full size, burn-out delays after the fire dies (s).
pub const FIRE_TICK: f32 = 6.0;

pub const FIRE_GROWTH: f32 = 2.0;

pub const FIRE_FULL: f32 = 20.0;

pub const FIRE_FADE: [f32; 2] = [3.0, 5.0];

/// PROVISIONAL: intensity the crew puts out per tick. The exe asks the ship's IFlammable interface (crew-skill driven; not decoded).
pub const FIRE_FIGHTING: f32 = 0.5;

pub fn fire_scale(i: f32) -> f32 {
    if i > FIRE_FULL { 1.0 } else { (i / FIRE_FULL).max(0.1) }
}

/// PROVISIONAL thresholds: lost HP fraction under 25% none, then Light, Medium, Heavy at 25/50/75%, Destroyed at 0 HP.
pub fn damage_fx(mut commands: Commands, lib: Res<Library>, fxl: Res<FxLib>, mut ships: Query<(Entity, &Transform, &Ship, &Health, &mut DamageFx, Option<&Sinking>)>) {
    const NAMES: [&str; 5] = ["", "Damaged Section_Light", "Damaged Section_Medium", "Damaged Section_Heavy", "Damaged Section_Destroyed"];
    for (e, tf, ship, h, mut d, sinking) in &mut ships {
        if h.destroyed {
            if sinking.is_none() {
                for (_, effect) in &mut d.0 {
                    if let Some(effect) = effect.take() {
                        commands.entity(effect).try_despawn();
                    }
                }
            }
            continue;
        }
        let class = &lib.classes[ship.class];
        for (i, sec) in class.sections.iter().enumerate() {
            let lost = 1.0 - h.sections[i] / sec.hp;
            let level = if h.sections[i] <= 0.0 {
                4
            } else if lost >= 0.75 {
                3
            } else if lost >= 0.5 {
                2
            } else if lost >= 0.25 {
                1
            } else {
                0
            };
            let (cur, ent) = d.0[i];
            if level == cur {
                continue;
            }
            if let Some(old) = ent
                && let Ok(mut c) = commands.get_entity(old)
            {
                c.despawn();
            }
            let local = Transform::from_translation((sec.min + sec.max) / 2.0);
            let new = if level > 0 {
                fx::spawn(&mut commands, &fxl, NAMES[level as usize], Some(e), local, *tf * local, false).inspect(|&en| {
                    let half = (sec.max - sec.min) / 2.0;
                    commands.entity(en).queue(move |mut w: EntityWorldMut| {
                        if let Some(mut f) = w.get_mut::<fx::Fx>() {
                            f.src_half = half;
                        }
                    });
                })
            } else {
                None
            };
            d.0[i] = (level, new);
        }
    }
}

/// Fire update every FIRE_TICK s, in the exe's order: crew puts out the strongest fires first, burning fires damage
/// their section by their intensity, fires on destroyed sections die, dead fires fade out (3 s) then go (5 s), burning
/// fires double. Effect size follows intensity (full at 20, floor 10%).
pub fn fires(
    mut commands: Commands,
    time: Res<Time>,
    lib: Res<Library>,
    mut rng: ResMut<Rng>,
    mut sfx: MessageWriter<Sfx>,
    mut ships: Query<(&Transform, &Ship, &mut Health, &mut Fires)>,
    mut fxq: Query<&mut fx::Fx>,
) {
    let dt = time.delta_secs();
    for (tf, ship, mut hp, mut fires) in &mut ships {
        let fires = &mut *fires;
        if hp.destroyed {
            for f in fires.list.drain(..) {
                if let Some(e) = f.fx
                    && let Ok(mut c) = commands.get_entity(e)
                {
                    c.despawn();
                }
            }
            continue;
        }
        for f in fires.list.iter_mut().filter(|f| f.state > 0) {
            f.t += dt;
        }
        fires.tick += dt;
        let tick = fires.tick >= FIRE_TICK && !fires.list.is_empty();
        if tick {
            fires.tick = 0.0;
            let mut crew = FIRE_FIGHTING;
            while crew > 0.0 {
                let Some(f) = fires.list.iter_mut().filter(|f| f.state == 0).max_by(|a, b| a.intensity.total_cmp(&b.intensity)) else { break };
                let used = crew.min(f.intensity);
                f.intensity -= used;
                crew -= used;
                if f.intensity <= 0.0 {
                    f.state = 1;
                    f.t = 0.0;
                }
            }
            let class = &lib.classes[ship.class];
            for f in fires.list.iter().filter(|f| f.state == 0) {
                let was = hp.sections[f.sec];
                hp.sections[f.sec] = (was - f.intensity).max(0.0);
                if was > 0.0 && hp.sections[f.sec] == 0.0 {
                    let sec = &class.sections[f.sec];
                    if sec.vital {
                        hp.core -= core_bleed(class, &hp, f.intensity); // fire goes through the same damage path
                    }
                    sfx.write(Sfx { name: format!("Damage_Wood0{}", 1 + (rng.f() * 5.0) as u32 % 5), at: Some(tf.translation), vol: 1.0, max: 600.0 });
                    info!("{} of {} burnt out", sec.label, class.name);
                }
            }
        }
        for f in &mut fires.list {
            if f.state == 0 && hp.sections[f.sec] <= 0.0 {
                f.state = 1;
                f.t = 0.0;
            }
            if f.state == 1 && f.t >= FIRE_FADE[0] {
                f.state = 2;
                f.t = 0.0;
                if let Some(Ok(mut x)) = f.fx.map(|e| fxq.get_mut(e)) {
                    x.emitting = false;
                }
            }
            if f.state == 2 && f.t >= FIRE_FADE[1] {
                f.state = 3;
            }
            if tick && f.state == 0 {
                f.intensity *= FIRE_GROWTH;
                if let Some(Ok(mut x)) = f.fx.map(|e| fxq.get_mut(e)) {
                    x.rate_scale = fire_scale(f.intensity);
                }
            }
            if f.state == 1
                && let Some(Ok(mut x)) = f.fx.map(|e| fxq.get_mut(e))
            {
                x.emitting = false;
            }
        }
        fires.list.retain(|f| {
            let done = f.state == 3;
            if done
                && let Some(e) = f.fx
                && let Ok(mut c) = commands.get_entity(e)
            {
                c.despawn();
            }
            !done
        });
    }
}

/// Exe: a destroyed swappable section swaps its mesh "<A>_<B>" for "<A>Damage_<B>".
/// The GLB carries both; show whichever matches the section state. Runs every frame so freshly loaded scenes are covered.
pub fn swap_meshes(lib: Res<Library>, ships: Query<(Entity, &Ship, &Health)>, children: Query<&Children>, mut nodes: Query<(&Name, &mut Visibility)>) {
    for (e, ship, h) in &ships {
        let class = &lib.classes[ship.class];
        for d in children.iter_descendants(e) {
            let Ok((name, mut vis)) = nodes.get_mut(d) else { continue };
            let n = name.as_str();
            let (base, damaged) = match n.split_once("Damage_") {
                Some((a, b)) => (format!("{a}_{b}"), true),
                None => (n.to_string(), false),
            };
            let Some(i) = class.sections.iter().position(|s| s.layer.eq_ignore_ascii_case(&base)) else { continue };
            let want = if (h.sections[i] <= 0.0) == damaged { Visibility::Inherited } else { Visibility::Hidden };
            if *vis != want {
                *vis = want;
            }
        }
    }
}

/// Crew damage control (every 12 s, docs/re/refit-resupply.md §3.4): damaged (not destroyed) sections slowly mend.
/// PROVISIONAL amount and odds: the exe scales a chance by the crew's engineering skill and adds an amount not yet decoded; here
/// each tick has a 75% chance to restore 5% of the most damaged living section's hit points.
pub const CREW_REPAIR_TICK: f32 = 12.0;

pub fn crew_repair(
    time: Res<Time>,
    lib: Res<Library>,
    mut rng: ResMut<Rng>,
    points: Option<Res<crate::campaign::CampaignPoints>>,
    mut ships: Query<(&Ship, &mut Health, Option<&crate::crew_arms::CrewPosts>)>,
    mut acc: Local<f32>,
) {
    // The human fleet's odds follow the roster's best engineer (0.5 .. 1.0); others keep 75%.
    let engineer = points.as_deref().and_then(|p| crate::crew_arms::best(p, "engineering")).map(|e| 0.5 + 0.5 * e);
    *acc += time.delta_secs();
    if *acc < CREW_REPAIR_TICK {
        return;
    }
    *acc -= CREW_REPAIR_TICK;
    for (ship, mut h, posts) in &mut ships {
        let odds = if ship.side == crate::Side::Player { posts.map_or(engineer.unwrap_or(0.75), |p| p.skill(4, "engineering").map_or(0.75, |e| 0.5 + 0.5 * e)) } else { 0.75 };
        if h.destroyed || rng.f() > odds {
            continue;
        }
        let class = &lib.classes[ship.class];
        let worst = h
            .sections
            .iter()
            .zip(&class.sections)
            .enumerate()
            .filter(|(_, (hp, s))| **hp > 0.0 && **hp < s.hp)
            .min_by(|a, b| (a.1.0 / a.1.1.hp).total_cmp(&(b.1.0 / b.1.1.hp)))
            .map(|(i, _)| i);
        if let Some(i) = worst {
            h.sections[i] = (h.sections[i] + class.sections[i].hp * 0.05).min(class.sections[i].hp);
        }
    }
}
