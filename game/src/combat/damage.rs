//! Damage presentation: smoke/fire emitters, shipboard fires and swapped damage meshes.
use super::*;

/// Per damage section: current smoke/fire level (0 none .. 4 destroyed) and its effect entity.
#[derive(Component)]
pub struct DamageFx(pub(super) Vec<(u8, Option<Entity>)>);

impl DamageFx {
    pub fn new(c: &Class) -> Self {
        DamageFx(vec![(0, None); c.sections.len()])
    }

    /// The sections not yet lost (damage level below destroyed): the ones the death wipe still takes down.
    pub fn standing(&self) -> Vec<usize> {
        self.0.iter().enumerate().filter(|(_, (level, _))| *level < 4).map(|(i, _)| i).collect()
    }
}

/// The death wipe (docs/re/effects.md "Death wipe"): 0.14 s after the kill every section is hit for all it has left. Each section that was
/// still standing is lost then: the one explosion sound and its debris piece (a section that dropped its piece earlier has none left), but
/// no explosion burst. The speech lines the original adds for the player's own flagship are not built.
#[allow(clippy::too_many_arguments)]
pub fn death_wipe(
    commands: &mut Commands,
    (assets, fxl): (&AssetServer, &FxLib),
    rng: &mut Rng,
    sfx: &mut MessageWriter<Sfx>,
    (tf, class, vel): (&Transform, &crate::data::Class, Vec3),
    d: &DamageFx,
) {
    for i in d.standing() {
        sfx.write(Sfx { name: "Explosion_Med01".into(), at: Some(tf.translation), vol: 1.0, max: 6000.0 });
        spawn_debris(commands, assets, fxl, &class.sections[i], tf, vel, || rng.f());
    }
}

/// Fires burning on a ship (docs/re/effects.md).
#[derive(Component, Default)]
pub struct Fires {
    pub list: Vec<Fire>,
    pub tick: f32,
}

/// A fire's life, changed only on the fire ticks: burning; out (found dead, stops emitting); cut loose from the ship; gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FireState {
    Burning,
    Out,
    Loose,
    Done,
}

pub struct Fire {
    pub sec: usize,
    pub intensity: f32,
    pub state: FireState,
    pub fx: Option<Entity>,
}

/// Fire tick (s), growth per tick, intensity at which the effect is full size.
pub const FIRE_TICK: f32 = 6.0;

pub const FIRE_GROWTH: f32 = 2.0;

pub const FIRE_FULL: f32 = 20.0;

/// Intensity the crew puts out per tick, spent on the strongest fire first (docs/re/effects.md): every crew member of every
/// post adds their leadership (0..1). An unmanned gunner, engineer or rigger post counts the first mates instead.
/// Retail saves of a default-crewed Cutter (12 hands rated 1) fit 1.1-1.36 per tick against the 1.2 this gives.
/// `posts`: the 0..1 leadership of each manned slot, `ROLES` order.
pub fn fire_fighting(posts: &[Vec<f32>; 8]) -> f32 {
    (0..8).map(|p| if posts[p].is_empty() && matches!(p, 0 | 4 | 5) { 2 } else { p }).map(|p| posts[p].iter().map(|v| v.clamp(0.0, 1.0)).sum::<f32>()).sum()
}

/// The 0..1 `skill` of every slot of post `role`: named crew first, then generic hands up to the class capacity.
/// Every ship uses its own crew, computer or not: `posts` is its named crew, or its level's generic crew for a placed ship
/// (`crew_arms::fallback_posts`); an empty slot is a rating-1 hand (0.1). Every slot but the captain's and first mate's gains the
/// ship's leadership bonus (`crew_arms::leadership_bonus`, 0 for a ship without crew).
pub fn crew_slots(key: crate::scenario::SpawnIndex, class: &Class, posts: Option<&crate::crew_arms::CrewPosts>, role: usize, skill: &str) -> Vec<f32> {
    const GENERIC: f32 = 0.1;
    let cap = crate::boarding::counts(&class.name)[role] as usize;
    let named: Vec<f32> = posts.map(|p| p.0[role].iter().map(|id| crate::crew_arms::ability(id, skill).unwrap_or(GENERIC)).collect()).unwrap_or_default();
    let bonus = if matches!(role, 1 | 2) { 0.0 } else { posts.map_or(0.0, |p| crate::crew_arms::leadership_bonus(key, p)) };
    named.iter().copied().chain(std::iter::repeat(GENERIC)).take(cap.max(named.len())).map(|v| (v + bonus).min(1.0)).collect()
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
/// for its whole fall. Not retail: a visual refinement.
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
        // Decoded: the effect sits at the hull origin and takes its particle positions from the section mesh's vertices.
        let new = if level > 0 {
            fx::spawn(commands, fxl, NAMES[level as usize], Some(e), Transform::IDENTITY, *tf, false).inspect(|&en| {
                let vertices: Vec<Vec3> = sec.tris.iter().flatten().copied().collect();
                commands.entity(en).queue(move |mut w: EntityWorldMut| {
                    if let Some(mut f) = w.get_mut::<fx::Fx>() {
                        f.vertices = vertices;
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

/// The rules' "Damage Group/Unit by X percent" (not yet measured): every section in order is hit once for
/// round(its current hit points x `fraction`) through the normal hit routine (spill into neighbours of a section left at 0, core bleed
/// through dead vital sections, the human's ships taking the difficulty factor: the hit carries no attacker, so computer ships take
/// none). The core is never scaled directly: a fraction below 1 leaves it whole unless vital sections die.
pub fn fraction_hit(class: &Class, hp: &mut Health, fraction: f32, humans_own_ship: bool, invuln: f32, difficulty: u8) {
    let scale = super::shells::side_damage_scale(false, humans_own_ship, difficulty, false);
    let mut rng = Rng::default();
    for k in 0..hp.sections.len() {
        let d = (hp.sections[k].round_ties_even() * fraction * scale).round_ties_even();
        super::shells::apply_ship_hit(class, hp, k, [d, d, 0.0, 0.0, 0.0], invuln, false, &mut rng);
    }
}

/// Damage factor on the human player's own ships per difficulty (Easy, Normal, Hard): shells of the other side and fire.
pub(super) const INCOMING_BY_DIFFICULTY: [f32; 3] = [0.25, 0.5, 1.0];
/// Damage factor of the player's shells on computer ships per difficulty (Easy, Normal, Hard).
pub(super) const OUTGOING_BY_DIFFICULTY: [f32; 3] = [3.0, 2.0, 1.0];

/// What a burning fire takes from its section on a fire tick. In the original a fire hurts its section through the ordinary hit
/// routine, which scales by the victim's owner: on the human player's own ships by the incoming difficulty factor (Easy x0.25,
/// Normal x0.5, Hard x1), on any other ship not at all (a fire has no attacker, and only the player's own hits are scaled up).
/// Hit points are whole numbers, rounded to the nearest (half to even): a fire of 1.0 takes nothing on Normal (measured on the
/// original: eight times of eight).
pub fn fire_damage(intensity: f32, humans_own_ship: bool, difficulty: u8) -> f32 {
    let scale = if humans_own_ship { INCOMING_BY_DIFFICULTY[difficulty.min(2) as usize] } else { 1.0 };
    (intensity * scale).round_ties_even()
}

/// Fire update every FIRE_TICK s, in the original's order: crew puts out the strongest fires first, burning fires damage
/// their section by their intensity, fires on destroyed sections die, dead fires fade out (3 s) then go (5 s), burning
/// fires double. Effect size follows intensity (full at 20, floor 10%).
pub fn fires(
    spawn_order: Query<&crate::scenario::SpawnIndex>,
    difficulty: Res<crate::scenario::Difficulty>,
    mut commands: Commands,
    time: Res<Time>,
    lib: Res<Library>,
    mut ships: Query<(
        Entity,
        &Transform,
        &Ship,
        &mut Health,
        &mut Fires,
        Option<&crate::crew_arms::CrewPosts>,
        Option<&crate::campaign::MissionShip>,
        Option<&crate::ship::FleetMember>,
        Option<&crate::boarding::Boardable>,
    )>,
    mut fxq: Query<&mut fx::Fx>,
    (fleets, table, players): (Option<Res<crate::ship::Fleets>>, crate::alliance::Hostility, Option<Res<crate::commands::Players>>),
) {
    let dt = time.delta_secs();
    for (me, _, ship, mut hp, mut fires, posts, mission, member, boarding) in &mut ships {
        let humans = crate::ship::is_human_ship(ship.side, member, players.as_deref(), fleets.as_deref());
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
        fires.tick += dt;
        let tick = fires.tick >= FIRE_TICK && !fires.list.is_empty();
        if tick {
            fires.tick = 0.0;
            let class = &lib.classes[ship.class];
            let generic = posts.is_none().then(|| crate::crew_arms::fallback_posts(table.with_human(ship.side), boarding, &class.name)).flatten();
            let posts = posts.or(generic.as_ref());
            let mut crew = fire_fighting(&std::array::from_fn(|role| crew_slots(crate::scenario::SpawnIndex::of(&spawn_order, me), class, posts, role, "leadership")));
            while crew > 0.0 {
                let Some(f) = fires.list.iter_mut().filter(|f| f.state == FireState::Burning && f.intensity > 0.0).max_by(|a, b| a.intensity.total_cmp(&b.intensity)) else {
                    break;
                };
                let used = crew.min(f.intensity);
                f.intensity -= used;
                crew -= used;
            }
            let (core, secs) = (hp.core, hp.sections.clone());
            for f in fires.list.iter().filter(|f| f.state == FireState::Burning) {
                let burn = fire_damage(f.intensity, humans, difficulty.0);
                let was = hp.sections[f.sec];
                hp.sections[f.sec] = (was - burn).max(0.0);
                if was > 0.0 && hp.sections[f.sec] == 0.0 {
                    let sec = &class.sections[f.sec];
                    if sec.vital {
                        hp.core -= core_bleed(class, &hp, burn); // fire goes through the same damage path
                    }
                    info!("{} of {} burnt out", sec.label, class.name);
                }
            }
            // A section burnt to nothing puts its fire out even if the hull floor ('Invulnerable Percent') then props it back up.
            let burnt: Vec<bool> = hp.sections.iter().map(|&s| s <= 0.0).collect();
            super::shells::floor_hull(class, &mut hp, (core, &secs), mission.map_or(0.0, |m| m.invuln));
            // Decoded: a fire changes state only on these ticks. One that is out (no strength, or its section gone) stops emitting
            // on the tick that finds it, is cut loose from the ship on the next and deleted on the one after; a burning fire doubles.
            for f in &mut fires.list {
                match f.state {
                    FireState::Loose => f.state = FireState::Done,
                    FireState::Out => {
                        f.state = FireState::Loose;
                        if let Some(e) = f.fx {
                            commands.entity(e).queue(|mut w: EntityWorldMut| {
                                let world = w.get::<GlobalTransform>().map(|g| g.compute_transform());
                                w.remove::<ChildOf>();
                                if let (Some(t), Some(mut local)) = (world, w.get_mut::<Transform>()) {
                                    *local = t;
                                }
                            });
                        }
                    }
                    _ if f.intensity <= 0.0 || burnt[f.sec] => {
                        f.state = FireState::Out;
                        if let Some(Ok(mut x)) = f.fx.map(|e| fxq.get_mut(e)) {
                            x.emitting = false;
                        }
                    }
                    _ => {
                        f.intensity *= FIRE_GROWTH;
                        if let Some(Ok(mut x)) = f.fx.map(|e| fxq.get_mut(e)) {
                            x.rate_scale = fire_scale(f.intensity);
                        }
                    }
                }
            }
        }
        fires.list.retain(|f| {
            let done = f.state == FireState::Done;
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
/// The GLB carries both; show whichever matches the section state. Scenes load asynchronously, so the nodes under a ship are
/// resolved to (section, damaged variant) once, whenever any node's name or parent changes (or a ship leaves `Sinking`),
/// and cached on the ship; afterwards only a change of a section's destroyed state touches `Visibility`.
#[derive(Component, Default)]
pub struct SwapCache {
    /// (node, section index, is the damaged variant). A damaged variant of a layer that is no swappable section has `usize::MAX`.
    nodes: Vec<(Entity, usize, bool)>,
    /// Per section: destroyed state last applied to `nodes`.
    applied: Vec<bool>,
}

pub fn swap_meshes(
    mut commands: Commands,
    lib: Res<Library>,
    mut ships: Query<(Entity, &Ship, &Health, Option<&super::GunStates>, Option<&mut SwapCache>), Without<super::Sinking>>,
    mut bases: Query<(Entity, &super::ArmedBase, &Health, Option<&super::GunStates>, Option<&mut SwapCache>), Without<Ship>>,
    children: Query<&Children>,
    mut nodes: Query<(&Name, &mut Visibility)>,
    mut gun_roots: Query<&mut Visibility, (With<super::gunnery::GunPart>, Without<Name>)>,
    renamed: Query<(), Or<(Changed<Name>, Changed<ChildOf>)>>,
    mut unsunk: RemovedComponents<super::Sinking>,
) {
    // New or re-parented nodes, or a ship that is back under this system: look at every ship's nodes again.
    let rescan = !renamed.is_empty() | (unsunk.read().count() > 0);
    // A breaking-up ship's section nodes belong to `deaths` until it is gone. Armed bases swap their destroyed parts the same way.
    let all = ships.iter_mut().map(|(e, s, h, g, c)| (e, s.class, h, g, c)).chain(bases.iter_mut().map(|(e, b, h, g, c)| (e, b.class, h, g, c)));
    for (e, class, h, guns, cache) in all {
        let class = &lib.classes[class];
        // The damaged mesh carries its own wrecked mounts, so the guns the section carried go with the intact mesh. PROVISIONAL: no
        // retail code that hides them was found; the original keeps them as children of the section's node.
        for (mount, state) in class.mounts.iter().zip(guns.iter().flat_map(|g| g.0.iter())) {
            let want = if mount.parent.is_some_and(|p| class.sections[p].swappable && h.sections[p] <= 0.0) { Visibility::Hidden } else { Visibility::Inherited };
            if let Ok(mut v) = gun_roots.get_mut(state.root) {
                v.set_if_neq(want);
            }
        }
        let destroyed: Vec<bool> = h.sections.iter().map(|&hp| hp <= 0.0).collect();
        let want = |&(_, i, damaged): &(Entity, usize, bool), destroyed: &[bool]| match destroyed.get(i) {
            Some(&d) if d == damaged => Visibility::Inherited,
            _ => Visibility::Hidden,
        };
        let Some(mut cache) = cache else {
            let found = resolve_swap_nodes(e, class, &children, &nodes);
            for n in &found {
                if let Ok((_, mut vis)) = nodes.get_mut(n.0) {
                    vis.set_if_neq(want(n, &destroyed));
                }
            }
            commands.entity(e).insert(SwapCache { nodes: found, applied: destroyed });
            continue;
        };
        if rescan {
            cache.nodes = resolve_swap_nodes(e, class, &children, &nodes);
            cache.applied.clear();
        }
        if cache.applied == destroyed {
            continue;
        }
        let cache = &mut *cache;
        for n in &cache.nodes {
            // A node whose section did not change keeps what it shows (unless everything was just resolved again).
            if cache.applied.get(n.1) == destroyed.get(n.1) && n.1 != usize::MAX {
                continue;
            }
            if let Ok((_, mut vis)) = nodes.get_mut(n.0) {
                vis.set_if_neq(want(n, &destroyed));
            }
        }
        cache.applied = destroyed;
    }
}

/// The section a model node belongs to, by its layer name (a damaged mesh's `_damaged` suffix is stripped; case is ignored), and
/// whether the node is the damaged mesh. `swappable_only` skips sections that have no intact/damaged pair.
pub(super) fn section_of_node(class: &Class, name: &str, swappable_only: bool) -> (Option<usize>, bool) {
    let (base, damaged) = match crate::data::derive::damaged_layer(name) {
        Some(base) => (base, true),
        None => (name.to_string(), false),
    };
    (class.sections.iter().position(|s| (!swappable_only || s.swappable) && s.layer.eq_ignore_ascii_case(&base)), damaged)
}

/// The nodes under `root` that are a section's intact or damaged mesh, by name.
fn resolve_swap_nodes(root: Entity, class: &Class, children: &Query<&Children>, nodes: &Query<(&Name, &mut Visibility)>) -> Vec<(Entity, usize, bool)> {
    let mut found = Vec::new();
    for d in children.iter_descendants(root) {
        let Ok((name, _)) = nodes.get(d) else { continue };
        let n = name.as_str();
        // A layer node's primitives are named "<layer>.<texture>": they follow their node, which is the one that is swapped.
        if n.contains('.') {
            continue;
        }
        // A damaged mesh of a layer that is no swappable section can never be shown (it would sit on top of the intact one).
        match section_of_node(class, n, true) {
            (Some(i), damaged) => found.push((d, i, damaged)),
            (None, true) => found.push((d, usize::MAX, true)),
            (None, false) => {}
        }
    }
    found
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
    spawn_order: Query<&crate::scenario::SpawnIndex>,
    time: Res<Time>,
    lib: Res<Library>,
    mut rng: ResMut<SimRng>,
    mut ships: Query<(Entity, &Ship, &mut Health, Option<&crate::crew_arms::CrewPosts>, Option<&crate::boarding::Boardable>)>,
    (mut acc, table): (Local<f32>, crate::alliance::Hostility),
) {
    *acc += time.delta_secs();
    if *acc < CREW_REPAIR_TICK {
        return;
    }
    *acc -= CREW_REPAIR_TICK;
    for (e, ship, mut h, posts, boarding) in crate::scenario::in_spawn_order(&spawn_order, ships.iter_mut(), |x| x.0) {
        if h.destroyed {
            continue;
        }
        let class = &lib.classes[ship.class];
        let generic = posts.is_none().then(|| crate::crew_arms::fallback_posts(table.with_human(ship.side), boarding, &class.name)).flatten();
        let posts = posts.or(generic.as_ref());
        let (engineers, riggers) = (
            crew_slots(crate::scenario::SpawnIndex::of(&spawn_order, e), class, posts, 4, "engineering"),
            crew_slots(crate::scenario::SpawnIndex::of(&spawn_order, e), class, posts, 5, "rigging"),
        );
        damage_control(class, &mut h.sections, &engineers, &riggers, &mut || rng.f());
    }
}

#[cfg(test)]
mod tests;
