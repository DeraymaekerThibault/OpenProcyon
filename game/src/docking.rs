//! Docking (docs/re/docking-and-orders.md): dock points, the exe's `ShipDockRoutine` (wait for a point, approach leg, final approach,
//! docked, then for AI ships move away / turn / return), repair while docked, dock-point marker effects and the rule hooks
//! (`Dock Ships`, `Set Group/Unit Dockable`, `Set Dock Time`, `Toggle Island Repair When Docked`, `Group/Unit Is Docked`).
//! Clicks and the right-click menu issue `DockMsg`s; rules.rs reads `Script::docked`, which this module keeps up to date.
//! PROVISIONAL: straight-line legs (the exe plans paths around obstacles), the far-side detour point, dock-point yaw sign, which
//! dockers hear the docked sound, and the repair effect placement (one at the ship centre).
//! Tows (menu Tow start / stop): the tower steers for the towee and the link is registered inside 50 units of surface distance; the
//! rigid tow itself lives in rules.rs. The docking status icon lives on the primary ship's bottom-bar portrait in panels.rs.
//! PROVISIONAL: the tow approach point (the towee's centre, not its tow node).
use bevy::ecs::system::{ParamSet, SystemParam};
use bevy::prelude::*;
use serde_json::Value;
use std::f32::consts::PI;

use crate::audio::Sfx;
use crate::campaign::{Mission, MissionBase};
use crate::combat::{Health, Library, ray_hit, seg_sphere};
use crate::fx::FxLib;
use crate::{PlayerShip, Ship, Side, assets_dir, game_to_bevy};

/// Docked once this close to the dock point (exe 10.0, also the approach-leg arrival tolerance here).
const ARRIVE: f32 = 10.0;
/// With every point taken, a waiting ship holds this far from the nearest one (exe 80.0).
const WAIT_DIST: f32 = 80.0;
/// Exe default dock duration; the actual one is the larger of this ('Set Dock Time' overrides it) and the repair time.
const MIN_DOCK_SECS: f32 = 5.0;
/// Approach accepted when the ship and the approach point are within this angle seen from the dockee (exe π·0.6); bases always pass.
const APPROACH_ARC: f32 = PI * 0.6;
/// Leaving a ship dockee: distance ⅔·(both lengths) along the dock axis, done within 5 units or after 10 s (exe constants).
const AWAY_FRAC: f32 = 2.0 / 3.0;
const AWAY_DONE: f32 = 5.0;
const LEG_SECS: f32 = 10.0;
/// Turning back toward the start point ends within 0.1 rad (or after 10 s); the return trip's first leg is 100 units.
const TURN_DONE: f32 = 0.1;
const RETURN_LEG: f32 = 100.0;
/// A tow link is registered when the hulls are this close (surface distance, exe 50).
const TOW_RANGE: f32 = 50.0;

/// One dock point in the owner's local Bevy frame; `fwd` is the node's own forward axis (the exe lines dockers up along it).
#[derive(Clone, Copy, Debug)]
pub struct DockPoint {
    pub pos: Vec3,
    pub fwd: Vec3,
}

/// Something ships can dock with: a ship or a base (exe `DockPointKeeper`).
#[derive(Component)]
pub struct Dockable {
    pub points: Vec<DockPoint>,
    taken: Vec<Option<Entity>>,
    /// 'Can Be Docked With' / 'Set Group/Unit Dockable': on for tenders and every base/island, off for other ships.
    pub enabled: bool,
    /// 'Repair When Docked With' / 'Toggle Island Repair When Docked': same defaults as `enabled`.
    pub repairs: bool,
    markers: Vec<Entity>,
}
impl Dockable {
    pub fn new(points: Vec<DockPoint>, enabled: bool) -> Self {
        let taken = vec![None; points.len()];
        Dockable { points, taken, enabled, repairs: enabled, markers: vec![] }
    }
    fn world(&self, tf: &Transform, i: usize) -> (Vec3, Vec3) {
        let p = &self.points[i];
        (tf.transform_point(p.pos), (tf.rotation * p.fwd).with_y(0.0).normalize_or(Vec3::NEG_Z))
    }
    /// The point nearest `from` (world), free or not.
    fn nearest(&self, tf: &Transform, from: Vec3) -> Option<usize> {
        (0..self.points.len()).min_by(|a, b| self.world(tf, *a).0.distance(from).total_cmp(&self.world(tf, *b).0.distance(from)))
    }
    /// The free point nearest `from`.
    pub fn nearest_free(&self, tf: &Transform, from: Vec3) -> Option<usize> {
        (0..self.points.len()).filter(|i| self.taken[*i].is_none()).min_by(|a, b| self.world(tf, *a).0.distance(from).total_cmp(&self.world(tf, *b).0.distance(from)))
    }
    fn release(&mut self, docker: Entity) {
        for t in &mut self.taken {
            if *t == Some(docker) {
                *t = None;
            }
        }
    }
}

/// (Repair Time, Repair Effect Time) per 'Repair Effect Name' (hud.hdt 'Repair Speed Info', docs/re/refit-resupply.md
/// §3.1): Small 4/7; Medium and IC_Small 10/15; Large and IC_Medium 20/25; IC_Large 30/35. Repair_Tender (and no name) set no
/// time; PROVISIONAL Medium so such ships still get repaired.
fn repair_secs(class: Option<&str>) -> (f32, f32) {
    match class {
        Some("Repair_Small") => (4.0, 7.0),
        Some("Repair_Large" | "Repair_IC_Medium") => (20.0, 25.0),
        Some("Repair_IC_Large") => (30.0, 35.0),
        _ => (10.0, 15.0),
    }
}

/// Dock points by entity: `data/docks.json` (bases, islands) plus each ship's own `ships/<id>/docks.json`.
pub fn load_docks() -> Value {
    let root = assets_dir();
    let read = |p: std::path::PathBuf| std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
    let mut docks = read(root.join("data/docks.json")).unwrap_or_else(|| Value::Object(Default::default()));
    for id in crate::data::ship_ids(&root) {
        if let Some(d) = read(root.join(format!("ships/{id}/docks.json"))) {
            docks[id] = d;
        }
    }
    docks
}
/// `Dockable` for an entity type from `load_docks` (tools/export_docks.py), if it has dock points. Non-ship types (bases,
/// islands) are always dockable; ships only when they are tenders.
pub fn dockable_for(docks: &Value, entity: &str) -> Option<Dockable> {
    let d = docks.get(entity)?;
    let points: Vec<DockPoint> = d["points"]
        .as_array()?
        .iter()
        .map(|p| DockPoint { pos: game_to_bevy(&p["pos"]), fwd: Quat::from_rotation_y(p["yaw"].as_f64().unwrap_or(0.0) as f32) * Vec3::NEG_Z })
        .collect();
    let enabled = !entity.starts_with("Ship_") || d["tender"].as_bool().unwrap_or(false);
    (!points.is_empty()).then(|| Dockable::new(points, enabled))
}

#[derive(Message)]
pub enum DockMsg {
    /// `docker: None` is the player's ship.
    Dock {
        docker: Option<Entity>,
        dockee: Entity,
    },
    Enabled(Entity, bool),
    Repairs(Entity, bool),
    /// Abort the player's approach (X, Stop).
    CancelShip(Entity),
    /// Tow `towee` (`tower: None` is the player's ship): the tower approaches until the link is registered.
    TowStart {
        tower: Option<Entity>,
        towee: Entity,
    },
    /// Break every tow of `tower` (`None` is the player's ship).
    TowBreak {
        tower: Option<Entity>,
    },
}

/// A ship on its way to tow `towee` (PROVISIONAL approach). It becomes a rules tow link once in range.
#[derive(Component)]
pub struct Tower {
    towee: Entity,
}

/// True when two hulls (centres `here` / `there`, collision radii) are within the tow range of each other.
pub fn tow_in_range(here: Vec3, there: Vec3, r_tower: f32, r_towee: f32) -> bool {
    (here - there).with_y(0.0).length() - r_tower - r_towee < TOW_RANGE
}

/// Exe DOCKSTATE_*: WAITING_TO_DOCK, DOCK_APPROACH (+PREDOCKED), DOCKED, MOVINGAWAYFROMDOCK, TURNINGTORETURN, RETURNING.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Phase {
    Wait,
    Approach,
    Docked,
    /// The human ship after its time is up: still inside the dockee's hull, so collision stays off until it has sailed clear.
    Clear,
    Away,
    Turn,
    Return,
}
/// The dock routine of one ship (exe `ShipDockRoutine`).
#[derive(Component)]
pub struct Docker {
    dockee: Entity,
    point: Option<usize>,
    phase: Phase,
    /// Approach: the approach leg is done and the ship steers straight for the dock point.
    final_leg: bool,
    /// Fixed base approach point; keep its detour until the ship has sailed around the island.
    base_approach: Option<Vec3>,
    timer: f32,
    duration: f32,
    repair_secs: f32,
    /// The one-step heal has happened (it lands at Repair Time; the scaffold plays on until the dock duration ends).
    healed: bool,
    /// Full refit (revive destroyed sections and re-create every gun): the dockee is a base, or this is a campaign world.
    full: bool,
    start: Vec3,
    /// While docked: the ship's position in the dockee's frame (it keeps station on a moving dockee).
    offset: Vec3,
}
impl Docker {
    pub fn is_docked(&self) -> bool {
        self.phase == Phase::Docked
    }
    /// The docking status icon shows while the ship waits for, approaches or sits in a dock.
    pub fn shows_status(&self) -> bool {
        matches!(self.phase, Phase::Wait | Phase::Approach | Phase::Docked)
    }
    pub fn ignores_collision(&self, other: Entity) -> bool {
        // PROVISIONAL: allow docking contact until the retail collision rejection list is decoded.
        self.dockee == other && (matches!(self.phase, Phase::Docked | Phase::Clear | Phase::Away) || (self.phase == Phase::Approach && self.final_leg))
    }
}
pub fn plugin(app: &mut App) {
    app.add_message::<DockMsg>().add_systems(
        Update,
        (handle_messages, routine, tow_routine, markers).chain().before(crate::input).run_if(in_state(crate::menu::Screen::Battle)).run_if(crate::menu::overlay_closed),
    );
}

fn handle_messages(
    mut msgs: MessageReader<DockMsg>,
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    player: Query<Entity, With<PlayerShip>>,
    pos: Query<&Transform, With<Ship>>,
    mut dockables: Query<&mut Dockable>,
    dockers: Query<&Docker>,
    mut mission: Option<ResMut<Mission>>,
    fleets: Option<Res<crate::ship::Fleets>>,
) {
    let cancel = keys.just_pressed(KeyCode::Backspace);
    for m in msgs.read() {
        match *m {
            DockMsg::TowStart { tower, towee } => {
                let Some(tower) = tower.or_else(|| player.single().ok()) else { continue };
                // Tow links live in the rules, so only campaign missions (the ones with a `Mission`) can register one.
                if tower == towee || mission.is_none() || !pos.contains(towee) {
                    continue;
                }
                info!("tow: {tower} sails to tow {towee}");
                commands.entity(tower).insert(Tower { towee });
            }
            DockMsg::TowBreak { tower } => {
                let Some(tower) = tower.or_else(|| player.single().ok()) else { continue };
                commands.entity(tower).remove::<Tower>();
                if let Some(m) = mission.as_mut() {
                    m.script.break_tows(tower);
                }
            }
            DockMsg::Dock { docker, dockee } => {
                let Some(docker) = docker.or_else(|| player.single().ok()) else { continue };
                if docker == dockee || dockers.get(docker).is_ok_and(|d| d.dockee == dockee) || !dockables.get(dockee).is_ok_and(|d| d.enabled) {
                    continue;
                }
                let Ok(start) = pos.get(docker).map(|t| t.translation) else { continue };
                if let Ok(old) = dockers.get(docker)
                    && let Ok(mut od) = dockables.get_mut(old.dockee)
                {
                    od.release(docker);
                }
                info!("docking: {docker} ordered to dock with {dockee}");
                // Exe state 0: remember the start (the return point), then wait for / approach a point.
                commands.entity(docker).insert(Docker {
                    dockee,
                    point: None,
                    phase: Phase::Wait,
                    final_leg: false,
                    base_approach: None,
                    timer: 0.0,
                    duration: 0.0,
                    repair_secs: 0.0,
                    healed: false,
                    full: false,
                    start,
                    offset: Vec3::ZERO,
                });
            }
            DockMsg::Enabled(e, on) => {
                if let Ok(mut d) = dockables.get_mut(e) {
                    d.enabled = on;
                }
            }
            DockMsg::Repairs(e, on) => {
                if let Ok(mut d) = dockables.get_mut(e) {
                    d.repairs = on;
                }
            }
            DockMsg::CancelShip(e) => {
                commands.entity(e).remove::<Tower>();
                if let Ok(d) = dockers.get(e)
                    && matches!(d.phase, Phase::Wait | Phase::Approach)
                {
                    if let Ok(mut dk) = dockables.get_mut(d.dockee) {
                        dk.release(e);
                    }
                    commands.entity(e).remove::<Docker>();
                }
            }
        }
    }
    // X / Stop abort the player's approach (a ship already docked stays until its time is up; a registered tow is kept).
    if cancel && let Some(p) = fleets.as_ref().and_then(|f| f.selected).or_else(|| player.single().ok()) {
        commands.entity(p).remove::<Tower>();
        if let Ok(d) = dockers.get(p)
            && matches!(d.phase, Phase::Wait | Phase::Approach)
        {
            if let Ok(mut dk) = dockables.get_mut(d.dockee) {
                dk.release(p);
            }
            commands.entity(p).remove::<Docker>();
        }
    }
}

/// Towers steer for their towee; inside `TOW_RANGE` (surface distance) the link is registered in the rules and the tower is free again.
fn tow_routine(
    mut commands: Commands,
    lib: Res<Library>,
    mut mission: Option<ResMut<Mission>>,
    mut ships: ParamSet<(Query<(Entity, &Transform, &Ship, &Health)>, Query<(Entity, &Tower, &mut Ship)>)>,
) {
    let Some(m) = mission.as_mut() else { return };
    let snap: Vec<_> = ships
        .p0()
        .iter()
        .map(|(e, tf, sh, h)| {
            let class = &lib.classes[sh.class];
            (e, tf.translation, class.radius, h.destroyed, tf.transform_point(class.tow_points[0]), tf.transform_point(class.tow_points[1]))
        })
        .collect();
    let towers: Vec<(Entity, Entity)> = ships.p1().iter().map(|(e, t, _)| (e, t.towee)).collect();
    let find = |e: Entity| snap.iter().find(|s| s.0 == e).copied();
    let mut movers = ships.p1();
    for (tower, towee) in towers {
        let (Some((_, here, rh, hd, rear, _)), Some((_, there, rt, td, _, front))) = (find(tower), find(towee)) else {
            commands.entity(tower).remove::<Tower>();
            continue;
        };
        if hd || td {
            commands.entity(tower).remove::<Tower>();
            continue;
        }
        let Ok((_, _, mut sh)) = movers.get_mut(tower) else { continue };
        if tow_in_range(here, there, rh, rt) {
            info!("tow: {tower} tows {towee}");
            m.script.set_tow(tower, towee, true);
            commands.entity(tower).remove::<Tower>();
            sh.waypoints.clear();
            sh.goal = None;
            sh.order = 0;
        } else {
            sh.waypoints = vec![(front - (rear - here)).with_y(here.y)];
            sh.goal = None;
            sh.order = 2;
        }
    }
}

/// The dock heal, in one step: damaged sections back to full; a `full` refit also revives
/// destroyed sections and restores the core (PROVISIONAL: the traced heal does not show the core) — the gun re-creation is
/// `GunStates::rearm` at the call site. A destroyed ship stays destroyed.
fn repair(lib: &Library, ship: &Ship, h: &mut Health, full: bool) {
    let class = &lib.classes[ship.class];
    for (hp, s) in h.sections.iter_mut().zip(&class.sections) {
        if *hp > 0.0 || full {
            *hp = s.hp;
        }
    }
    if full {
        h.core = class.core_hp;
    }
}

/// Bevy heading (as `Ship::heading`) that points the bow from `from` to `to`.
fn bearing(from: Vec3, to: Vec3) -> f32 {
    let d = to - from;
    (-d.x).atan2(-d.z)
}
fn angdiff(a: f32, b: f32) -> f32 {
    (a - b + PI).rem_euclid(2.0 * PI) - PI
}
fn flat(a: Vec3, b: Vec3) -> f32 {
    (a - b).with_y(0.0).length()
}

/// The approach point lies on the dock point's own axis, on the docker's side of it, one docker length out for a base
/// and 1.5 dockee lengths out for a ship.
fn approach_point(point: Vec3, fwd: Vec3, ship: Vec3, leg: f32) -> Vec3 {
    let dir = if fwd.dot((ship - point).with_y(0.0)) >= 0.0 { fwd } else { -fwd };
    (point + dir * leg).with_y(ship.y)
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn routine(
    time: Res<Time>,
    lib: Res<Library>,
    fxl: Option<Res<FxLib>>,
    mut commands: Commands,
    mut sfx: MessageWriter<Sfx>,
    mut mission: Option<ResMut<Mission>>,
    mut dockers: Query<(Entity, &mut Transform, &mut Ship, &mut Health, &mut Docker, Has<PlayerShip>, Option<&mut crate::combat::GunStates>)>,
    mut dockees: Query<(&Transform, &mut Dockable, Option<&Ship>, Option<&Health>), Without<Docker>>,
    obstacles: Query<(&crate::collision::Obstacle, Option<&Visibility>), (Without<Ship>, Without<crate::collision::Ghost>)>,
) {
    let dt = time.delta_secs();
    let where_: Vec<(Entity, Vec3)> = dockers.iter().map(|(e, tf, ..)| (e, tf.translation)).collect();
    let pos_of = |e: Entity| where_.iter().find(|w| w.0 == e).map(|w| w.1);
    let mut bumped = vec![];
    for (e, mut tf, mut ship, mut h, mut d, human, mut guns) in &mut dockers {
        let gone = |m: &mut Option<ResMut<Mission>>, dk: &mut Dockable, d: &Docker| {
            dk.release(e);
            if let Some(m) = m.as_mut() {
                m.script.set_docked(e, d.dockee, false);
            }
        };
        let Ok((dtf, mut dk, dship, dhealth)) = dockees.get_mut(d.dockee) else {
            commands.entity(e).remove::<Docker>();
            continue;
        };
        if h.destroyed || dhealth.is_some_and(|h| h.destroyed) || (!dk.enabled && matches!(d.phase, Phase::Wait | Phase::Approach)) {
            gone(&mut mission, &mut dk, &d);
            commands.entity(e).remove::<Docker>();
            continue;
        }
        let me = &lib.classes[ship.class];
        let base = dship.is_none();
        let dockee_len = dship.map_or(0.0, |s| lib.classes[s.class].length);
        let here = tf.translation;
        // Exe: a reserved point that is no longer the nearest gives way to the nearest one when that is free.
        if let (Some(i), Phase::Approach) = (d.point, d.phase)
            && let Some(n) = dk.nearest(dtf, here)
            && n != i
            && dk.taken[n].is_none()
        {
            dk.release(e);
            dk.taken[n] = Some(e);
            d.point = Some(n);
            d.final_leg = false;
            d.base_approach = None;
        }
        match d.phase {
            Phase::Wait => {
                if let Some(i) = dk.nearest_free(dtf, here) {
                    dk.taken[i] = Some(e);
                    d.point = Some(i);
                    d.phase = Phase::Approach;
                    d.final_leg = false;
                    d.base_approach = None;
                    continue;
                }
                let Some(i) = dk.nearest(dtf, here) else { continue };
                let (p, _) = dk.world(dtf, i);
                // Every point taken: take this one over if we would get there sooner than its holder (exe compares distance ÷ own
                // speed, so a stopped ship never does), else hold short of it.
                let holder = dk.taken[i].and_then(pos_of);
                if ship.speed.abs() > 0.01 && holder.is_some_and(|h| here.distance(p) < h.distance(p)) {
                    bumped.extend(dk.taken[i]);
                    dk.taken[i] = Some(e);
                    d.point = Some(i);
                    d.phase = Phase::Approach;
                    d.final_leg = false;
                    d.base_approach = None;
                } else if here.distance(p) < WAIT_DIST {
                    ship.waypoints.clear();
                    ship.order = 0;
                } else {
                    ship.waypoints = vec![p.with_y(here.y)];
                    ship.goal = None;
                    ship.order = 2;
                }
            }
            Phase::Approach => {
                let Some(i) = d.point else {
                    d.phase = Phase::Wait;
                    continue;
                };
                let (p, fwd) = dk.world(dtf, i);
                if flat(p, here) < ARRIVE {
                    d.phase = Phase::Docked;
                    d.timer = 0.0;
                    let (heal_at, effect) = if dk.repairs { repair_secs(me.repair_fx.as_deref()) } else { (0.0, 0.0) };
                    d.repair_secs = heal_at;
                    d.healed = false;
                    // Campaign worlds (IsCampaign) and every base give the full refit; a skirmish tender only tops up.
                    d.full = base || mission.is_some();
                    d.duration = mission.as_ref().and_then(|m| m.script.dock_time(e)).unwrap_or(MIN_DOCK_SECS).max(effect);
                    d.offset = dtf.compute_affine().inverse().transform_point3(here);
                    info!("docking: {e} docked with {} for {:.1} s (repair {:.1} s)", d.dockee, d.duration, d.repair_secs);
                    ship.order = 0;
                    ship.speed = 0.0;
                    ship.waypoints.clear();
                    ship.goal = None;
                    if let Some(m) = mission.as_mut() {
                        m.script.set_docked(e, d.dockee, true);
                    }
                    // Exe: the dock routine plays HUD_Options_Dock on docking and the repair setup starts the ship's repair effect.
                    if human || ship.side == Side::Player {
                        sfx.write(Sfx::ui("HUD_Options_Dock"));
                    }
                    if d.repair_secs > 0.0
                        && let (Some(fxl), Some(name)) = (fxl.as_deref(), me.repair_fx.as_deref())
                    {
                        let local = Transform::from_translation(me.centre);
                        crate::fx::spawn(&mut commands, fxl, name, Some(e), local, *tf * local, true);
                    }
                    continue;
                }
                let leg = if base { me.length } else { 1.5 * dockee_len };
                let ap = d.base_approach.unwrap_or_else(|| approach_point(p, fwd, here, leg));
                if base && d.base_approach.is_none() {
                    d.base_approach = Some(ap);
                    let scenery = crate::collision::scenery(obstacles.iter());
                    let Some(mut route) = crate::collision::route_clear(me.collision_bounds, here, ap, &scenery) else {
                        ship.waypoints.clear();
                        ship.goal = None;
                        ship.order = 0;
                        warn!("docking: no clear base approach for {e} to {}", d.dockee);
                        continue;
                    };
                    route.push(p.with_y(here.y));
                    ship.waypoints = route;
                }
                if base && !d.final_leg && ship.waypoints.is_empty() {
                    ship.order = 0;
                    continue;
                }
                // The leg is done near the approach point, or once the ship is already lined up between it and the dock point.
                // PROVISIONAL: the exe's path-follower tolerance is not decoded; 10 units alone makes a cutter circle the point.
                let rel = (here - p).with_y(0.0);
                let along = (ap - p).with_y(0.0).normalize_or_zero();
                let lined_up = rel.dot(along) < leg && (rel - along * rel.dot(along)).length() < me.length;
                if !d.final_leg && (!base || ship.waypoints.len() <= 2) && (flat(ap, here) < ARRIVE.max(me.length) || lined_up) {
                    d.final_leg = true;
                }
                ship.goal = None;
                if d.final_leg {
                    ship.waypoints = vec![p.with_y(here.y)];
                    ship.order = 1;
                } else if base {
                    ship.order = 2;
                } else {
                    // Exe: a ship on the far side of a ship dockee (angle at its centre > 0.6π) re-plans around it; with straight legs
                    // here that is a detour point out beside the dock point first.
                    let c = dtf.translation;
                    let far = !base && (here - c).with_y(0.0).angle_between((ap - c).with_y(0.0)) > APPROACH_ARC;
                    let side = (p + (p - c).with_y(0.0).normalize_or_zero() * (me.length + dockee_len)).with_y(here.y);
                    ship.waypoints = if far { vec![side, ap, p.with_y(here.y)] } else { vec![ap, p.with_y(here.y)] };
                    ship.order = 2;
                }
            }
            Phase::Docked => {
                d.timer += dt;
                // PROVISIONAL: the exe's arrival steering (not decoded) carries the ship the last few units; here it settles onto the
                // point, parallel to the point's axis, instead of stopping up to 10 units short.
                let k = 1.0 - (-2.0 * dt).exp();
                if let Some(i) = d.point {
                    let pl = dk.points[i].pos.with_y(d.offset.y);
                    d.offset = d.offset.lerp(pl, k);
                    let (_, fwd) = dk.world(dtf, i);
                    let along = bearing(Vec3::ZERO, fwd);
                    let want = if angdiff(along, ship.heading).abs() <= PI / 2.0 { along } else { along + PI };
                    ship.heading += angdiff(want, ship.heading) * k;
                    ship.ang_vel = 0.0;
                }
                tf.translation = dtf.transform_point(d.offset);
                ship.order = 0;
                ship.speed = 0.0;
                if d.repair_secs > 0.0 && !d.healed && d.timer >= d.repair_secs {
                    d.healed = true;
                    repair(&lib, &ship, &mut h, d.full);
                    if d.full
                        && let Some(g) = guns.as_mut()
                    {
                        g.rearm();
                    }
                    info!("docking: {e} repaired ({})", if d.full { "full refit" } else { "damaged sections" });
                }
                if d.timer >= d.duration {
                    info!("docking: {e} released by {} (core {:.0}/{:.0})", d.dockee, h.core, me.core_hp);
                    gone(&mut mission, &mut dk, &d);
                    // The human ship just stays where it is; AI ships leave along the dock axis (ship dockees) and sail back.
                    if human {
                        d.phase = Phase::Clear;
                        d.timer = 0.0;
                    } else {
                        d.phase = if base { Phase::Turn } else { Phase::Away };
                        d.timer = 0.0;
                    }
                }
            }
            Phase::Clear => {
                d.timer += dt;
                let out = d.point.is_none_or(|i| flat(dk.world(dtf, i).0, here) > me.length);
                if out || d.timer >= LEG_SECS {
                    commands.entity(e).remove::<Docker>();
                }
            }
            Phase::Away => {
                d.timer += dt;
                let (p, fwd) = d.point.map_or((dtf.translation, Vec3::NEG_Z), |i| dk.world(dtf, i));
                let heading = Vec3::new(-ship.heading.sin(), 0.0, -ship.heading.cos());
                let dir = if heading.dot(fwd) >= 0.0 { fwd } else { -fwd };
                let to = (p + dir * AWAY_FRAC * (dockee_len + me.length)).with_y(here.y);
                ship.waypoints = vec![to];
                ship.order = 1;
                if flat(to, here) < AWAY_DONE || d.timer >= LEG_SECS {
                    d.phase = Phase::Turn;
                    d.timer = 0.0;
                }
            }
            Phase::Turn => {
                d.timer += dt;
                let want = bearing(here, d.start);
                ship.waypoints.clear();
                ship.goal = Some(want);
                ship.order = 1;
                if angdiff(want, ship.heading).abs() < TURN_DONE || d.timer >= LEG_SECS {
                    let dir = (d.start - here).with_y(0.0).normalize_or_zero();
                    ship.goal = None;
                    ship.waypoints = if flat(d.start, here) > RETURN_LEG { vec![here + dir * RETURN_LEG, d.start] } else { vec![d.start] };
                    ship.order = 2;
                    d.phase = Phase::Return;
                }
            }
            Phase::Return => {
                if ship.waypoints.is_empty() || flat(d.start, here) < ARRIVE {
                    ship.waypoints.clear();
                    ship.order = 0;
                    commands.entity(e).remove::<Docker>();
                }
            }
        }
    }
    // A ship whose point was taken over goes back to waiting (exe: its routine state is reset to 1).
    for b in bumped {
        if let Ok((.., mut d, _, _)) = dockers.get_mut(b) {
            d.point = None;
            d.phase = Phase::Wait;
        }
    }
}

/// Exe `DockPointEffectKeeper`: every dock point of a dockable object shows the 'Docking_Point' effect while the object can be
/// docked with; the effects go away when it cannot (disabled or destroyed).
fn markers(mut commands: Commands, fxl: Option<Res<FxLib>>, mut q: Query<(Entity, &Transform, &mut Dockable, Option<&Health>)>) {
    let Some(fxl) = fxl else { return };
    for (e, tf, mut dk, h) in &mut q {
        let on = dk.enabled && !h.is_some_and(|h| h.destroyed);
        if on && dk.markers.is_empty() {
            dk.markers = (0..dk.points.len())
                .filter_map(|i| {
                    let p = dk.points[i];
                    let local = Transform::from_translation(p.pos).looking_to(p.fwd, Vec3::Y);
                    crate::fx::spawn(&mut commands, &fxl, "Docking_Point", Some(e), local, *tf * local, false)
                })
                .collect();
        } else if !on && !dk.markers.is_empty() {
            for m in std::mem::take(&mut dk.markers) {
                if let Ok(mut c) = commands.get_entity(m) {
                    c.despawn();
                }
            }
        }
    }
}

/// 'Set Group/Unit Towable' FALSE: no tow can be started on this ship.
#[derive(Component)]
pub struct NoTow;

/// Click picking shared by the left-click dock and the right-click menu: what is under the cursor and whether the player can dock there.
#[derive(SystemParam)]
pub struct Dockees<'w, 's> {
    q: Query<'w, 's, (Entity, &'static Transform, &'static Dockable, Option<&'static Ship>, Option<&'static MissionBase>)>,
    hulls: Query<'w, 's, &'static crate::collision::Obstacle>,
    armed: Query<'w, 's, (&'static crate::combat::ArmedBase, &'static Health)>,
    untowable: Query<'w, 's, (), With<NoTow>>,
    lib: Res<'w, Library>,
    sensors: Res<'w, crate::environment::Sensors>,
    mission: Option<Res<'w, Mission>>,
    msg: MessageWriter<'w, DockMsg>,
    menu: Res<'w, crate::options_menu::OptionsMenu>,
}
impl Dockees<'_, '_> {
    pub fn can_dock(&self, actor: Entity, dockee: Entity) -> bool {
        let Ok((_, _, d, ship, base)) = self.q.get(dockee) else { return false };
        let hostile = base.is_some_and(|b| self.mission.as_deref().is_some_and(|m| m.script.hostile(m.script.player(), &b.owner)));
        d.enabled && !hostile && ship.is_none_or(|s| s.side == Side::Player) && self.sensors.in_sight(actor, dockee, self.mission.as_deref())
    }
    /// Dockable ally under the ray, with whether the selected ship's group has sight (no separate dock range).
    pub fn under(&self, ray: Ray3d, actor: Entity) -> Option<(Entity, bool)> {
        let hostile = |m: Option<&MissionBase>| m.is_some_and(|m| self.mission.as_deref().is_some_and(|ms| ms.script.hostile(ms.script.player(), &m.owner)));
        self.q
            .iter()
            .filter(|(e, _, _, ship, base)| ship.is_none_or(|s| s.side == Side::Player) && !hostile(*base) && self.sensors.player_sees(*e, self.mission.as_deref()))
            .filter_map(|(e, tf, d, ship, _)| {
                // Islands hover on their collision hull: dock points sit far offshore, so a dock-point sphere swallowed nearby waypoint clicks.
                let t = match (ship, self.hulls.get(e)) {
                    (Some(s), _) => ray_hit(&self.lib.classes[s.class], tf, ray)?,
                    (None, Ok(hull)) => hull.ray_hit(ray)?,
                    (None, Err(_)) => {
                        let r = d.points.iter().map(|p| p.pos.length()).fold(0.0, f32::max) + 8.0;
                        seg_sphere(ray.origin, ray.origin + *ray.direction * 3000.0, tf.translation, r)? * 3000.0
                    }
                };
                Some((e, t, self.can_dock(actor, e)))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(e, _, ok)| (e, ok))
    }
    /// Hostile base gun tower under the ray, including the clicked point in base-local coordinates.
    pub fn hostile_base_hit(&self, ray: Ray3d, at: Vec3) -> Option<(Entity, f32, Vec3)> {
        self.q
            .iter()
            .filter_map(|(e, tf, _, _, base)| {
                let base = base?;
                let mission = self.mission.as_deref()?;
                if !mission.script.hostile(mission.script.player(), &base.owner) || !crate::environment::can_see(Some(mission), at, tf.translation) {
                    return None;
                }
                let (armed, hp) = self.armed.get(e).ok()?;
                let t = crate::combat::base_ray_hit(&self.lib.classes[armed.class], hp, tf, ray)?;
                Some((e, t, tf.to_matrix().inverse().transform_point3(ray.get_point(t))))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }
    /// True on the frame an options-menu click closed the menu: that click is not a world click.
    pub fn menu_swallows_click(&self) -> bool {
        self.menu.swallow.load(std::sync::atomic::Ordering::Relaxed)
    }
    pub fn cancel_as(&mut self, actor: Entity) {
        self.msg.write(DockMsg::CancelShip(actor));
    }
    pub fn dock_as(&mut self, actor: Entity, dockee: Entity) {
        self.msg.write(DockMsg::Dock { docker: Some(actor), dockee });
    }
    /// The ship `tower` is towing, if any (links are kept by the rules: campaign missions only).
    pub fn towee_of(&self, tower: Entity) -> Option<Entity> {
        self.mission.as_deref()?.script.towee_of(tower)
    }
    /// Whether tows can be made at all here (campaign missions only, see `DockMsg::TowStart`).
    pub fn can_tow(&self) -> bool {
        self.mission.is_some()
    }
    pub fn towable(&self, e: Entity) -> bool {
        !self.untowable.contains(e)
    }
    pub fn tow_as(&mut self, actor: Entity, towee: Entity) {
        self.msg.write(DockMsg::TowStart { tower: Some(actor), towee });
    }
    pub fn break_tow_as(&mut self, actor: Entity) {
        self.msg.write(DockMsg::TowBreak { tower: Some(actor) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn mission_two_base_docking_plans_around_the_island_before_allowing_contact() {
        let (mut app, lib) = app();
        app.init_resource::<crate::collision::Shapes>().add_systems(Update, crate::collision::sail.after(routine));
        let map: Value = serde_json::from_slice(&std::fs::read(assets_dir().join("maps/mission02/map.json")).unwrap()).unwrap();
        let docks = load_docks();
        let mut dockee = None;
        let mut start = Vec3::ZERO;
        for object in map["objects"].as_array().unwrap() {
            let tf = crate::scenario::scenery_transform(object, crate::scenario::game_rotation(&object["rot"]), &docks);
            let kind = object["type"].as_str().unwrap();
            let body = app.world().resource::<crate::collision::Shapes>().obstacle(kind, &tf);
            let e = app.world_mut().spawn(tf).id();
            if let Some(body) = body {
                app.world_mut().entity_mut(e).insert(body);
            }
            if kind == "Base_AbandonedCamp" {
                let dk = dockable_for(&docks, kind).unwrap();
                let p = dk.world(&tf, 0).0;
                start = (tf.translation - (p - tf.translation).with_y(0.0).normalize() * 280.0).with_y(0.0);
                app.world_mut().entity_mut(e).insert(dk);
                dockee = Some(e);
            }
        }
        let dockee = dockee.unwrap();
        let (s, h) = ship(&lib, "Ship_Navy_Torpedo", Side::Player);
        let me = app.world_mut().spawn((Transform::from_translation(start), s, h, PlayerShip, crate::collision::Drift::default())).id();
        app.world_mut().resource_mut::<Messages<DockMsg>>().write(DockMsg::Dock { docker: None, dockee });
        tick(&mut app, 0.1);
        tick(&mut app, 0.1);
        let d = app.world().get::<Docker>(me).unwrap();
        assert!(!d.ignores_collision(dockee), "the island stays solid during the detour");
        let route = app.world().get::<Ship>(me).unwrap().waypoints.clone();
        assert!(route.len() > 2, "the far-side approach needs a detour: {route:?}");
        for _ in 0..6000 {
            tick(&mut app, 0.1);
            if app.world().get::<Docker>(me).unwrap().is_docked() {
                return;
            }
        }
        panic!("base detour did not reach the dock: {:?}", app.world().get::<Transform>(me).unwrap().translation);
    }

    #[test]
    fn coventry_docks_and_repairs_at_norville_on_the_flight_plane() {
        let (mut app, lib) = app();
        app.init_resource::<crate::collision::Shapes>().add_systems(Update, crate::collision::sail.after(routine));
        let map: Value = serde_json::from_slice(&std::fs::read(assets_dir().join("maps/mission02/map.json")).unwrap()).unwrap();
        let mission: Value = serde_json::from_slice(&std::fs::read(assets_dir().join("maps/mission02/mission.json")).unwrap()).unwrap();
        let kind = "Base_Safetytown";
        let object = map["objects"].as_array().unwrap().iter().find(|o| o["type"] == kind).unwrap();
        let docks = load_docks();
        let tf = crate::scenario::scenery_transform(object, crate::scenario::game_rotation(&object["rot"]), &docks);
        let dk = dockable_for(&docks, kind).unwrap();
        let (point, axis) = dk.world(&tf, 0);
        assert!(point.y.abs() < 0.01);
        let body = app.world().resource::<crate::collision::Shapes>().obstacle(kind, &tf).unwrap();
        let dockee = app.world_mut().spawn((tf, dk, body)).id();
        let class = mission["ships"].as_array().unwrap().iter().find(|s| s["name"] == "SS Coventry").unwrap()["type"].as_str().unwrap();
        let (s, mut h) = ship(&lib, class, Side::Player);
        let full = h.core;
        h.core *= 0.25;
        let start = point + axis * lib.classes[s.class].length * 2.0;
        let me = app.world_mut().spawn((Transform::from_translation(start), s, h, crate::collision::Drift::default())).id();
        app.world_mut().resource_mut::<Messages<DockMsg>>().write(DockMsg::Dock { docker: Some(me), dockee });
        for _ in 0..6000 {
            tick(&mut app, 0.1);
            if app.world().get::<Docker>(me).is_some_and(Docker::is_docked) {
                assert!(app.world().get::<Transform>(me).unwrap().translation.y.abs() < 0.01);
                tick(&mut app, 6.0);
                assert_eq!(app.world().get::<Health>(me).unwrap().core, full);
                return;
            }
        }
        panic!("Coventry could not reach Norville: {:?}", app.world().get::<Transform>(me).unwrap().translation);
    }

    #[test]
    fn towing_detaches_only_its_formation_follower() {
        let (mut w, ids) = crate::ship::movement_tests::fleet_world();
        crate::ship::set_formation(&mut w, "Player", Some(crate::ship::Formation::Column), true);
        w.entity_mut(ids[1]).insert(Tower { towee: ids[4] });
        crate::ship::fleet_follow(&mut w);
        assert!(!w.get::<crate::ship::FleetPilot>(ids[1]).unwrap().hold);
        assert!(w.get::<crate::ship::FleetPilot>(ids[2]).unwrap().hold);
    }

    fn app() -> (App, Library) {
        let lib = Library::load(&assets_dir());
        let mut app = App::new();
        app.add_message::<DockMsg>()
            .add_message::<Sfx>()
            .insert_resource(Time::<()>::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(Library::load(&assets_dir()))
            .add_systems(Update, (handle_messages, routine).chain());
        (app, lib)
    }
    fn tick(app: &mut App, secs: f32) {
        app.world_mut().resource_mut::<Time>().advance_by(Duration::from_secs_f32(secs));
        app.update();
    }
    fn ship(lib: &Library, name: &str, side: Side) -> (Ship, Health) {
        let class = lib.index(name);
        let c = &lib.classes[class];
        (
            Ship {
                class,
                side,
                heading: 0.0,
                speed: 3.0,
                ang_vel: 0.0,
                order: 2,
                waypoints: vec![],
                goal: None,
                cap: 1.0,
                throttle: None,
                sail_energy: 1.0,
                crew_speed: 1.0,
                crew_turn: 1.0,
            },
            Health { sections: c.sections.iter().map(|s| s.hp).collect(), core: c.core_hp, destroyed: false },
        )
    }
    fn point(x: f32) -> DockPoint {
        DockPoint { pos: Vec3::new(x, 0.0, 0.0), fwd: Vec3::NEG_Z }
    }

    #[test]
    fn player_docks_repairs_and_leaves_its_slot_free() {
        let (mut app, lib) = app();
        app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default()))
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, crate::course::update.after(routine));
        let line = app.world_mut().resource_mut::<Assets<Mesh>>().add(Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default()));
        let marks = app.world_mut().resource_mut::<Assets<Mesh>>().add(Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default()));
        for mesh in [&line, &marks] {
            app.world_mut().resource_mut::<Assets<Mesh>>().get_mut(mesh).unwrap().insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0, 0.0, 0.0]; 3]);
        }
        app.world_mut().spawn((crate::course::Course, Mesh3d(line.clone())));
        app.world_mut().spawn((crate::course::Marks::test(), Mesh3d(marks.clone())));
        let (s, full) = ship(&lib, "Ship_Navy_Cutter", Side::Player);
        let mut hurt = Health { sections: full.sections.iter().map(|h| h * 0.2).collect(), core: full.core * 0.2, destroyed: false };
        hurt.sections[0] = 0.0;
        let dockee = app.world_mut().spawn((Transform::from_xyz(100.0, 0.0, 0.0), Dockable::new(vec![point(-20.0)], true))).id();
        let me = app.world_mut().spawn((Transform::from_xyz(60.0, 0.0, 15.0), s, hurt, PlayerShip)).id();
        app.world_mut().resource_mut::<Messages<DockMsg>>().write(DockMsg::Dock { docker: None, dockee });
        tick(&mut app, 0.1);
        tick(&mut app, 0.1);
        let d = app.world().get::<Docker>(me).unwrap();
        assert_eq!((d.phase, d.point), (Phase::Approach, Some(0)), "order accepted, point reserved");
        assert!(d.shows_status(), "approach shows the bottom-bar docking animation");
        // A base dockee: the approach leg ends one docker length out along the point's axis, on the docker's side (+z here).
        let wp = app.world().get::<Ship>(me).unwrap().waypoints.clone();
        assert_eq!(wp.len(), 2);
        for mesh in [&line, &marks] {
            assert_eq!(app.world().resource::<Assets<Mesh>>().get(mesh).unwrap().count_vertices(), 0, "automatic docking paths are hidden");
        }
        assert!((wp[0] - Vec3::new(80.0, 0.0, lib.classes[lib.index("Ship_Navy_Cutter")].length)).length() < 0.01, "{wp:?}");
        // the movement system is not part of this test: put the ship on the dock point
        app.world_mut().get_mut::<Transform>(me).unwrap().translation = Vec3::new(80.0, 0.0, 2.0);
        tick(&mut app, 0.1);
        let docker = app.world().get::<Docker>(me).unwrap();
        assert!(docker.is_docked());
        assert!(docker.shows_status(), "animation remains during repair");
        assert!(docker.ignores_collision(dockee));
        assert!(!docker.ignores_collision(me));
        tick(&mut app, 2.0);
        let mid = app.world().get::<Health>(me).unwrap().core;
        assert_eq!(mid, full.core * 0.2, "no heal before Repair Time: it lands in one step");
        tick(&mut app, 2.5); // past the 4 s Repair Time (Repair_Small)
        assert_eq!(app.world().get::<Health>(me).unwrap().sections, full.sections, "base dock: full refit revives the destroyed section");
        assert!(app.world().get::<Docker>(me).unwrap().is_docked(), "still docked until the 7 s Repair Effect Time");
        tick(&mut app, 3.0);
        assert_eq!(app.world().get::<Docker>(me).unwrap().phase, Phase::Clear, "released, collision stays off while inside the hull");
        app.world_mut().get_mut::<Transform>(me).unwrap().translation = Vec3::new(80.0, 0.0, 200.0);
        tick(&mut app, 0.1);
        assert!(app.world().get::<Docker>(me).is_none(), "human ship is free once clear");
        let h = app.world().get::<Health>(me).unwrap();
        assert_eq!(h.core, full.core);
        assert_eq!(h.sections, full.sections);
        assert!(app.world().get::<Dockable>(dockee).unwrap().nearest_free(&Transform::default(), Vec3::ZERO).is_some(), "slot freed");
        let position = app.world().get::<Transform>(me).unwrap().translation;
        {
            let mut ship = app.world_mut().get_mut::<Ship>(me).unwrap();
            ship.order = 2;
            ship.waypoints = vec![position + Vec3::NEG_Z * 100.0];
        }
        tick(&mut app, 0.1);
        for mesh in [&line, &marks] {
            assert!(app.world().resource::<Assets<Mesh>>().get(mesh).unwrap().count_vertices() > 0, "ordinary routes return after docking");
        }
    }

    #[test]
    fn closer_ship_takes_over_a_point_and_ai_ship_returns_home() {
        let (mut app, lib) = app();
        let tender = ship(&lib, "Ship_Navy_Tender", Side::Player);
        let dockee = app.world_mut().spawn((Transform::default(), Dockable::new(vec![point(10.0)], true), tender.0, tender.1)).id();
        let (a, ha) = ship(&lib, "Ship_Navy_Cutter", Side::Player);
        let (b, hb) = ship(&lib, "Ship_Navy_Cutter", Side::Player);
        let far = app.world_mut().spawn((Transform::from_xyz(10.0, 0.0, 300.0), a, ha)).id();
        let near = app.world_mut().spawn((Transform::from_xyz(10.0, 0.0, 120.0), b, hb)).id();
        app.world_mut().resource_mut::<Messages<DockMsg>>().write(DockMsg::Dock { docker: Some(far), dockee });
        tick(&mut app, 0.1);
        tick(&mut app, 0.1);
        assert_eq!(app.world().get::<Docker>(far).unwrap().point, Some(0));
        app.world_mut().resource_mut::<Messages<DockMsg>>().write(DockMsg::Dock { docker: Some(near), dockee });
        tick(&mut app, 0.1);
        tick(&mut app, 0.1);
        tick(&mut app, 0.1);
        assert_eq!(app.world().get::<Docker>(near).unwrap().point, Some(0), "the nearer ship takes the point over");
        assert_eq!(app.world().get::<Docker>(far).unwrap().phase, Phase::Wait, "the other one waits");
        // Dock `near`, then let its time run out: an AI ship moves away, turns and sails back to where it was ordered from.
        app.world_mut().get_mut::<Transform>(near).unwrap().translation = Vec3::new(10.0, 0.0, 1.0);
        tick(&mut app, 0.1);
        assert_eq!(app.world().get::<Docker>(near).unwrap().phase, Phase::Docked);
        tick(&mut app, 10.5);
        assert_eq!(app.world().get::<Docker>(near).unwrap().phase, Phase::Away);
        tick(&mut app, 10.5);
        assert_eq!(app.world().get::<Docker>(near).unwrap().phase, Phase::Turn);
        assert_eq!(app.world().get::<Docker>(far).unwrap().phase, Phase::Approach, "the freed point goes to the waiting ship");
        tick(&mut app, 10.5);
        tick(&mut app, 0.1);
        assert_eq!(app.world().get::<Docker>(near).unwrap().phase, Phase::Return);
        assert_eq!(app.world().get::<Ship>(near).unwrap().waypoints.last().copied(), Some(Vec3::new(10.0, 0.0, 120.0)));
    }

    #[test]
    fn disabled_dockee_refuses_and_exported_points_load() {
        let (mut app, lib) = app();
        let (s, h) = ship(&lib, "Ship_Navy_Cutter", Side::Player);
        let dockee = app.world_mut().spawn((Transform::default(), Dockable::new(vec![point(0.0)], false))).id();
        let me = app.world_mut().spawn((Transform::from_xyz(0.0, 0.0, 30.0), s, h, PlayerShip)).id();
        app.world_mut().resource_mut::<Messages<DockMsg>>().write(DockMsg::Dock { docker: None, dockee });
        tick(&mut app, 0.1);
        assert!(app.world().get::<Docker>(me).is_none());
        let docks = load_docks();
        let tender = dockable_for(&docks, "Ship_Navy_Tender").expect("tender dock points exported");
        assert_eq!(tender.points.len(), 2);
        assert!(tender.points[0].pos.x < 0.0 && tender.points[1].pos.x > 0.0, "port and starboard");
        assert!(tender.enabled && tender.repairs, "tenders can be docked with");
        assert!(!dockable_for(&docks, "Ship_Navy_Frigate").unwrap().enabled, "other ships only after 'Set Group/Unit Dockable'");
        assert!(dockable_for(&docks, "Base_Grant").unwrap().enabled, "bases always");
        assert_eq!(repair_secs(lib.classes[lib.index("Ship_Navy_Tender")].repair_fx.as_deref()), (10.0, 15.0));
    }

    #[test]
    fn tow_registers_inside_fifty_units_of_surface_distance() {
        // Centres 120 apart with 30 + 30 of hull: 60 units of surface gap, out of range; 90 apart: 30, in range.
        assert!(!tow_in_range(Vec3::ZERO, Vec3::new(120.0, 0.0, 0.0), 30.0, 30.0));
        assert!(tow_in_range(Vec3::ZERO, Vec3::new(90.0, 0.0, 0.0), 30.0, 30.0));
        assert!(tow_in_range(Vec3::new(0.0, 500.0, 0.0), Vec3::new(90.0, -500.0, 0.0), 30.0, 30.0), "height is ignored");
    }

    #[test]
    fn approach_point_is_on_the_dockers_side_of_the_axis() {
        let p = Vec3::new(10.0, 0.0, 0.0);
        assert_eq!(approach_point(p, Vec3::NEG_Z, Vec3::new(50.0, 0.0, -80.0), 30.0), Vec3::new(10.0, 0.0, -30.0));
        assert_eq!(approach_point(p, Vec3::NEG_Z, Vec3::new(50.0, 0.0, 80.0), 30.0), Vec3::new(10.0, 0.0, 30.0));
    }
}
