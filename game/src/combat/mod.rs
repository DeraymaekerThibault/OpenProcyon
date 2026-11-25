//! Guns, shells, damage sections and the ship-class library. Data comes from the WOT/MDB/MSB files via
//! tools/export_combat.py. PROVISIONAL items are my interpretations, not measured original behaviour.

use bevy::prelude::*;

use crate::audio::Sfx;
pub use crate::data::*;
use crate::fx::{self, FxLib};
use crate::{Ship, Side, assets_dir};

mod asteroid;
pub mod beam;
mod damage;
pub mod gunnery;
mod hit_test;
pub mod lob;
mod minefield;
mod shells;
mod status;
#[cfg(test)]
mod tests;

pub use asteroid::*;
pub use damage::*;
pub use gunnery::*;
pub use hit_test::*;
pub use lob::{Detonate, Shoot, Strike};
pub use minefield::MapMine;
pub use shells::*;
pub use status::*;

/// Order matters: "Reinforced Wood" must match Reinforced before Wood.
pub const MATERIALS: [&str; 6] = ["Reinforced", "Wood", "Iron", "Cloth", "Stone", "Dragon"];

#[derive(Component)]
pub struct Health {
    pub sections: Vec<f32>,
    pub core: f32,
    pub destroyed: bool,
}
/// How a mission's 'Setup Island' makes the island's guns shoot: `accuracy` is the factor on the gun's burst spread (docs/re/volcanoes.md;
/// 0 = perfect aim), a `silent` island (Dummy stance) never opens fire, a `neutral` one only fires at ships that target it.
#[derive(Component, Clone, Copy)]
pub struct IslandGunnery {
    pub accuracy: f32,
    pub silent: bool,
    pub neutral: bool,
}

/// A base with gun towers (island): `class` is its `Base_*` entry in the library (mounts and sections); `Health` holds the section HP.
#[derive(Component)]
pub struct ArmedBase {
    pub class: usize,
}
/// The side a skirmish base fights for (a mission's bases take their allegiance from the rules instead). A base fires on ships of
/// any other side and is a target for them. PROVISIONAL: which side a map's base belongs to (`scenario::faction`).
#[derive(Component, Clone, Copy)]
pub struct BaseSide(pub crate::ship::Side);
/// The side a skirmish base started on; a base whose `BaseSide` differs was captured by boarding (the rules read it).
#[derive(Component, Clone, Copy)]
pub struct BaseOrigin(pub crate::ship::Side);
/// docs/re/campaign.md: per section kind (survival, max velocity, manoeuvrability, mission), 1 - the current over the maximum hit
/// points summed over the sections of that kind; the core is left out. None for a kind the class has no section of.
pub fn vital_damage(sections: &[crate::data::Section], hp: &[f32]) -> [Option<f32>; 4] {
    let kind = |k: usize, s: &crate::data::Section| [s.vital, s.speed, s.turn, s.mission][k];
    std::array::from_fn(|k| {
        let (cur, max) = sections.iter().zip(hp).filter(|(s, _)| kind(k, s)).fold((0.0, 0.0), |(c, m), (s, h)| (c + h.max(0.0), m + s.hp));
        (max > 0.0).then(|| 1.0 - cur / max)
    })
}
/// What the rules' 'Vital Section has X damage' reads for a base (PROVISIONAL: mean damage of its vitalToMission sections).
pub fn base_mission_damage(class: &Class, hp: &Health) -> f32 {
    let v: Vec<f32> = class.sections.iter().zip(&hp.sections).filter(|(c, _)| c.mission).map(|(c, h)| 1.0 - h / c.hp.max(1.0)).collect();
    if v.is_empty() { 0.0 } else { (v.iter().sum::<f32>() / v.len() as f32).clamp(0.0, 1.0) }
}
/// Where the dev autopilot clicks on a base, in world space (None when every vital section is destroyed): of the live mission sections'
/// surface points (triangle centres) that `visible` passes, the one nearest its section's middle, so a near miss either way still lands on
/// it; with none visible, the roof (`hit_test::base_aim_point`), and the autopilot looks for a firing spot. Aiming at the roof sent mission
/// 6's cannons over its 40-unit gate, and the nearest point to the ship put mission 2's shells into the island in front of its towers.
/// At most `TESTS` points are tried, spread over the candidates from the middle outward.
pub fn live_tower_point(class: &Class, hp: &Health, tf: &Transform, visible: impl Fn(Vec3) -> bool) -> Option<Vec3> {
    const TESTS: usize = 64;
    let live: Vec<&crate::data::Section> = class.sections.iter().zip(&hp.sections).filter(|(s, h)| s.mission && **h > 0.0).map(|(s, _)| s).collect();
    let mut points: Vec<(f32, Vec3)> = live
        .iter()
        .flat_map(|s| {
            let mid = (s.min + s.max) * 0.5;
            s.tris.iter().map(move |[a, b, c]| {
                let p = (*a + *b + *c) / 3.0;
                (p.distance_squared(mid), p)
            })
        })
        .collect();
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    let step = points.len().div_ceil(TESTS).max(1);
    points.iter().step_by(step).map(|(_, p)| tf.transform_point(*p)).find(|p| visible(*p)).or_else(|| hit_test::base_aim_point(class, hp, None).map(|p| tf.transform_point(p)))
}
/// A shell struck `victim` (counted even when the victim is invulnerable); fed to the mission rules.
#[derive(Message)]
pub struct Hit {
    pub shooter: Entity,
    pub victim: Entity,
}
/// A blast (mortar, mine, star burst) struck a section of `.0`: cloaked ships drop their cloak (cloak.rs).
#[derive(Message)]
pub struct Splashed(pub Entity);
/// An object crossed the death threshold: an asteroid (breakup and rules consume independent readers) or a ship (`deaths`, once, when
/// it starts sinking; `by` is the ship that last damaged it). The rules count it as a kill by `by`'s owner.
#[derive(Message)]
pub struct Destroyed {
    pub by: Entity,
    pub victim: Entity,
}
/// A gun burst started at `target` (None never happens for ships; kept for asteroids/scenery later).
#[derive(Message)]
pub struct Fired {
    pub shooter: Entity,
    pub target: Option<Entity>,
    /// The burst's bullet type (crew callouts: torpedoes away, mortar fired).
    pub bullet: String,
    /// `bullet` is a torpedo / a mine, read once at load (`GunDef::bullet_torpedo`, `bullet_family`).
    pub torpedo: bool,
    pub mine: bool,
}
/// Exhaust effect on a ship. The ship's speed fraction is sent to it once a second as both the emitter rate and the
/// velocity-length multiplier; it goes out with the section it hangs on.
#[derive(Component, Default)]
pub struct Engine {
    section: Option<usize>,
    mult: f32,
    since: f32,
    started: bool,
    effect: String,
}

impl Engine {
    /// The speed fraction the exhaust was last sent (0..1).
    pub fn thrust(&self) -> f32 {
        self.mult
    }
    /// The exhaust's effect file name (jets such as `NavyEngine_Heavy`, or `SmokeStack_Smoke` on Procyon funnels).
    pub fn effect(&self) -> &str {
        &self.effect
    }
}

/// Multiplies the emitter rate of the exhausts whose effect name contains `matching`: scale 1 = classic; a look that draws
/// its own jet flame (the render profile, render/trails.rs) thins those sprites while funnel smoke stays.
#[derive(Resource, Clone, PartialEq, Debug)]
pub struct ExhaustParticles {
    pub scale: f32,
    pub matching: String,
}
/// Marks entities that a restart must despawn.
#[derive(Component)]
pub struct Transient;
/// The player's target and where on it the player clicked (ship-local); the guns aim at that spot, as in the original.
#[derive(Resource, Default)]
pub struct Target(pub Option<Entity>, pub Option<Vec3>, pub GroundFire);
/// One step of a 64-bit xorshift, as a float in 0..1.
pub fn xorshift_unit(state: &mut u64) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    (*state >> 40) as f32 / (1u64 << 24) as f32
}

/// One xorshift stream. The simulation's streams live in `SimRng`; a `Rng` of its own is for drawn-only effects.
pub struct Rng(u64);

impl Default for Rng {
    fn default() -> Self {
        Self(SimRng::DEFAULT_SEED)
    }
}

/// The battle's random numbers, reseeded at every battle start (`scenario::reset_sim`), so one seed and the same input give the same
/// battle, a restart included. Seed: `--seed N` / `PROCYON_SEED`, else `DEFAULT_SEED`. It derefs to the battle stream, which every roll
/// that changes the battle takes (gun scatter, damage, fire, ricochet, boarding, lifeboats, asteroids; docs/re/tick.md "Random
/// numbers"); systems that kept a private generator before draw from their own named stream so they do not shift the battle rolls.
/// PROVISIONAL: the original seeds its battle generator from the map; this one is the same for every map unless given.
#[derive(Resource)]
pub struct SimRng {
    pub seed: u64,
    battle: Rng,
    /// Nebula storm lightning and meteor rolls (`nebula::storms`).
    pub nebula: Rng,
    /// A cloaked ship being spotted when it fires a torpedo (`cloak::spotted`).
    pub cloak: Rng,
}

impl SimRng {
    /// The seed every battle used before seeds were settable, kept so the battle stream starts where it did.
    pub const DEFAULT_SEED: u64 = 0x9E3779B97F4A7C15;
    pub fn new(seed: u64) -> Self {
        // splitmix64 of (seed, stream number): independent, never-zero xorshift states.
        let stream = |n: u64| {
            let mut z = seed.wrapping_add(n.wrapping_mul(0x9E3779B97F4A7C15));
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
            (z ^ (z >> 31)) | 1
        };
        Self { seed, battle: Rng(if seed == 0 { stream(0) } else { seed }), nebula: Rng(stream(1)), cloak: Rng(stream(2)) }
    }
    /// The generator states, to compare two `SimRng`s.
    #[cfg(test)]
    pub fn clone_state(&self) -> (u64, u64, u64, u64) {
        (self.seed, self.battle.0, self.nebula.0, self.cloak.0)
    }
    /// The seed of the next battle: `--seed N`, else `PROCYON_SEED`, else `DEFAULT_SEED`.
    pub fn configured_seed() -> u64 {
        crate::cli::value("--seed").or_else(|| std::env::var("PROCYON_SEED").ok()).and_then(|s| s.trim().parse().ok()).unwrap_or(Self::DEFAULT_SEED)
    }
}

impl Default for SimRng {
    fn default() -> Self {
        Self::new(Self::DEFAULT_SEED)
    }
}

impl std::ops::Deref for SimRng {
    type Target = Rng;
    fn deref(&self) -> &Rng {
        &self.battle
    }
}
impl std::ops::DerefMut for SimRng {
    fn deref_mut(&mut self) -> &mut Rng {
        &mut self.battle
    }
}
/// How shells are drawn: an unlit sphere, gold for the player's side, red for the enemy's.
#[derive(Resource)]
pub struct Look {
    pub mesh: Handle<Mesh>,
    pub shell: Handle<StandardMaterial>,
    pub enemy_shell: Handle<StandardMaterial>,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }
    fn direction(&mut self) -> Vec3 {
        Vec3::new(self.f() * 2.0 - 1.0, self.f() * 2.0 - 1.0, self.f() * 2.0 - 1.0).try_normalize().unwrap_or(Vec3::X)
    }
    pub fn f(&mut self) -> f32 {
        xorshift_unit(&mut self.0)
    }
    /// Uniform in `lo..hi` (a damage or speed range).
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.f() * (hi - lo)
    }
    /// 1..=n: one of the numbered variants of a sound (`Explosion_Med01` .. `08`).
    pub fn variant(&mut self, n: u32) -> u32 {
        1 + (self.f() * n as f32) as u32 % n
    }
    /// A random entry of a non-empty pool.
    pub fn pick<'a, T>(&mut self, pool: &'a [T]) -> &'a T {
        &pool[(self.f() * pool.len() as f32) as usize % pool.len()]
    }
}

impl crate::data::BulletDef {
    /// A random impact sound of this bullet on `material` at `at`, if it has any.
    pub fn impact_sound(&self, material: &str, at: Vec3, rng: &mut Rng) -> Option<crate::audio::Sfx> {
        let pool = self.impact_snd.get(material).filter(|p| !p.is_empty())?;
        Some(crate::audio::Sfx { name: rng.pick(pool).clone(), at: Some(at), vol: self.impact_vol, max: self.impact_max })
    }
}

/// The damage-table material (`MATERIALS`) a section's material string names; wood when none matches.
pub fn material_key(material: &str) -> &'static str {
    MATERIALS.iter().find(|m| material.contains(**m)).copied().unwrap_or("Wood")
}

/// The library `preload` built, for `plugin` to take.
static PRELOADED: std::sync::Mutex<Option<Library>> = std::sync::Mutex::new(None);

/// Loads the ship and weapon data of the asset root now and keeps it for `plugin`, so bad data in a mod ends the launch with a message
/// (the caller prints it and exits) instead of a panic while the app is built.
pub fn preload() -> Result<(), String> {
    let lib = Library::try_load(assets_dir())?;
    if let Ok(mut slot) = PRELOADED.lock() {
        *slot = Some(lib);
    }
    Ok(())
}

pub fn plugin(app: &mut App) {
    crate::scenario::number_spawns::<Ship>(app);
    crate::scenario::number_spawns::<Shell>(app);
    crate::scenario::number_spawns::<lob::Blast>(app);
    crate::scenario::number_spawns::<Asteroid>(app);
    crate::scenario::number_spawns::<ArmedBase>(app);
    // Space animals (whales, dragons) are spawned from the map's data in its order; a number gives them a `commands::NetId`.
    crate::scenario::number_spawns::<crate::environment::SpaceAnimal>(app);
    app.insert_resource(PRELOADED.lock().ok().and_then(|mut slot| slot.take()).unwrap_or_else(|| Library::load(assets_dir())))
        .init_resource::<Target>()
        .init_resource::<LastDamager>()
        .add_message::<Hit>()
        .add_message::<Splashed>()
        .add_message::<Fired>()
        .add_message::<Destroyed>()
        .add_message::<Detonate>()
        .add_message::<Shoot>()
        .add_message::<Strike>()
        .add_message::<AsteroidSetup>()
        .init_resource::<crate::data::AsteroidDefs>()
        .init_resource::<SimRng>()
        .init_resource::<crate::scenario::Difficulty>()
        .add_systems(Startup, (setup, beam::setup_look))
        .add_systems(
            FixedUpdate,
            (
                (minefield::arm, fire, beam::burn, base_fire, lob::shoot, shells, lob::detonate, lob::blasts, lob::grav_wells, lob::ram_damage, lob::strikes).chain(),
                (asteroid_deaths, fires, status::tick_nets, crew_repair, sink_clock, note_damagers, deaths).chain(),
            )
                .chain()
                .in_set(crate::BattleSet::Combat),
        )
        .add_systems(FixedUpdate, (asteroid_motion, crate::collision::update_asteroids).chain().in_set(crate::BattleSet::Forces).run_if(in_state(crate::menu::Screen::Battle)))
        .add_systems(Update, (breakup, draw, engine_rate, damage_fx, debris, swap_meshes, beam::draw).chain())
        .add_systems(Update, asteroid_demo.run_if(in_state(crate::menu::Screen::Battle)));
}

impl Health {
    pub fn new(c: &Class) -> Self {
        Health { sections: c.sections.iter().map(|s| s.hp).collect(), core: c.core_hp, destroyed: false }
    }
    /// Multiplies every section and the core by `f` (1 keeps them, 0.5 halves them).
    pub fn scale(&mut self, f: f32) {
        self.sections.iter_mut().for_each(|x| *x *= f);
        self.core *= f;
    }
}
pub fn spawn_engines(commands: &mut Commands, fxl: &FxLib, lib: &Library, ship: Entity, class: usize, world: Transform) {
    for (pos, rot, name, section) in &lib.classes[class].engines {
        let local = Transform { translation: *pos, rotation: *rot, ..default() };
        if let Some(e) = fx::spawn(commands, fxl, name, Some(ship), local, world * local, false) {
            commands.entity(e).insert(Engine { section: *section, effect: name.clone(), ..default() });
        }
    }
}

/// The engine keeper's update: once a second the ship's speed fraction (0..1) becomes both the
/// emitter-rate and the velocity-length multiplier of every exhaust; an exhaust exists only while that is above 0 and its
/// section stands, and the cloaking factor (1 - level) scales the rate. PROVISIONAL: the fraction is speed over Full (the original
/// multiplies two physics values).
fn engine_rate(
    time: Res<Time>,
    lib: Res<Library>,
    particles: Option<Res<ExhaustParticles>>,
    ships: Query<(&Ship, &Health, Option<&crate::cloak::Cloak>)>,
    mut q: Query<(&mut fx::Fx, &mut Engine, &ChildOf)>,
) {
    for (mut f, mut engine, parent) in &mut q {
        let Ok((ship, h, cloak)) = ships.get(parent.parent()) else { continue };
        engine.since += time.delta_secs();
        if !engine.started || engine.since > 1.0 {
            (engine.started, engine.since) = (true, 0.0);
            // A towed hull's engines are idle: it moves on the cable, not on thrust.
            engine.mult = if ship.towed { 0.0 } else { (ship.speed.abs() / crate::ship::full_speed(&lib.classes[ship.class])).clamp(0.0, 1.0) };
        }
        let section_lost = engine.section.is_some_and(|s| h.sections.get(s).is_some_and(|hp| *hp <= 0.0));
        // The flames fade with the hull as a ship cloaks (1 - level), gone at full cloak.
        let open = 1.0 - cloak.map_or(0.0, |c| c.level());
        f.emitting = !h.destroyed && !section_lost && engine.mult > 0.0 && open > 0.0;
        f.size_scale = 2.6; // PROVISIONAL: data size 1.6 gives a thread; the reference jet is about hull-wide
        let thin = particles.as_ref().filter(|p| engine.effect.contains(&p.matching)).map_or(1.0, |p| p.scale);
        (f.rate_scale, f.vel_scale) = (engine.mult * open * thin, engine.mult);
    }
}

/// Indirect weapons (mortars, mines, gravity charges) never auto-target: the player arms the bank in the weapon panel and
/// clicks a ground point for each shot.
#[derive(Default)]
pub struct GroundFire {
    pub armed: Option<usize>,
    pub shot: Option<(usize, Vec3)>,
    /// Panic button: every gun picks its own nearest enemy it can engage (as in the original) until the player retargets.
    pub panic: bool,
}

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut mats: ResMut<Assets<StandardMaterial>>) {
    let lit = |c: Color, mats: &mut Assets<StandardMaterial>| mats.add(StandardMaterial { base_color: c, unlit: true, ..default() });
    commands.insert_resource(Look {
        mesh: meshes.add(Sphere::new(0.35)),
        shell: lit(Color::srgb(1.0, 0.8, 0.3), &mut mats),
        enemy_shell: lit(Color::srgb(1.0, 0.25, 0.25), &mut mats),
    });
}

/// A destroyed ship breaking up (docs/re/effects.md "Ship sinking and final destruction"): the class explosion plays at once, then
/// the scene's break-apart clip moves the section meshes rigidly; a section is hidden, with its effect burst, when the clip passes
/// its `destructFx` start.
#[derive(Component)]
pub struct Sinking {
    elapsed: f32,
    /// Section mesh nodes: (node, section, rest pose, burst done); filled on the first breakup frame.
    nodes: Option<Vec<(Entity, usize, Transform, bool)>>,
    /// The 0.14 s section wipe (`damage::death_wipe`) has happened.
    wiped: bool,
}

/// Seconds between the kill and the start of the break-apart clip.
const CLIP_DELAY: f32 = 0.14;
/// Breakup length when a class has no break-apart clip (older imports, mods). PROVISIONAL.
const BREAKUP_FALLBACK: f32 = 1.0;
/// The ship's break-apart clip.
fn breakup_clip<'a>(lib: &'a Library, class: &crate::data::Class) -> Option<&'a crate::clips::Clip> {
    lib.clips.get(class.scene.as_deref()?)?.iter().find(|(n, _)| n == "ANIMATION00").map(|(_, c)| c)
}

impl Sinking {
    pub(crate) fn new() -> Self {
        Sinking { elapsed: 0.0, nodes: None, wiped: false }
    }

    /// What the dying hull's drive sees (docs/re/effects.md "Death drive"): from `CLIP_DELAY` after the kill every section is at 0 HP,
    /// so the speed and turning losses sit at their 0.9 cap and thrust, top speed and rudder torque are 0.1 of normal. Before that the
    /// ship still has the sections it died with. The stored `Health` is left alone: crew speech, damage looks and statistics keep
    /// reading what the hull really lost.
    pub fn drive_health(&self, h: &Health) -> Option<Health> {
        (self.elapsed >= CLIP_DELAY).then(|| Health { sections: vec![0.0; h.sections.len()], core: h.core, destroyed: true })
    }
}

/// Clip time at which a section's chunk is gone: when the clip passes `start` for sections with an effect (always just before the
/// clip's end), else at `end` (retail's mid hulls carry no effect and have start 0). PROVISIONAL for effect-less sections.
fn hide_time(d: &crate::data::Destruct) -> f32 {
    if d.fx.is_some() { d.start } else { d.end }
}

/// The ship that last damaged each ship: a shell, blast, mine or beam hit (`Hit`) or a ram (`lob::ram_damage`). It credits the kill
/// when the hull sinks. Fire and the collision with a rock add nothing, so they keep the earlier credit. PROVISIONAL: the original's
/// rule for the killing blow is not measured; the Game Summary's "Ships Sunk" (`stats.rs`) uses the same last-shooter rule.
#[derive(Resource, Default)]
pub struct LastDamager(pub std::collections::HashMap<Entity, Entity>);

fn note_damagers(mut hits: MessageReader<Hit>, mut last: ResMut<LastDamager>) {
    for h in hits.read() {
        if h.shooter != h.victim {
            last.0.insert(h.victim, h.shooter);
        }
    }
}

/// Gameplay death is immediate (`destroyed`: not a target, no collision, not alive for the rules); the hull keeps its speed and
/// heading and is steered on by the computer (`collision::tick` drives it while `Sinking`), while the presentation breaks it up for a
/// few seconds, then removes it. A hull that starts sinking sends one `Destroyed` credited to the ship that last damaged it, which the
/// rules count as that player's kill ('Player Killed A Object', 'Team has destroyed a ship'). No credit, as in the retail summary, for
/// a hull the black hole swallowed, for a lifeboat, or when nothing ever damaged it (PROVISIONAL: lifeboats).
#[allow(clippy::too_many_arguments)]
fn deaths(
    mut commands: Commands,
    lib: Res<Library>,
    mut lost: MessageWriter<crate::lifeboats::ShipLost>,
    mut killed: MessageWriter<Destroyed>,
    mut last: ResMut<LastDamager>,
    mut ships: Query<(Entity, &Ship, &mut Health, Has<crate::blackhole::Swallowed>, Has<crate::lifeboats::Lifeboat>)>,
) {
    for (e, ship, mut hp, swallowed, boat) in &mut ships {
        if hp.destroyed || hp.core > 0.0 {
            continue;
        }
        hp.destroyed = true;
        lost.write(crate::lifeboats::ShipLost(e));
        if let Some(by) = last.0.remove(&e).filter(|_| !swallowed && !boat) {
            killed.write(Destroyed { by, victim: e });
        }
        commands.entity(e).insert(Sinking::new());
        info!("{} destroyed", lib.classes[ship.class].name);
    }
}

#[allow(clippy::too_many_arguments)]
fn breakup(
    mut commands: Commands,
    lib: Res<Library>,
    fxl: Res<FxLib>,
    mut rng: Local<Rng>,
    mut sfx: MessageWriter<Sfx>,
    children: Query<&Children>,
    assets: Res<AssetServer>,
    mut ships: Query<(Entity, &Transform, &Ship, &mut Sinking, Option<&damage::DamageFx>)>,
    mut nodes: Query<(&Name, &mut Transform, &mut Visibility), Without<Ship>>,
) {
    for (e, tf, ship, mut sinking, dfx) in &mut ships {
        let class = &lib.classes[ship.class];
        let clip = breakup_clip(&lib, class);
        let t = (sinking.elapsed - CLIP_DELAY).max(0.0);
        if !sinking.wiped && sinking.elapsed >= CLIP_DELAY {
            sinking.wiped = true;
            if let Some(dfx) = dfx {
                let vel = tf.rotation * Vec3::NEG_Z * ship.speed;
                damage::death_wipe(&mut commands, (&assets, &fxl), &mut rng, &mut sfx, (tf, class, vel), dfx);
            }
        }
        if sinking.nodes.is_none() {
            // PROVISIONAL death sound pool; visual randomness must not perturb combat rolls.
            let at = tf.translation;
            sfx.write(Sfx { name: format!("Explosion_Heavy0{}", rng.variant(8)), at: Some(at), vol: 1.0, max: 8000.0 });
            if let Some(n) = &class.explosion_fx {
                let t = Transform::from_translation(at);
                fx::spawn(&mut commands, &fxl, n, None, t, t, true);
            }
            let mut found = vec![];
            for d in children.iter_descendants(e) {
                let Ok((name, ntf, _)) = nodes.get(d) else { continue };
                if let (Some(i), _) = damage::section_of_node(class, name.as_str(), false) {
                    found.push((d, i, *ntf, false));
                }
            }
            sinking.nodes = Some(found);
        }
        for (node, i, rest, done) in sinking.nodes.as_mut().into_iter().flatten() {
            let section = &class.sections[*i];
            let Ok((name, mut ntf, mut nvis)) = nodes.get_mut(*node) else { continue };
            if let Some(pose) = clip.and_then(|c| c.pose(&section.layer, t, *rest)) {
                *ntf = pose;
            }
            let Some(d) = &section.destruct else { continue };
            if t < hide_time(d) {
                continue;
            }
            if *nvis != Visibility::Hidden {
                *nvis = Visibility::Hidden;
            }
            // One burst per section, from its visible (undamaged or damaged) mesh.
            if !*done && !crate::data::derive::is_damage_layer(name.as_str()) {
                *done = true;
                if let Some(fx_name) = &d.fx {
                    let at = Transform::from_translation(tf.transform_point(ntf.transform_point(d.point)));
                    fx::spawn(&mut commands, &fxl, fx_name, None, at, at, true);
                }
            }
        }
    }
}

/// The break-up clock runs on the tick (the death drive reads it, `Sinking::drive_health`), and so does the end of the break-up:
/// the hull's presentation goes and the ship is queued for removal at the start of the next tick. `breakup` only draws from it.
fn sink_clock(mut commands: Commands, lib: Res<Library>, time: Res<Time>, children: Query<&Children>, mut ships: Query<(Entity, &Ship, &mut Sinking, &mut Visibility)>) {
    for (e, ship, mut sinking, mut vis) in &mut ships {
        sinking.elapsed += time.delta_secs();
        let clip = breakup_clip(&lib, &lib.classes[ship.class]);
        if sinking.elapsed - CLIP_DELAY < clip.map_or(BREAKUP_FALLBACK, |c| c.duration) {
            continue;
        }
        *vis = Visibility::Hidden;
        // Its entire presentation goes too (including looping smoke).
        if let Ok(children) = children.get(e) {
            for child in children.iter() {
                commands.entity(child).try_despawn();
            }
        }
        commands.entity(e).remove::<Sinking>();
        crate::queue_removal(&mut commands, e);
    }
}

fn ring(g: &mut Gizmos, c: Vec3, r: f32, col: Color) {
    let n = 32;
    for k in 0..n {
        let (a, b) = (k as f32 / n as f32 * std::f32::consts::TAU, (k + 1) as f32 / n as f32 * std::f32::consts::TAU);
        g.line(c + Vec3::new(a.cos(), 0.0, a.sin()) * r, c + Vec3::new(b.cos(), 0.0, b.sin()) * r, col);
    }
}

/// A ring on the target (the HUD draws the HP; gun readiness is in the weapon panel).
fn draw(mut g: Gizmos, lib: Res<Library>, target: Res<Target>, ships: Query<(Entity, &Transform, &Ship, &Health)>, mission: Option<Res<crate::campaign::Mission>>) {
    if crate::briefing::active(mission.as_deref()) {
        return;
    }
    for (e, tf, ship, h) in &ships {
        if h.destroyed {
            continue;
        }
        let class = &lib.classes[ship.class];
        if target.0 == Some(e) {
            ring(&mut g, tf.translation, class.radius, Color::srgb(1.0, 0.2, 0.2));
        }
    }
}
