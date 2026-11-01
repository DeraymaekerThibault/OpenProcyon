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

/// Intensity the crew puts out per tick, spent on the strongest fire first (docs/re/effects.md): every crew member of every
/// post adds their leadership (0..1). An unmanned gunner, engineer or rigger post counts the first mates instead.
/// Retail saves of a default-crewed Cutter (12 hands rated 1) fit 1.1-1.36 per tick against the 1.2 this gives.
/// `posts`: the 0..1 leadership of each manned slot, `ROLES` order.
pub fn fire_fighting(posts: &[Vec<f32>; 8]) -> f32 {
    (0..8).map(|p| if posts[p].is_empty() && matches!(p, 0 | 4 | 5) { 2 } else { p }).map(|p| posts[p].iter().map(|v| v.clamp(0.0, 1.0)).sum::<f32>()).sum()
}

/// The 0..1 `skill` of every slot of post `role`: named crew first, then generic hands up to the class capacity.
/// PROVISIONAL: computer crews are rated 0.75 (fits the measured computer hull repair, reinforced +5 and iron +2 per tick);
/// the player's empty slots are generic hands (0.1). Every slot but the captain's and first mate's gains the ship's leadership
/// bonus (`crew_arms::leadership_bonus`, 0 for a ship without named crew).
pub fn crew_slots(e: Entity, ship: &Ship, class: &Class, posts: Option<&crate::crew_arms::CrewPosts>, role: usize, skill: &str) -> Vec<f32> {
    const COMPUTER: f32 = 0.75;
    const GENERIC: f32 = 0.1;
    let cap = crate::boarding::counts(&class.name)[role] as usize;
    let named: Vec<f32> = posts.map(|p| p.0[role].iter().map(|id| crate::crew_arms::ability(id, skill).unwrap_or(GENERIC)).collect()).unwrap_or_default();
    let fill = if ship.side == crate::Side::Player { GENERIC } else { COMPUTER };
    let bonus = if matches!(role, 1 | 2) { 0.0 } else { posts.map_or(0.0, |p| crate::crew_arms::leadership_bonus(e, p)) };
    named.iter().copied().chain(std::iter::repeat(fill)).take(cap.max(named.len())).map(|v| (v + bonus).min(1.0)).collect()
}

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
/// A piece's smoke trail stops after this long (the smoke already out fades on its own), so a tumbling piece does not smoulder
/// for its whole fall. Visual refinement, not retail.
pub const DEBRIS_SMOKE_TIME: f32 = 3.0;

/// The smoke trail effect of a debris piece.
#[derive(Component)]
pub struct DebrisSmoke(Entity);

/// Spawns section `sec`'s debris piece off a ship at `tf` moving at `ship_vel`; `roll` gives 0..1 numbers for the tumble.
pub fn spawn_debris(commands: &mut Commands, assets: &AssetServer, fxl: &FxLib, sec: &Section, tf: &Transform, ship_vel: Vec3, mut roll: impl FnMut() -> f32) {
    let Some((model, pivot)) = &sec.debris else { return };
    let at = Transform::from_translation(tf.transform_point(*pivot)).with_rotation(tf.rotation).with_scale(tf.scale);
    let axis = Vec3::new(roll() - 0.5, 0.0, roll() - 0.5).normalize_or(Vec3::X);
    let e = commands
        .spawn((Debris { vel: ship_vel, axis, spin: DEBRIS_SPIN * (0.5 + roll()), age: 0.0 }, Transient, at, Visibility::default()))
        .with_child((crate::data::scene(assets, model), Transform::from_translation(-*pivot)))
        .id();
    if let Some(name) = &sec.debris_fx
        && let Some(smoke) = fx::spawn(commands, fxl, name, Some(e), Transform::IDENTITY, at, false)
    {
        commands.entity(e).insert(DebrisSmoke(smoke));
    }
}

/// Debris pieces drift on with their ship's velocity, sink and tumble, then go.
pub fn debris(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut Transform, &mut Debris, Option<&DebrisSmoke>)>, mut smokes: Query<&mut fx::Fx>) {
    let dt = time.delta_secs().min(0.1);
    for (e, mut tf, mut d, smoke) in &mut q {
        d.age += dt;
        if d.age > DEBRIS_SMOKE_TIME
            && let Some(DebrisSmoke(s)) = smoke
        {
            if let Ok(mut fx) = smokes.get_mut(*s) {
                fx.emitting = false;
            }
            commands.entity(e).remove::<DebrisSmoke>();
        }
        if d.age > DEBRIS_LIFE {
            commands.entity(e).despawn();
            continue;
        }
        tf.translation += (d.vel + Vec3::NEG_Y * DEBRIS_FALL) * dt;
        tf.rotate(Quat::from_axis_angle(d.axis, d.spin * dt));
    }
}

/// Damage state of a section from its remaining share `r` of HP, as the original bands it: over 0.75 none, 0.5 up to 0.75 Light,
/// 0.25 up to 0.5 Medium, under 0.25 Heavy, none left Destroyed. Exactly 0.75 is in no band and falls to Heavy (decoded quirk).
fn damage_level(r: f32) -> u8 {
    if r <= 0.0 {
        4
    } else if r > 0.75 {
        0
    } else if (0.5..0.75).contains(&r) {
        1
    } else if (0.25..0.5).contains(&r) {
        2
    } else {
        3
    }
}

/// Smoke and section-loss effects of one hull or base: swaps each section's looping smoke when its damage state changes, and on
/// loss plays the one explosion sound and drops the debris piece.
#[allow(clippy::too_many_arguments)]
fn section_smoke(
    commands: &mut Commands,
    (assets, fxl): (&AssetServer, &FxLib),
    rng: &mut Rng,
    sfx: &mut MessageWriter<Sfx>,
    (e, tf, class, vel): (Entity, &Transform, &crate::data::Class, Vec3),
    explode: bool,
    h: &Health,
    d: &mut DamageFx,
) {
    const NAMES: [&str; 5] = ["", "Damaged Section_Light", "Damaged Section_Medium", "Damaged Section_Heavy", "Damaged Section_Destroyed"];
    for (i, sec) in class.sections.iter().enumerate() {
        let level = damage_level(h.sections[i] / sec.hp);
        let (cur, ent) = d.0[i];
        if level == cur {
            continue;
        }
        if level == 4 && cur < 4 {
            // The original plays its one `Explosion_Med` file where a section is lost (docs/re/sound.md), never a Damage_ file.
            sfx.write(Sfx { name: "Explosion_Med01".into(), at: Some(tf.translation), vol: 1.0, max: 6000.0 });
            spawn_debris(commands, assets, fxl, sec, tf, vel, || rng.f());
            if explode {
                // A ship section also bursts into one ShipExplosion_Medium at its own place (bases play their destruct effects instead).
                let at = Transform::from_translation(tf.transform_point((sec.min + sec.max) / 2.0));
                fx::spawn(commands, fxl, "ShipExplosion_Medium", None, at, at, true);
            }
        }
        if let Some(old) = ent
            && let Ok(mut c) = commands.get_entity(old)
        {
            c.despawn();
        }
        let local = Transform::from_translation((sec.min + sec.max) / 2.0);
        let new = if level > 0 {
            fx::spawn(commands, fxl, NAMES[level as usize], Some(e), local, *tf * local, false).inspect(|&en| {
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

#[allow(clippy::too_many_arguments)]
pub fn damage_fx(
    mut commands: Commands,
    lib: Res<Library>,
    fxl: Res<FxLib>,
    assets: Res<AssetServer>,
    mut rng: Local<Rng>,
    mut sfx: MessageWriter<Sfx>,
    mut ships: Query<(Entity, &Transform, &Ship, &Health, &mut DamageFx)>,
    mut bases: Query<(Entity, &Transform, &super::ArmedBase, &Health, &mut DamageFx), Without<Ship>>,
) {
    for (e, tf, ship, h, mut d) in &mut ships {
        // A destroyed ship's smoke goes at once: it would otherwise ride on the tumbling breakup pieces (owner's call).
        if h.destroyed {
            for (_, effect) in &mut d.0 {
                if let Some(effect) = effect.take() {
                    commands.entity(effect).try_despawn();
                }
            }
            continue;
        }
        let vel = tf.rotation * Vec3::NEG_Z * ship.speed;
        section_smoke(&mut commands, (&assets, &fxl), &mut rng, &mut sfx, (e, tf, &lib.classes[ship.class], vel), true, h, &mut d);
    }
    // Bases lose sections the same way; no sight test gates it (decoded: only the effect manager's distance cull).
    for (e, tf, base, h, mut d) in &mut bases {
        section_smoke(&mut commands, (&assets, &fxl), &mut rng, &mut sfx, (e, tf, &lib.classes[base.class], Vec3::ZERO), false, h, &mut d);
    }
}

/// Fire update every FIRE_TICK s, in the original's order: crew puts out the strongest fires first, burning fires damage
/// their section by their intensity, fires on destroyed sections die, dead fires fade out (3 s) then go (5 s), burning
/// fires double. Effect size follows intensity (full at 20, floor 10%).
pub fn fires(
    mut commands: Commands,
    time: Res<Time>,
    lib: Res<Library>,
    mut ships: Query<(Entity, &Transform, &Ship, &mut Health, &mut Fires, Option<&crate::crew_arms::CrewPosts>)>,
    mut fxq: Query<&mut fx::Fx>,
) {
    let dt = time.delta_secs();
    for (me, _, ship, mut hp, mut fires, posts) in &mut ships {
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
            let class = &lib.classes[ship.class];
            let mut crew = fire_fighting(&std::array::from_fn(|role| crew_slots(me, ship, class, posts, role, "leadership")));
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
            for f in fires.list.iter().filter(|f| f.state == 0) {
                let was = hp.sections[f.sec];
                hp.sections[f.sec] = (was - f.intensity).max(0.0);
                if was > 0.0 && hp.sections[f.sec] == 0.0 {
                    let sec = &class.sections[f.sec];
                    if sec.vital {
                        hp.core -= core_bleed(class, &hp, f.intensity); // fire goes through the same damage path
                    }
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
pub fn swap_meshes(
    lib: Res<Library>,
    ships: Query<(Entity, &Ship, &Health), Without<super::Sinking>>,
    bases: Query<(Entity, &super::ArmedBase, &Health), Without<Ship>>,
    children: Query<&Children>,
    mut nodes: Query<(&Name, &mut Visibility)>,
) {
    // A breaking-up ship's section nodes belong to `deaths` until it is gone. Armed bases swap their destroyed parts the same way.
    for (e, class, h) in ships.iter().map(|(e, s, h)| (e, s.class, h)).chain(bases.iter().map(|(e, b, h)| (e, b.class, h))) {
        let class = &lib.classes[class];
        for d in children.iter_descendants(e) {
            let Ok((name, mut vis)) = nodes.get_mut(d) else { continue };
            let n = name.as_str();
            let (base, damaged) = match n.split_once("Damage_") {
                Some((a, b)) => (format!("{a}_{b}"), true),
                None => (n.to_string(), false),
            };
            // A damaged mesh of a layer that is no section can never be shown (it would sit on top of the intact one).
            let want = match class.sections.iter().position(|s| s.layer.eq_ignore_ascii_case(&base)) {
                Some(i) if (h.sections[i] <= 0.0) == damaged => Visibility::Inherited,
                None if !damaged => continue,
                _ => Visibility::Hidden,
            };
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

pub fn crew_repair(
    time: Res<Time>,
    lib: Res<Library>,
    mut rng: ResMut<Rng>,
    mut ships: Query<(Entity, &Ship, &mut Health, Option<&crate::crew_arms::CrewPosts>)>,
    mut acc: Local<f32>,
) {
    *acc += time.delta_secs();
    if *acc < CREW_REPAIR_TICK {
        return;
    }
    *acc -= CREW_REPAIR_TICK;
    for (e, ship, mut h, posts) in &mut ships {
        if h.destroyed {
            continue;
        }
        let class = &lib.classes[ship.class];
        let (engineers, riggers) = (crew_slots(e, ship, class, posts, 4, "engineering"), crew_slots(e, ship, class, posts, 5, "rigging"));
        damage_control(class, &mut h.sections, &engineers, &riggers, &mut || rng.f());
    }
}

#[cfg(test)]
mod swap_tests {
    use super::*;
    use crate::combat::{ArmedBase, Health};

    #[test]
    #[ignore = "needs imported assets"]
    fn a_destroyed_base_part_shows_its_damaged_mesh() {
        let lib = Library::load(crate::assets_dir());
        let class = lib.index("Base_Procyon1");
        let mut app = App::new();
        app.insert_resource(Library::load(crate::assets_dir())).add_systems(Update, swap_meshes);
        let mut hp = Health::new(&lib.classes[class]);
        hp.sections[0] = 0.0; // BaseProcyons_Layer2
        let node = |app: &mut App, parent, name: &str| app.world_mut().spawn((Name::new(name.to_string()), Visibility::default(), ChildOf(parent))).id();
        let base = app.world_mut().spawn((ArmedBase { class }, hp)).id();
        let [whole, hurt, other, other_hurt] =
            ["BaseProcyons_Layer2", "BaseProcyonsDamage_Layer2", "BaseProcyons_Layer3", "BaseProcyonsDamage_Layer3"].map(|n| node(&mut app, base, n));
        let stray = node(&mut app, base, "BaseProcyonsDamage_Layer1");
        app.update();
        let vis = |app: &App, e| *app.world().get::<Visibility>(e).unwrap();
        assert_eq!([vis(&app, whole), vis(&app, hurt)], [Visibility::Hidden, Visibility::Inherited]);
        assert_eq!([vis(&app, other), vis(&app, other_hurt)], [Visibility::Inherited, Visibility::Hidden]);
        assert_eq!(vis(&app, stray), Visibility::Hidden);
    }
}

#[cfg(test)]
mod level_tests {
    use super::damage_level;

    #[test]
    fn smoke_bands_follow_the_originals_remaining_share() {
        let levels = [1.0, 0.8, 0.7, 0.5, 0.4, 0.25, 0.1, 0.0].map(damage_level);
        assert_eq!(levels, [0, 0, 1, 1, 2, 2, 3, 4]);
        assert_eq!(damage_level(0.75), 3, "exactly 0.75 is in no band and falls to Heavy");
    }
}
