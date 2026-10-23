//! Guns, shells, damage sections and the ship-class library. Data comes from the WOT/MDB/MSB files via
//! tools/export_combat.py. PROVISIONAL items are my interpretations, not measured original behaviour.

use bevy::prelude::*;

use crate::audio::Sfx;
pub use crate::data::*;
use crate::fx::{self, FxLib};
use crate::{PlayerShip, Ship, Side, assets_dir};

mod asteroid;
mod damage;
mod gunnery;
mod hit_test;
mod lob;
mod shells;
mod status;
#[cfg(test)]
mod tests;

pub use asteroid::*;
pub use damage::*;
pub use gunnery::*;
pub use hit_test::*;
pub use lob::{Detonate, Shoot, Strike};
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
/// A base with gun towers (island): `class` is its `Base_*` entry in the library (mounts and sections); `Health` holds the section HP.
#[derive(Component)]
pub struct ArmedBase {
    pub class: usize,
}
/// What the rules' 'Vital Section has X damage' reads for a base (PROVISIONAL: mean damage of its vitalToMission sections).
pub fn base_mission_damage(class: &Class, hp: &Health) -> f32 {
    let v: Vec<f32> = class.sections.iter().zip(&hp.sections).filter(|(c, _)| c.mission).map(|(c, h)| 1.0 - h / c.hp.max(1.0)).collect();
    if v.is_empty() { 0.0 } else { (v.iter().sum::<f32>() / v.len() as f32).clamp(0.0, 1.0) }
}
/// A shell struck `victim` (counted even when the victim is invulnerable); fed to the mission rules.
#[derive(Message)]
pub struct Hit {
    pub shooter: Entity,
    pub victim: Entity,
}
/// An asteroid crossed the death threshold; breakup and rules consume independent readers.
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
}
/// Exhaust effect on a ship; its rate follows the ship's speed (PROVISIONAL: 30% idle up to 100% at top speed).
#[derive(Component)]
pub struct Engine;
/// Marks entities that a restart must despawn.
#[derive(Component)]
pub struct Transient;
/// The player's target and where on it the player clicked (ship-local); the guns aim at that spot, as in the original.
#[derive(Resource, Default)]
pub struct Target(pub Option<Entity>, pub Option<Vec3>, pub GroundFire);
#[derive(Resource)]
pub struct Rng(u64);
#[derive(Resource)]
struct Look {
    mesh: Handle<Mesh>,
    shell: Handle<StandardMaterial>,
    enemy_shell: Handle<StandardMaterial>,
}

impl Rng {
    #[cfg(test)]
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }
    fn direction(&mut self) -> Vec3 {
        Vec3::new(self.f() * 2.0 - 1.0, self.f() * 2.0 - 1.0, self.f() * 2.0 - 1.0).try_normalize().unwrap_or(Vec3::X)
    }
    pub fn f(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

pub fn plugin(app: &mut App) {
    app.insert_resource(Library::load(&assets_dir()))
        .init_resource::<Target>()
        .add_message::<Hit>()
        .add_message::<Fired>()
        .add_message::<Destroyed>()
        .add_message::<Detonate>()
        .add_message::<Shoot>()
        .add_message::<Strike>()
        .add_message::<AsteroidSetup>()
        .init_resource::<crate::data::AsteroidDefs>()
        .insert_resource(Rng(0x9E3779B97F4A7C15))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                (fire, base_fire, lob::shoot, shells, lob::detonate, lob::blasts, lob::grav_wells, lob::ram_damage, lob::strikes).chain(),
                (asteroid_deaths, fires, crew_repair, deaths, draw, engine_rate, damage_fx, debris, swap_meshes).chain(),
            )
                .chain()
                .after(crate::sail),
        )
        .add_systems(
            Update,
            (asteroid_motion, crate::collision::update_asteroids, asteroid_demo).chain().after(crate::input).before(crate::sail).run_if(in_state(crate::menu::Screen::Battle)),
        );
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
    for (pos, rot, name) in &lib.classes[class].engines {
        let local = Transform { translation: *pos, rotation: *rot, ..default() };
        if let Some(e) = fx::spawn(commands, fxl, name, Some(ship), local, world * local, false) {
            commands.entity(e).insert(Engine);
        }
    }
}

fn engine_rate(ships: Query<(&Ship, &Health)>, mut q: Query<(&mut fx::Fx, &ChildOf), With<Engine>>) {
    for (mut f, parent) in &mut q {
        if let Ok((ship, h)) = ships.get(parent.parent()) {
            // Thrust follows the engine order: dark at All Stop, brighter with each step up (PROVISIONAL curve).
            // A ship held still (NIS freeze, docked) keeps its order but shows no thrust.
            let throttle = crate::ORDERS[ship.order].1;
            f.emitting = !h.destroyed && throttle > 0.0 && ship.speed.abs() > 0.01;
            f.size_scale = 2.6; // PROVISIONAL: data size 1.6 gives a thread; the reference jet is about hull-wide
            f.rate_scale = 0.75 * (0.25 + 0.75 * throttle.min(1.25));
        }
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

/// A destroyed ship breaking up (docs/re/effects.md "Ship destruction"): the class explosion plays at once, then every section's
/// mesh flies off at its WOT `destructFx` velocity and bursts with its effect somewhere in its [start, end] window, as retail does.
#[derive(Component)]
pub struct Sinking {
    start: Transform,
    elapsed: f32,
    /// Section mesh nodes: (node, section, rest translation, burst time); filled on the first breakup frame.
    nodes: Option<Vec<(Entity, usize, Vec3, f32)>>,
}

/// Breakup length when a class has no destruct data (older imports). PROVISIONAL.
const BREAKUP_FALLBACK: f32 = 1.0;

impl Sinking {
    fn new(start: Transform) -> Self {
        Sinking { start, elapsed: 0.0, nodes: None }
    }
}

/// When a section's chunk is gone: at its burst (a point in [start, end] picked by `roll` in 0..1) when it has an effect, else
/// at `end` (retail's mid hulls just drift apart and vanish).
fn burst_time(d: &crate::data::Destruct, roll: f32) -> f32 {
    if d.fx.is_some() { d.start + (d.end - d.start) * roll } else { d.end }
}

/// The section's offset from its place on the hull at `t` seconds (ship-local), or None once its chunk is gone.
fn chunk_offset(d: &crate::data::Destruct, burst: f32, t: f32) -> Option<Vec3> {
    (t < burst).then(|| d.vel * t)
}

/// Gameplay death is immediate; the presentation breaks the hull up for a few seconds, then removes it.
#[allow(clippy::too_many_arguments)]
fn deaths(
    mut commands: Commands,
    lib: Res<Library>,
    fxl: Res<FxLib>,
    mut rng: ResMut<Rng>,
    mut sfx: MessageWriter<Sfx>,
    mut lost: MessageWriter<crate::lifeboats::ShipLost>,
    time: Res<Time>,
    children: Query<&Children>,
    mut ships: Query<(Entity, &mut Transform, &mut Ship, &mut Health, &mut Visibility, Option<&mut Sinking>)>,
    mut nodes: Query<(&Name, &mut Transform, &mut Visibility), Without<Ship>>,
) {
    for (e, mut tf, mut ship, mut hp, mut vis, sinking) in &mut ships {
        let class = &lib.classes[ship.class];
        if !hp.destroyed {
            if hp.core > 0.0 {
                continue;
            }
            hp.destroyed = true;
            ship.speed = 0.0;
            ship.ang_vel = 0.0;
            lost.write(crate::lifeboats::ShipLost(e));
            commands.entity(e).insert(Sinking::new(*tf));
            info!("{} destroyed", class.name);
            // Retail: the class explosion at the moment of death, before the pieces part. Pool Explosion_Heavy01-08 (8000 units),
            // PROVISIONAL as the death sound.
            let at = tf.translation;
            sfx.write(Sfx { name: format!("Explosion_Heavy0{}", 1 + (rng.f() * 8.0) as u32 % 8), at: Some(at), vol: 1.0, max: 8000.0 });
            if let Some(n) = &class.explosion_fx {
                let t = Transform::from_translation(at);
                fx::spawn(&mut commands, &fxl, n, None, t, t, true);
            }
            continue;
        }
        // Scripted Destroy remains an immediate, silent removal without lifeboats.
        let Some(mut sinking) = sinking else { continue };
        let before = sinking.elapsed;
        sinking.elapsed += time.delta_secs();
        let t = sinking.elapsed;
        *tf = sinking.start;
        if sinking.nodes.is_none() {
            let mut found = vec![];
            for d in children.iter_descendants(e) {
                let Ok((name, ntf, _)) = nodes.get(d) else { continue };
                let base = name.as_str().replacen("Damage_", "_", 1);
                if let Some(i) = class.sections.iter().position(|s| s.layer.eq_ignore_ascii_case(&base)) {
                    let burst = class.sections[i].destruct.as_ref().map_or(BREAKUP_FALLBACK, |d| burst_time(d, rng.f()));
                    found.push((d, i, ntf.translation, burst));
                }
            }
            sinking.nodes = Some(found);
        }
        let mut last = BREAKUP_FALLBACK;
        for &(node, i, rest, burst) in sinking.nodes.as_deref().unwrap_or_default() {
            let section = &class.sections[i];
            let Some(d) = &section.destruct else { continue };
            last = last.max(burst);
            let Ok((name, mut ntf, mut nvis)) = nodes.get_mut(node) else { continue };
            match chunk_offset(d, burst, t) {
                Some(off) => ntf.translation = rest + off,
                None if *nvis != Visibility::Hidden => *nvis = Visibility::Hidden,
                None => {}
            }
            // One burst per section, from its visible (undamaged or damaged) mesh.
            if before < burst
                && t >= burst
                && !name.as_str().contains("Damage_")
                && let Some(fx_name) = &d.fx
            {
                let centre = (section.min + section.max) / 2.0 + d.vel * burst;
                let at = Transform::from_translation(tf.transform_point(centre));
                fx::spawn(&mut commands, &fxl, fx_name, None, at, at, true);
            }
        }
        if t < last {
            continue;
        }
        *vis = Visibility::Hidden;
        // Keep the logical ship for mission rules, but remove its entire presentation (including looping smoke).
        if let Ok(children) = children.get(e) {
            for child in children.iter() {
                commands.entity(child).try_despawn();
            }
        }
        commands.entity(e).remove::<Sinking>();
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
