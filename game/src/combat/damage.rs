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

/// Fires burning on a ship (docs/re/effects.md).
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

/// Intensity the crew puts out per tick, spent on the strongest fire first. Fitted on a retail save pair (uncrewed Navy Cutter,
/// Easy): a fire went 11.2 -> 273.8 over five ticks, i.e. 2 x (i - c) with c = 1.36; another fell 8 -> 6.9 in one tick.
/// PROVISIONAL: the crew-skill/size dependence is unknown (docs/re/effects.md).
pub const FIRE_FIGHTING: f32 = 1.36;

pub fn fire_scale(i: f32) -> f32 {
    if i > FIRE_FULL { 1.0 } else { (i / FIRE_FULL).max(0.1) }
}

/// A piece broken off a destroyed section (WOT `debrisMeshScene`), tumbling away trailing its `debrisFx` (docs/re/effects.md
/// "Section debris"). The entity sits at the piece's pivot; its model child is offset back into hull space.
#[derive(Component)]
pub struct Debris {
    pub vel: Vec3,
    pub axis: Vec3,
    pub spin: f32,
    pub age: f32,
}

/// PROVISIONAL (retail frames only: a torn-off mast still drifting and turning 6+ s later): sink rate, tumble rate and life.
pub const DEBRIS_FALL: f32 = 1.5;
pub const DEBRIS_SPIN: f32 = 0.5;
pub const DEBRIS_LIFE: f32 = 10.0;

/// Spawns section `sec`'s debris piece off a ship at `tf` moving at `ship_vel`; `roll` gives 0..1 numbers for the tumble.
pub fn spawn_debris(commands: &mut Commands, assets: &AssetServer, fxl: &FxLib, sec: &Section, tf: &Transform, ship_vel: Vec3, mut roll: impl FnMut() -> f32) {
    let Some((model, pivot)) = &sec.debris else { return };
    let at = Transform::from_translation(tf.transform_point(*pivot)).with_rotation(tf.rotation).with_scale(tf.scale);
    let axis = Vec3::new(roll() - 0.5, 0.0, roll() - 0.5).normalize_or(Vec3::X);
    let e = commands
        .spawn((Debris { vel: ship_vel, axis, spin: DEBRIS_SPIN * (0.5 + roll()), age: 0.0 }, Transient, at, Visibility::default()))
        .with_child((WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(crate::data::model(model)))), Transform::from_translation(-*pivot)))
        .id();
    if let Some(name) = &sec.debris_fx {
        fx::spawn(commands, fxl, name, Some(e), Transform::IDENTITY, at, false);
    }
}

/// Debris pieces drift on with their ship's velocity, sink and tumble, then go.
pub fn debris(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut Transform, &mut Debris)>) {
    let dt = time.delta_secs().min(crate::MAX_SIM_STEP);
    for (e, mut tf, mut d) in &mut q {
        d.age += dt;
        if d.age > DEBRIS_LIFE {
            commands.entity(e).despawn();
            continue;
        }
        tf.translation += (d.vel + Vec3::NEG_Y * DEBRIS_FALL) * dt;
        tf.rotate(Quat::from_axis_angle(d.axis, d.spin * dt));
    }
}

/// PROVISIONAL thresholds: lost HP fraction under 25% none, then Light, Medium, Heavy at 25/50/75%, Destroyed at 0 HP.
#[allow(clippy::too_many_arguments)]
pub fn damage_fx(
    mut commands: Commands,
    lib: Res<Library>,
    fxl: Res<FxLib>,
    assets: Res<AssetServer>,
    mut rng: ResMut<Rng>,
    mut ships: Query<(Entity, &Transform, &Ship, &Health, &mut DamageFx, Option<&Sinking>)>,
) {
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
            if level == 4 && cur < 4 {
                let vel = tf.rotation * Vec3::NEG_Z * ship.speed;
                spawn_debris(&mut commands, &assets, &fxl, sec, tf, vel, || rng.f());
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

/// Fire update every FIRE_TICK s, in the original's order: crew puts out the strongest fires first, burning fires damage
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

/// Retail: a destroyed swappable section swaps its mesh "<A>_<B>" for "<A>Damage_<B>".
/// The GLB carries both; show whichever matches the section state. Runs every frame so freshly loaded scenes are covered.
pub fn swap_meshes(lib: Res<Library>, ships: Query<(Entity, &Ship, &Health), Without<super::Sinking>>, children: Query<&Children>, mut nodes: Query<(&Name, &mut Visibility)>) {
    // A breaking-up ship's section nodes belong to `deaths` until it is gone.
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

/// Crew damage control (every 12 s, docs/re/refit-resupply.md#crew-damage-control): each tick the most damaged living sail
/// gets one roll per rigger slot (chance = that rigger's rigging, 0..1), +3 hit points per success; the most damaged
/// living non-sail section gets one roll with chance c = the engineers' mean engineering and, on success,
/// floor(10 x c x material factor) hit points. Never above the maximum; destroyed (0 HP) sections stay down.
pub const CREW_REPAIR_TICK: f32 = 12.0;

/// Hull repair factor per section material (the retail per-material table; Stone and others mend nothing). PROVISIONAL:
/// Wood, Reinforced and Iron are confirmed by measurement, the material order of the rest of the table is assumed.
fn repair_factor(material: &str) -> f32 {
    if material.contains("ReInforced") || material.contains("Reinforced") {
        0.67
    } else if material.contains("Iron") {
        0.33
    } else if material.contains("Wood") {
        1.0
    } else {
        0.0
    }
}

/// One damage-control tick on `sections`. `engineers` / `riggers`: the 0..1 skill of each slot of those posts; `roll`
/// yields uniform numbers in [0, 1).
pub fn damage_control(class: &Class, sections: &mut [f32], engineers: &[f32], riggers: &[f32], roll: &mut dyn FnMut() -> f32) {
    let most_damaged = |sections: &[f32], sail: bool| {
        (0..sections.len())
            .filter(|&i| class.sections[i].material.contains("Cloth") == sail && sections[i] > 0.0 && sections[i] < class.sections[i].hp)
            .min_by(|&a, &b| (sections[a] / class.sections[a].hp).total_cmp(&(sections[b] / class.sections[b].hp)))
    };
    if let Some(i) = most_damaged(sections, true) {
        for &p in riggers {
            if roll() < p.clamp(0.0, 1.0) {
                sections[i] = (sections[i] + 3.0).min(class.sections[i].hp);
            }
        }
    }
    let c = if engineers.is_empty() { 0.0 } else { engineers.iter().map(|e| e.clamp(0.0, 1.0)).sum::<f32>() / engineers.len() as f32 };
    if let Some(i) = most_damaged(sections, false)
        && roll() < c
    {
        sections[i] = (sections[i] + (10.0 * c * repair_factor(&class.sections[i].material)).floor()).min(class.sections[i].hp);
    }
}

pub fn crew_repair(time: Res<Time>, lib: Res<Library>, mut rng: ResMut<Rng>, mut ships: Query<(&Ship, &mut Health, Option<&crate::crew_arms::CrewPosts>)>, mut acc: Local<f32>) {
    // PROVISIONAL: computer crews are rated 0.75 (fits the measured computer hull repair, reinforced +5 and iron +2 per tick);
    // the player's empty slots are generic hands (0.1). The captain/first-mate leadership bonuses (+0.1 / +0.05 above a
    // threshold) are not modelled: their thresholds are not known.
    const COMPUTER: f32 = 0.75;
    const GENERIC: f32 = 0.1;
    *acc += time.delta_secs();
    if *acc < CREW_REPAIR_TICK {
        return;
    }
    *acc -= CREW_REPAIR_TICK;
    for (ship, mut h, posts) in &mut ships {
        if h.destroyed {
            continue;
        }
        let class = &lib.classes[ship.class];
        let caps = crate::boarding::counts(&class.name);
        let slots = |role: usize, skill: &str| -> Vec<f32> {
            let named: Vec<f32> = posts.map(|p| p.0[role].iter().map(|id| crate::crew_arms::ability(id, skill).unwrap_or(GENERIC)).collect()).unwrap_or_default();
            let fill = if ship.side == crate::Side::Player { GENERIC } else { COMPUTER };
            named.iter().copied().chain(std::iter::repeat(fill)).take((caps[role] as usize).max(named.len())).collect()
        };
        let (engineers, riggers) = (slots(4, "engineering"), slots(5, "rigging"));
        damage_control(class, &mut h.sections, &engineers, &riggers, &mut || rng.f());
    }
}
