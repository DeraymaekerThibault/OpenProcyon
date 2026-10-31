//! PROCYON_REPORT=<game seconds> (tools/battle_check.py): watch a skirmish battle and, when one side is gone or the time is up, log one
//! `battle report {json}` line and quit. Per side: ships, bursts fired, hits, game time of the first burst. Per ship: distance
//! sailed, bursts fired, whether a computer commands it, and the longest unbroken scenery contact. Per fleet of a lobby skirmish:
//! victory points spent against the budget. Plus the closest approach between hostile ships. The checker turns these
//! into pass/fail expectations ("the computer engages", "nobody grinds on an island") so whole-battle regressions show up in
//! `tools/check.sh` instead of in playtests.
use bevy::prelude::*;
use std::collections::{BTreeMap, HashMap};

use crate::combat::{Fired, Health, Hit};
use crate::{Ship, Side};

#[derive(Resource)]
pub struct Report {
    limit: f32,
    time: f32,
    sides: BTreeMap<String, SideLine>,
    ships: HashMap<Entity, ShipLine>,
    closest: f32,
    /// Real frame times (ms), for stutter: a few long frames hide in an average.
    frames: Vec<f32>,
}

#[derive(Default)]
struct SideLine {
    ships: u32,
    alive: u32,
    bursts: u32,
    hits: u32,
    first_burst: Option<f32>,
}

struct ShipLine {
    class: String,
    side: String,
    last: Vec3,
    sailed: f32,
    bursts: u32,
    /// A computer captain commands it: not the human's own fleet (allied computer fleets share the human's side).
    computer: bool,
    contact: f32,
    worst_contact: f32,
    /// Sank during the watch: a ship sunk early cannot be judged on how far it sailed.
    sunk: bool,
}

fn side_name(side: Side) -> String {
    if side == Side::Player { "player".into() } else { format!("side{}", side.0) }
}

pub fn plugin(app: &mut App) {
    let Some(limit) = std::env::var("PROCYON_REPORT").ok().and_then(|v| v.parse().ok()) else { return };
    app.insert_resource(Report { limit, time: 0.0, sides: BTreeMap::new(), ships: HashMap::new(), closest: f32::INFINITY, frames: vec![] })
        .add_systems(FixedUpdate, watch.in_set(crate::BattleSet::Outcome).run_if(in_state(crate::menu::Screen::Battle)))
        .add_systems(Update, frame_time.run_if(in_state(crate::menu::Screen::Battle)));
}

fn frame_time(time: Res<Time<Real>>, mut report: ResMut<Report>) {
    report.frames.push(time.delta_secs() * 1000.0);
}

/// (median, 99th percentile, worst, frames over 50 ms) of the battle's frame times, after the first second (loading).
fn frame_stats(frames: &[f32]) -> (f32, f32, f32, usize) {
    let mut elapsed = 0.0;
    let mut f: Vec<f32> = frames
        .iter()
        .copied()
        .filter(|ms| {
            elapsed += ms;
            elapsed > 1000.0
        })
        .collect();
    if f.is_empty() {
        return (0.0, 0.0, 0.0, 0);
    }
    let over = f.iter().filter(|ms| **ms > 50.0).count();
    f.sort_by(f32::total_cmp);
    let at = |q: f32| f[((f.len() - 1) as f32 * q) as usize];
    (at(0.5), at(0.99), f[f.len() - 1], over)
}

/// Victory points each fleet of a lobby skirmish spent against the budget (`Skirmish::victory_limit`). Only open maps assemble fleets;
/// historical maps and the command-line shortcut have no budget, so they report none.
fn fleets_json(cfg: &crate::Config, sk: &crate::menu::Skirmish, lib: &crate::Library) -> serde_json::Value {
    if sk.map.is_none() || !sk.open() {
        return serde_json::Value::Array(vec![]);
    }
    cfg.groups
        .iter()
        .map(|g| {
            let names: Vec<String> = g.ships.iter().map(|s| s.0.clone()).collect();
            serde_json::json!({
                "player": g.player, "side": side_name(g.side), "human": g.human, "ships": names.len(),
                "spent": crate::menu::fleet_points(lib, &names, &g.weapons, &g.crew), "budget": sk.victory_limit, "limit": sk.fleet_size,
            })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn watch(
    time: Res<Time>,
    lib: Res<crate::Library>,
    mut report: ResMut<Report>,
    (cfg, sk): (Res<crate::Config>, Res<crate::menu::Skirmish>),
    ships: Query<(Entity, &Ship, &Transform, &Health, Option<&crate::ship::FleetMember>)>,
    fleets: Res<crate::ship::Fleets>,
    obstacles: Query<(&crate::collision::Obstacle, Option<&Visibility>), (Without<Ship>, Without<crate::combat::Asteroid>)>,
    mut fired: MessageReader<Fired>,
    mut hits: MessageReader<Hit>,
) {
    let dt = time.delta_secs();
    report.time += dt;
    let now = report.time;
    let side_of = |e: Entity| ships.get(e).ok().map(|(_, s, ..)| side_name(s.side));
    for f in fired.read() {
        if let Some(side) = side_of(f.shooter) {
            let line = report.sides.entry(side).or_default();
            line.bursts += 1;
            line.first_burst.get_or_insert(now);
        }
        if let Some(line) = report.ships.get_mut(&f.shooter) {
            line.bursts += 1;
        }
    }
    for h in hits.read() {
        if let Some(side) = side_of(h.shooter) {
            report.sides.entry(side).or_default().hits += 1;
        }
    }
    let scenery = crate::collision::scenery(obstacles.iter());
    for line in report.sides.values_mut() {
        line.alive = 0;
    }
    let mut live: Vec<(Side, Vec3)> = vec![];
    for (e, ship, tf, health, member) in &ships {
        let side = side_name(ship.side);
        let class = &lib.classes[ship.class];
        if !report.ships.contains_key(&e) {
            report.sides.entry(side.clone()).or_default().ships += 1;
            // The same test the captain AI uses (ship.rs `ai`): only the human's own fleet waits for orders.
            let computer = !(ship.side == Side::Player && member.is_none_or(|m| m.0 == fleets.local));
            report.ships.insert(
                e,
                ShipLine { class: class.name.clone(), side: side.clone(), last: tf.translation, sailed: 0.0, bursts: 0, computer, contact: 0.0, worst_contact: 0.0, sunk: false },
            );
        }
        if health.destroyed {
            report.ships.get_mut(&e).unwrap().sunk = true;
            continue;
        }
        report.sides.entry(side).or_default().alive += 1;
        live.push((ship.side, tf.translation));
        let touching = crate::collision::touches(class.collision_bounds, tf, &scenery);
        let line = report.ships.get_mut(&e).unwrap();
        line.sailed += tf.translation.distance(line.last);
        line.last = tf.translation;
        line.contact = if touching { line.contact + dt } else { 0.0 };
        line.worst_contact = line.worst_contact.max(line.contact);
    }
    for (i, (a, p)) in live.iter().enumerate() {
        for (b, q) in &live[i + 1..] {
            if a != b {
                report.closest = report.closest.min(p.distance(*q));
            }
        }
    }
    let sides_alive = report.sides.values().filter(|l| l.alive > 0).count();
    if report.time < report.limit && (sides_alive > 1 || report.time < 5.0) {
        return;
    }
    let (p50, p99, worst, over) = frame_stats(&report.frames);
    let json = serde_json::json!({
        "time": report.time.round(),
        "frame_ms": {"p50": p50.round(), "p99": p99.round(), "worst": worst.round(), "over_50": over},
        "closest": if report.closest.is_finite() { report.closest.round() } else { -1.0 },
        "sides": report.sides.iter().map(|(name, l)| (name.clone(), serde_json::json!({
            "ships": l.ships, "alive": l.alive, "bursts": l.bursts, "hits": l.hits, "first_burst": l.first_burst.map(f32::round),
        }))).collect::<serde_json::Map<_, _>>(),
        "ships": report.ships.values().map(|s| serde_json::json!({
            "class": s.class, "side": s.side, "computer": s.computer, "bursts": s.bursts, "sailed": s.sailed.round(), "sunk": s.sunk,
            "worst_contact": (s.worst_contact * 10.0).round() / 10.0,
        })).collect::<Vec<_>>(),
        "fleets": fleets_json(&cfg, &sk, &lib),
    });
    info!("battle report {json}");
    std::process::exit(0);
}
