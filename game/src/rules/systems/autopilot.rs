//! Dev autopilot for `--script mission` (tools/autoplay.py): steers the human's ship through a campaign mission from what its rules
//! wait for. A test driver for unattended runs, not game behaviour; all its tuning is PROVISIONAL.
use super::*;

/// `--script mission`, outside mission 1's scripted opening: the bot's orders for one frame (fleet, docking, towing, steering).
#[allow(clippy::too_many_arguments)]
pub(super) fn drive(
    commands: &mut Commands,
    mission: &mut Mission,
    world: &[Unit],
    extra: &mut Extra,
    ships: &mut Query<RuleShip>,
    target: &mut Target,
    rocks: &Query<(Entity, &Transform, &Asteroid), (Without<Ship>, Without<crate::environment::SpaceAnimal>)>,
    lib: &Library,
) {
    mission.script.crew_screen = false; // keep the default crew and arms (missions 3+ open the screen after their first NIS)
    // PROCYON_AUTOINVULN also keeps the player's fleet from being boarded: a pirate captured mission 4's flagship while the
    // invulnerable bot fought it, leaving the run on a stranded stand-in ship.
    if std::env::var_os("PROCYON_AUTOINVULN").is_some()
        && let Some(owner) = world.iter().find(|u| u.primary).map(|u| u.owner.clone())
    {
        for s in ships.iter().filter(|s| s.mission.owner == owner) {
            extra.board.write(crate::boarding::BoardMsg::Boardable(s.entity, false));
        }
    }
    autopilot_dock(&mission.script, world, &mission.data, extra);
    autopilot_tow(&mission.script, world, &mission.data, extra);
    // While running, the fleet follows in formation with cease fire, as a player would order from the fleet bar: left to their
    // own captains the escorts stay and fight a battle they cannot win (mission 3's ambush). Released when the run ends.
    let flee = fleeing(&mission.script);
    if flee != *extra.fleet_fleeing {
        *extra.fleet_fleeing = flee;
        commands.queue(move |w: &mut World| {
            let owner = w.resource::<crate::ship::Fleets>().local.clone();
            let Some(fleet) = w.resource::<crate::ship::Fleets>().groups.get(&owner).cloned() else { return };
            let kind = if fleet.formation == crate::ship::Formation::None { crate::ship::Formation::Column } else { fleet.formation };
            crate::ship::set_formation(w, &owner, Some(kind), flee);
            if fleet.cease_fire != flee {
                crate::ship::fleet_help(w, &owner, false); // toggles cease fire
            }
        });
    }
    // A ship that is docking or sailing to a tow steers itself (docking.rs); steering it as well would fight the approach.
    if extra.docking_player.is_empty() && extra.towing_player.is_empty() {
        let from = world.iter().find(|u| u.primary).map_or(Vec3::ZERO, |u| u.pos);
        let boats: Vec<(Vec3, &str)> = extra.lifeboats.iter().map(|(tf, b)| (tf.translation, b.owner.as_str())).collect();
        // The player's mortar mounts (mission 9's StarHammer carries Star and Nova mortars on different arcs).
        let mortars: Vec<Mortar> = ships
            .iter()
            .find(|s| s.primary)
            .and_then(|s| {
                let guns = extra.player_guns.get(s.entity).ok()?;
                let mounts = lib.classes[s.ship.class].mounts.iter().enumerate().filter(|(i, _)| guns.gun(*i).contains("Mortar"));
                Some(
                    mounts
                        .map(|(i, m)| Mortar { bank: m.bank, range: lib.guns.get(guns.gun(i)).map_or(0.0, |d| d.range[2]), rot: s.transform.rotation * m.rot, arc: m.arc })
                        .collect(),
                )
            })
            .unwrap_or_default();
        // Where the player's base-capable guns point, as angles off the bow (no mortar, torpedo, harpoon, mine or point defence):
        // the autopilot turns one of them, not always the bow, on a gun tower or gate.
        let guns: Vec<f32> = ships
            .iter()
            .find(|s| s.primary)
            .and_then(|s| {
                let guns = extra.player_guns.get(s.entity).ok()?;
                let base_gun = |i: usize| {
                    let g = guns.gun(i);
                    !["Mortar", "Torpedo", "Harpoon", "Mine"].iter().any(|k| g.contains(k)) && lib.guns.get(g).is_some_and(|d| !d.point_defense)
                };
                Some(lib.classes[s.ship.class].mounts.iter().enumerate().filter(|(i, _)| base_gun(*i)).map(|(_, m)| mount_angle(m.rot)).collect())
            })
            .unwrap_or_default();
        let sight = Sight { guns, land: extra.land.iter().filter(|(_, v)| *v != Some(&Visibility::Hidden)).map(|(o, _)| o).collect() };
        // Armed bases that still have a live mission section, with the world and base-local point the player's guns aim at: one they
        // can see past the islands from here (`combat::live_tower_point`).
        let gun = from + Vec3::Y * GUN_HEIGHT;
        let base_ids: Vec<(Entity, Vec3, Vec3)> = extra
            .bases
            .iter()
            .filter_map(|(e, _, tf, armed)| {
                let p = armed.and_then(|(a, h)| crate::combat::live_tower_point(&lib.classes[a.class], h, tf, |p| sight.clear(gun, p)))?;
                Some((e, p, tf.to_matrix().inverse().transform_point3(p)))
            })
            .collect();
        autopilot(&mission.script, world, &mission.data, rocks, &base_ids, &boats, ships, target, &mortars, &sight, lib, &mut extra.pilot);
    }
    // A new target, the fleet bar's attack order, as a player would give it: the escorts join the fight instead of trailing in formation
    // (`ship::fleet_help` ignores a target their captains may not attack). Not while running (cease fire above).
    let aim = target.0.filter(|_| !flee);
    if aim != extra.pilot.helped {
        extra.pilot.helped = aim;
        if aim.is_some() {
            commands.queue(|w: &mut World| {
                let owner = w.resource::<crate::ship::Fleets>().local.clone();
                crate::ship::fleet_help(w, &owner, true);
            });
        }
    }
}

/// How far ahead the dev autopilot sets its waypoint while leaving a volume (PROVISIONAL).
const LEAVE_STEP: f32 = 400.0;
/// One of the player's mortar mounts as the dev autopilot sees it: its bank, maximum range, world rotation and arc (radians).
struct Mortar {
    bank: usize,
    range: f32,
    rot: Quat,
    arc: f32,
}

/// What the dev autopilot knows to aim from a good spot: the player's base-capable guns (angles off the bow) and the islands' and bases'
/// collision, which stops every shell (`combat::shells`).
struct Sight<'a> {
    guns: Vec<f32>,
    land: Vec<&'a crate::collision::Obstacle>,
}

/// Height of the player's guns above the sailing plane for the autopilot's line-of-sight test (PROVISIONAL: a typical deck height).
const GUN_HEIGHT: f32 = 4.0;
/// Within this range a hostile ship the island hides is approached from a spot that sees it, `FOE_SPOT` from it (PROVISIONAL).
const FOE_SIGHT_RANGE: f32 = 400.0;
const FOE_SPOT: f32 = 90.0;
/// How far short of the aim point the first collision may lie and still count as a hit on it (a base's section sits just inside its hull).
const SIGHT_SLACK: f32 = 1.5;

impl Sight<'_> {
    /// Whether a shell from `a` reaches `b`: no island or base surface before the last `SIGHT_SLACK` units.
    fn clear(&self, a: Vec3, b: Vec3) -> bool {
        let len = a.distance(b);
        self.land.iter().filter_map(|o| o.segment_hit(a, b)).all(|t| (1.0 - t) * len <= SIGHT_SLACK)
    }
    /// Open water: no island under or over the point.
    fn open(&self, at: Vec3) -> bool {
        let (a, b) = (at.with_y(60.0), at.with_y(-60.0));
        self.land.iter().all(|o| o.segment_hit(a, b).is_none())
    }
    /// The open-water point `radius` from `target` nearest `from` with a clear shot at `target`; island geometry hides a stockade or a
    /// moored ship from most sides (#353), so the autopilot sails round to one that sees it.
    fn firing_spot(&self, target: Vec3, from: Vec3, radius: f32) -> Option<Vec3> {
        const BEARINGS: usize = 36;
        (0..BEARINGS)
            .map(|i| {
                let a = i as f32 / BEARINGS as f32 * std::f32::consts::TAU;
                target.with_y(0.0) + Vec3::new(a.cos(), 0.0, a.sin()) * radius
            })
            .filter(|c| self.open(*c) && self.clear(*c + Vec3::Y * GUN_HEIGHT, target))
            .min_by(|a, b| a.distance_squared(from).total_cmp(&b.distance_squared(from)))
    }
}

/// A mount's rest direction as an angle off the bow, in the heading convention (0 = bow, -Z; positive turns about +Y).
fn mount_angle(rot: Quat) -> f32 {
    let d = rot * Vec3::NEG_Z;
    (-d.x).atan2(-d.z)
}

/// The heading that turns the gun nearest the bow (of `guns`, angles off the bow) onto `bow_on`, the heading that points the bow at the
/// target. Mission 2's sloop holds bow-on with its forward Lancer; mission 6's command ship has only broadside guns and must turn its side
/// to the stockade gate (#353). No guns: bow-on.
fn tower_face(bow_on: f32, guns: &[f32]) -> f32 {
    guns.iter().copied().min_by(|a, b| a.abs().total_cmp(&b.abs())).map_or(bow_on, |a| bow_on - a)
}

/// Fraction of the mortar's maximum range the dev autopilot fires from (PROVISIONAL).
const LOB_REACH: f32 = 0.8;
/// Distance at which the dev autopilot counts the objective point as reached (PROVISIONAL).
const POINT_REACHED: f32 = 150.0;

/// How far from a defended objective point the dev autopilot looks for its attackers (PROVISIONAL).
const DEFEND_RADIUS: f32 = 600.0;

/// True for an objective text that tells the player to guard something (dev autopilot heuristic, PROVISIONAL: English task texts).
fn defend_task(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    ["defend", "protect", "keep the pirates from"].iter().any(|w| t.contains(w))
}

/// Where the objective marker is: a ship it follows ('Set Current Objective Point On Ship', mission 9's St Roch), else the point.
fn objective_at(sc: &Script, data: &Value) -> Option<Vec3> {
    sc.point_pos.or_else(|| sc.point.map(|i| game_to_bevy(&data["points"][i]["Position"])))
}

/// A task to dock, by its name or its text (mission 9's "M09E - Return To Ship" reads "Return to and dock with Command Ship").
fn is_dock_task(t: &Task) -> bool {
    t.name.to_ascii_lowercase().contains("dock") || t.text.to_ascii_lowercase().contains("dock")
}

/// The dev autopilot is running for the objective point instead of fighting (see `flee_task`).
fn fleeing(sc: &Script) -> bool {
    (sc.point.is_some() || sc.point_pos.is_some()) && sc.tasks.iter().any(|t| t.active && !t.complete && flee_task(&t.text))
}

/// True for an objective text that tells the player to get away rather than fight (dev autopilot heuristic, PROVISIONAL: English task texts).
fn flee_task(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    ["take cover", "avoid", "escape", "flee", "alive", "retreat"].iter().any(|w| t.contains(w))
}

/// `--script mission`: for any active task named `Tow<name>` (mission 2's `TowCoventry`), orders a tow of the ship whose name ends with
/// `<name>` (as the menu's Tow would) unless the player already tows or is on the way; once linked, the autopilot sails for the objective point.
/// The bot's progress check: where the ship was at the last check, and a detour it is on.
/// The bot's memory between frames (one per `update` system, so it starts fresh with each app).
#[derive(Default)]
pub(super) struct State {
    stuck: Option<Stuck>,
    /// (last call, since when, the damage then) for the tower hold.
    tower_stall: Option<(f32, f32, f32)>,
    /// The game minute last logged.
    minute: u32,
    /// What the bot did last frame (`autopilot` logs a change).
    intent: &'static str,
    /// Route cells: whether a hull there touches land (filled as the planner asks).
    blocked: std::collections::HashMap<(i32, i32), bool>,
    /// The target the fleet was last ordered to attack.
    helped: Option<Entity>,
    /// The planned route: the goal's cell and the path to it.
    route: Option<((i32, i32), Vec<Vec3>)>,
}

/// Route grid cell size (units), hull clearance added to the ship's radius, the planner's cell budget and how far (cells) the goal may move
/// before a new plan. PROVISIONAL bot tuning.
const CELL: f32 = 20.0;
const HULL_MARGIN: f32 = 5.0;
const MAX_EXPAND: usize = 30_000;
const REPLAN_CELLS: i32 = 3;
const WEDGED_CELLS: i32 = 3;
/// Beyond this distance to its goal the bot sails at Emergency Speed (PROVISIONAL bot tuning).
const EMERGENCY_DIST: f32 = 300.0;

fn cell_of(p: Vec3) -> (i32, i32) {
    ((p.x / CELL).floor() as i32, (p.z / CELL).floor() as i32)
}

fn cell_centre(c: (i32, i32)) -> Vec3 {
    Vec3::new((c.0 as f32 + 0.5) * CELL, 0.0, (c.1 as f32 + 0.5) * CELL)
}

/// Whether a hull of radius `hull` in cell `c` touches land (the sailing contact test, `collision::touches`).
fn blocked(memo: &mut std::collections::HashMap<(i32, i32), bool>, land: &[&crate::collision::Obstacle], hull: f32, c: (i32, i32)) -> bool {
    *memo.entry(c).or_insert_with(|| {
        let bounds = [Vec3::new(-hull, -2.0, -hull), Vec3::new(hull, 6.0, hull)];
        crate::collision::touches(bounds, &Transform::from_translation(cell_centre(c)), land)
    })
}

/// Whether the straight course from `a` to `b` stays off land, past the ship's own cell (it may lie against the shore).
fn line_clear(memo: &mut std::collections::HashMap<(i32, i32), bool>, land: &[&crate::collision::Obstacle], hull: f32, a: Vec3, b: Vec3) -> bool {
    let len = a.with_y(0.0).distance(b.with_y(0.0));
    let steps = (len / (CELL * 0.5)).ceil() as i32;
    (2..steps).all(|i| !blocked(memo, land, hull, cell_of(a.lerp(b, i as f32 / steps as f32))))
}

/// A* over the route cells from `from` to `to` (8 neighbours). The start cell may touch land (a ship against the shore) and arriving within
/// two cells of the goal is enough (a base stands on land). None when the budget runs out.
fn plan(memo: &mut std::collections::HashMap<(i32, i32), bool>, land: &[&crate::collision::Obstacle], hull: f32, from: Vec3, to: Vec3) -> Option<Vec<Vec3>> {
    use std::cmp::Reverse;
    let (start, goal) = (cell_of(from), cell_of(to));
    let near = |c: (i32, i32), e: (i32, i32)| (c.0 - e.0).abs() <= 2 && (c.1 - e.1).abs() <= 2;
    let h = |c: (i32, i32)| (((c.0 - goal.0).pow(2) + (c.1 - goal.1).pow(2)) as f32).sqrt();
    let mut open = std::collections::BinaryHeap::new();
    let mut came: std::collections::HashMap<(i32, i32), ((i32, i32), f32)> = std::collections::HashMap::from([(start, (start, 0.0))]);
    open.push((Reverse((h(start) * 1000.0) as i64), start));
    let mut expanded = 0;
    while let Some((_, c)) = open.pop() {
        if near(c, goal) {
            let mut path = vec![to];
            let mut k = c;
            while k != start {
                path.push(cell_centre(k));
                k = came[&k].0;
            }
            path.reverse();
            return Some(path);
        }
        expanded += 1;
        if expanded > MAX_EXPAND {
            return None;
        }
        let g = came[&c].1;
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
            let n = (c.0 + dx, c.1 + dz);
            if blocked(memo, land, hull, n) {
                continue;
            }
            let ng = g + if dx != 0 && dz != 0 { std::f32::consts::SQRT_2 } else { 1.0 };
            if came.get(&n).is_none_or(|(_, old)| ng < *old) {
                came.insert(n, (c, ng));
                open.push((Reverse(((ng + h(n)) * 1000.0) as i64), n));
            }
        }
    }
    None
}

/// Where to steer for `goal`: the goal itself when the straight course is clear, else the farthest point of the planned route the ship can
/// sail to in a straight line (the plan is kept until the goal moves `REPLAN_CELLS`). No route found: the goal (`unstick` takes over).
fn route(st: &mut State, land: &[&crate::collision::Obstacle], hull: f32, from: Vec3, goal: Vec3) -> Vec3 {
    if line_clear(&mut st.blocked, land, hull, from, goal) {
        st.route = None;
        return goal;
    }
    let gc = cell_of(goal);
    if st.route.as_ref().is_none_or(|(c, _)| (c.0 - gc.0).abs() > REPLAN_CELLS || (c.1 - gc.1).abs() > REPLAN_CELLS) {
        let path = plan(&mut st.blocked, land, hull, from, goal);
        info!("autopilot route to {goal:.0}: {}", path.as_ref().map_or("none found".into(), |p| format!("{} points", p.len())));
        st.route = Some((gc, path.unwrap_or_default())); // a failed plan is kept too, not retried every frame
    }
    let Some((_, path)) = st.route.take().filter(|(_, p)| !p.is_empty()) else { return goal };
    let next = path.iter().rev().find(|q| line_clear(&mut st.blocked, land, hull, from, **q)).copied().unwrap_or(path[0]);
    st.route = Some((gc, path));
    next
}

/// The ship wedged at `at` steering for `toward`: the next WEDGED_CELLS cells that way count as land from now on, and the route is planned
/// again. A gap the grid thinks open but the ship cannot pass (a tow behind it, a shore the hull box misses) was otherwise planned through
/// again after every detour (mission 11).
fn wedged(st: &mut State, at: Vec3, toward: Vec3) {
    let dir = (toward - at).with_y(0.0).normalize_or_zero();
    for i in 1..=WEDGED_CELLS {
        st.blocked.insert(cell_of(at + dir * CELL * i as f32), true);
    }
    st.route = None;
}

#[derive(Default)]
struct Stuck {
    since: f32,
    at: Vec3,
    detour: Option<(f32, Vec3)>,
    side: f32,
    /// Detours since the ship last got nearer its goal, and the nearest it got.
    tries: u32,
    best: f32,
}

/// PROVISIONAL bot tuning: a ship heading for a goal farther than STUCK_GOAL that moved less than STUCK_MOVE in STUCK_SECS is
/// pressed against land (missions 2 and 3 wedge on islands). Under 1 unit/s: mission 11's flagship sails at 2.1, and the old 40 units read
/// every turn as wedged, sending it off on detours away from "Pursue Silver 2". It backs off to one side, DETOUR_STEP further each time it wedges again
/// without getting PROGRESS nearer the goal, so it works along an island chain; after MAX_TRIES it tries the other side. A detour
/// lasts until reached or its length at DETOUR_SPEED units a second has passed.
const STUCK_SECS: f32 = 20.0;
const STUCK_MOVE: f32 = 20.0;
const STUCK_GOAL: f32 = 150.0;
const DETOUR_STEP: f32 = 250.0;
const DETOUR_SPEED: f32 = 4.0;
const PROGRESS: f32 = 200.0;
const MAX_TRIES: u32 = 4;
/// How long a stalled tower hold sails for its new hold point.
const DETOUR_SECS: f32 = 30.0;

/// PROVISIONAL bot tuning: a tower hold whose target's damage has not changed for HOLD_STALL_SECS moves TOWER_ROUND radians round it.
const HOLD_STALL_SECS: f32 = 30.0;
/// A tower hold or firing spot this close counts as reached: stop and turn a gun on (PROVISIONAL).
const SPOT_REACHED: f32 = 20.0;
const TOWER_ROUND: f32 = 1.0;

/// True once a hold has gone HOLD_STALL_SECS without the target's damage changing; a gap in the calls (the hold was left) starts over.
fn hold_stalled(time: f32, damage: f32, cell: &mut Option<(f32, f32, f32)>) -> bool {
    let lock = cell;
    let (last, since, then) = lock.get_or_insert((time, time, damage));
    if time < *last || time - *last > 2.0 || (damage - *then).abs() > 1e-4 {
        (*since, *then) = (time, damage);
    }
    *last = time;
    if time - *since >= HOLD_STALL_SECS {
        *since = time;
        return true;
    }
    false
}

/// Sends the bot to `d` for DETOUR_SECS, whatever its goal.
fn start_detour(time: f32, d: Vec3, cell: &mut Option<Stuck>) {
    let lock = cell;
    let st = lock.get_or_insert_with(|| Stuck { since: time, at: d, side: 1.0, best: f32::MAX, ..default() });
    st.detour = Some((time + DETOUR_SECS, d));
    info!("autopilot hold stalled; detour to {d:.0}");
}

/// A detour point while the ship is wedged on its way to `goal`, else None (the goal stands).
fn unstick(time: f32, pos: Vec3, goal: Option<Vec3>, cell: &mut Option<Stuck>) -> Option<Vec3> {
    let lock = cell;
    let st = lock.get_or_insert_with(|| Stuck { since: time, at: pos, side: 1.0, best: f32::MAX, ..default() });
    // A new run (game time went back) starts over.
    if time < st.since {
        *st = Stuck { since: time, at: pos, side: 1.0, best: f32::MAX, ..default() };
    }
    if let Some((until, d)) = st.detour {
        if time < until && d.distance(pos) > 40.0 {
            return Some(d);
        }
        st.detour = None;
        st.since = time;
        st.at = pos;
    }
    let goal = goal.filter(|g| g.distance(pos) > STUCK_GOAL)?;
    if pos.distance(st.at) >= STUCK_MOVE {
        st.since = time;
        st.at = pos;
        return None;
    }
    if time - st.since < STUCK_SECS {
        return None;
    }
    let dist = goal.distance(pos);
    if dist < st.best - PROGRESS {
        st.tries = 0;
    }
    st.best = st.best.min(dist);
    st.tries += 1;
    if st.tries > MAX_TRIES {
        st.tries = 1;
        st.side = -st.side;
    }
    let to = (goal - pos).with_y(0.0).normalize_or_zero();
    let side = Vec3::new(-to.z, 0.0, to.x) * st.side;
    let step = DETOUR_STEP * st.tries as f32;
    let d = pos + (side - to * 0.5) * step;
    st.detour = Some((time + step / DETOUR_SPEED, d));
    info!("autopilot stuck at {pos:.0} heading for {goal:.0}; detour to {d:.0}");
    Some(d)
}

/// The ship a `Tow...` task asks the player to tow: the one the task name ends with, else the named ship its text mentions (mission 4's
/// "Tow tender to safety" reads "Tow the RLS Bachanalia to the Rally Point.", #351). A task whose text alone starts with "Tow" counts
/// too: mission 4's "MO4 free stuck sloop" reads "Tow the RLS Yorkshire Clear of the Island it has Run Aground On.". The text may
/// misspell the name (mission 4's "Tow the RLS Indominable ..." for the RLS Indomitable), so the first six letters of the name's last
/// word are enough.
fn task_towee<'a>(t: &super::eval::Task, w: &'a [Unit], player: Entity) -> Option<&'a Unit> {
    let name = t.name.strip_prefix("Tow").filter(|n| !n.is_empty());
    if name.is_none() && !t.text.starts_with("Tow ") {
        return None;
    }
    let candidates = || w.iter().filter(|u| u.alive && u.e != player && !u.name.is_empty());
    let stem = |u: &&Unit| u.name.rsplit(' ').next().and_then(|w| w.get(..6)).is_some_and(|w| t.text.contains(w));
    name.and_then(|n| candidates().find(|u| u.name.ends_with(n))).or_else(|| candidates().find(|u| t.text.contains(u.name.as_str()))).or_else(|| candidates().find(stem))
}

fn autopilot_tow(sc: &Script, w: &[Unit], data: &Value, extra: &mut Extra) {
    let Some(p) = w.iter().find(|u| u.primary && u.alive) else { return };
    if sc.nis || !extra.towing_player.is_empty() || !extra.docking_player.is_empty() || sc.tows().iter().any(|t| t.0 == p.e) {
        return;
    }
    if let Some(towee) = sc.tasks.iter().filter(|t| t.active && !t.complete).find_map(|t| task_towee(t, w, p.e)) {
        extra.dock.write(DockMsg::TowStart { tower: None, towee: towee.e });
        return;
    }
    // Otherwise a visible ship an unfired rule waits for the player to tow (mission 11's SS Harvester at the black hole), once the
    // objective point is reached or there is none: like `wants`, it never outranks the point. Mission 11's tow rule has no gate, and
    // sailing for the Harvester straight after Cydonia skipped the pirate trap that starts the ambush.
    if objective_at(sc, data).is_some_and(|at| at.distance(p.pos) > POINT_REACHED) {
        return;
    }
    let towee = sc
        .wants(w)
        .into_iter()
        .filter_map(|x| match x {
            Want::Tow(e) => w.iter().find(|u| u.e == e && u.visible),
            _ => None,
        })
        .nearest_to(p.pos, |u| u.pos);
    if let Some(towee) = towee {
        extra.dock.write(DockMsg::TowStart { tower: None, towee: towee.e });
    }
}

/// `--script mission`: for any active task named `Dock...` (mission 2's camps), once the player is near the task's objective point
/// (the point carries the task's name), orders the dock with the base nearest that point, as a click on it would. Mission 1's
/// tender has its own hook above.
fn autopilot_dock(sc: &Script, w: &[Unit], data: &Value, extra: &mut Extra) {
    const NEAR: f32 = 600.0;
    const BASE_RANGE: f32 = 400.0;
    let Some(p) = w.iter().find(|u| u.primary && u.alive) else { return };
    if !extra.docking_player.is_empty() || sc.nis || sc.task_active("Dock With Tender") {
        return;
    }
    let dock_task = |t: &&Task| t.active && !t.complete && is_dock_task(t);
    for t in sc.tasks.iter().filter(dock_task) {
        // A task whose objective point carries its name (mission 2's camps): the base nearest that point.
        let Some(at) = data["points"].as_array().and_then(|a| a.iter().find(|x| x["Name"] == t.name.as_str())).map(|x| game_to_bevy(&x["Position"])) else { continue };
        if at.distance(p.pos) > NEAR {
            continue;
        }
        let base = extra.bases.iter().filter(|(_, _, tf, _)| tf.translation.distance(at) < BASE_RANGE).nearest_to(at, |b| b.2.translation);
        if let Some((dockee, ..)) = base {
            extra.dock.write(DockMsg::Dock { docker: None, dockee });
            return;
        }
    }
    // Otherwise whatever an unfired rule waits for the player to dock with, once the player is near it: while a task says to dock, or
    // when the target sits at the objective point (mission 5's "Deliver Supplies to Alamein" is a dock at Alamein).
    let point = objective_at(sc, data);
    if let Some((dockee, at)) = dock_target(sc, w, p.pos).filter(|(_, at)| at.distance(p.pos) < NEAR)
        && (sc.tasks.iter().any(|t| dock_task(&t)) || point.is_some_and(|q| q.distance(at) < BASE_RANGE))
    {
        extra.dock.write(DockMsg::Dock { docker: None, dockee });
    }
}

/// The nearest of the rules' dock targets (`Script::dock_targets`) and where it is.
fn dock_target(sc: &Script, w: &[Unit], from: Vec3) -> Option<(Entity, Vec3)> {
    sc.dock_targets(w).into_iter().filter_map(|e| w.iter().find(|u| u.e == e && u.alive).map(|u| (e, u.pos))).nearest_to(from, |t| t.1)
}

/// `--script mission`: follows the objective point, then closes on the nearest hostile and targets it (smoke test of the rules).
fn autopilot(
    sc: &Script,
    w: &[Unit],
    data: &Value,
    rocks: &Query<(Entity, &Transform, &Asteroid), (Without<Ship>, Without<crate::environment::SpaceAnimal>)>,
    bases: &[(Entity, Vec3, Vec3)],
    lifeboats: &[(Vec3, &str)],
    ships: &mut Query<RuleShip>,
    target: &mut Target,
    mortars: &[Mortar],
    sight: &Sight,
    lib: &Library,
    st: &mut State,
) {
    /// Distance the dev autopilot stops from a live gun tower, bow on it (PROVISIONAL; the retail test fires from 100 units).
    const TOWER_HOLD: f32 = 110.0;
    /// Closest the autopilot lets itself creep to a gun tower before backing out (PROVISIONAL).
    const TOWER_MIN: f32 = 70.0;
    if sc.nis || sc.ended.is_some() {
        return;
    }
    let Some(p) = w.iter().find(|u| u.primary && u.alive) else { return };
    // Ships only: whales count as hostile until the reprimand rule (PROVISIONAL default), and chasing them stalled the run after Objective 3.
    let foe = w.iter().filter(|u| u.alive && u.visible && sc.hostile(&u.owner, &p.owner) && ships.contains(u.e)).nearest_to(p.pos, |a| a.pos);
    // An enemy towing a friendly ship away (mission 10's Plunder takes the convoy's lead ship) comes first, wherever it is: the convoy
    // waits until the tow breaks. Only once it is visible: mission 1's hidden Pirate Schooner tows the tender in distress from the start,
    // and chasing it then pulls the player away from the fleet before the opening is over.
    let kidnapper = sc.tows().iter().find_map(|&(tower, towee)| {
        let (t, v) = (w.iter().find(|u| u.e == tower)?, w.iter().find(|u| u.e == towee)?);
        (t.alive && t.visible && sc.hostile(&t.owner, &p.owner) && !sc.hostile(&v.owner, &p.owner)).then_some(t)
    });
    // A task to defend what sits at the objective point (mission 12's "Defend Her Majesty Aboard Parliament"): the enemy closest to that
    // point comes before whatever attacks the player, or the boarders capture it while the bot duels submarines.
    let raider = objective_at(sc, data).filter(|_| sc.tasks.iter().any(|t| t.active && !t.complete && defend_task(&t.text))).and_then(|at| {
        w.iter().filter(|u| u.alive && u.visible && sc.hostile(&u.owner, &p.owner) && ships.contains(u.e)).nearest_to(at, |u| u.pos).filter(|u| u.pos.distance(at) < DEFEND_RADIUS)
    });
    // What the unfired rules wait for (`Script::wants`): a volume to enter or a unit to shoot (mission 5's blockaders) is where to sail
    // once the objective point is reached or there is none (mission 4's and 8's searches). It never outranks the point: optional
    // rules (mission 1's whales) would pull the bot off its course.
    let wants = sc.wants(w);
    let wanted_hit = wants
        .iter()
        .filter_map(|x| match x {
            Want::Hit(e) => w.iter().find(|u| u.e == *e),
            _ => None,
        })
        .nearest_to(p.pos, |u| u.pos);
    let wanted_at = wants
        .iter()
        .filter_map(|x| match x {
            Want::Reach(at) => Some(*at),
            _ => None,
        })
        .chain(wanted_hit.map(|u| u.pos))
        .nearest_to(p.pos, |a| *a);
    // A shell the rules want landed in a volume (mission 9's Dark Matter): close to within the mortar's reach, then fire it at the
    // centroid as a click with the lob cursor would. Gunnery drops the shot once fired; the next follows until the rule fires.
    // A mount whose arc covers the point fires; with none, the ship turns its bow on it (an immobile ship may never manage).
    let range = mortars.iter().map(|m| m.range).fold(0.0, f32::max);
    let covering = |at: Vec3| {
        mortars
            .iter()
            .find(|m| {
                let local = m.rot.inverse() * (at - p.pos).with_y(0.0);
                at.distance(p.pos) < m.range && (-local.x).atan2(-local.z).abs() <= m.arc / 2.0
            })
            .map(|m| m.bank)
    };
    let lob_points: Vec<Vec3> = wants
        .iter()
        .filter_map(|x| match x {
            Want::Lob(at) => Some(*at),
            _ => None,
        })
        .filter(|_| !fleeing(sc) && !mortars.is_empty())
        .collect();
    // The first point (the nearest is listed first) a mount covers, else the nearest one to sail or turn for.
    let lob = lob_points.iter().find_map(|at| covering(*at).map(|bank| (*at, range, Some(bank)))).or_else(|| lob_points.first().map(|at| (*at, range, None)));
    if let Some((at, _, Some(bank))) = lob
        && target.2.shot.is_none()
    {
        target.2.shot = Some((bank, at));
    }
    let kidnapper = kidnapper.or(raider);
    // Hostile bases (mission 2's island gun towers) must be shot down before they can be docked.
    let tower = w
        .iter()
        .filter(|u| u.alive && sc.hostile(&u.owner, &p.owner))
        .filter_map(|u| bases.iter().find(|b| b.0 == u.e).copied())
        .filter(|(_, at, _)| at.distance(p.pos) < 900.0)
        .min_by(|a, b| a.1.distance(p.pos).total_cmp(&b.1.distance(p.pos)));
    target.1 = None; // no click point unless a tower is the target (set below)
    let dock = sc.task_active("Dock With Tender");
    let shoot_rocks = sc.task_active("Destroy Asteroids");
    let rock = rocks.iter().filter(|(_, _, a)| a.group == "Asteroid Belt" && a.hp > 0.0).nearest_to(p.pos, |a| a.1.translation);
    let mut face: Option<f32> = None; // hold position with the bow on this heading (a gun tower in reach)
    // A task that says to run (mission 3's "Take Cover in the Asteroid Field", "Avoid the Pirate Trap"): sail for the objective point and
    // leave the enemy alone. The player cannot sink under PROCYON_AUTOINVULN, so fighting there never ends and the escorts die.
    let flee = fleeing(sc);
    let foe = foe.filter(|_| !flee);
    let tower = tower.filter(|_| !flee);
    // A tower or gate the island hides from here, with the nearest open-water spot that sees it.
    let tower_spot = tower.filter(|t| !sight.clear(p.pos + Vec3::Y * GUN_HEIGHT, t.1)).and_then(|t| sight.firing_spot(t.1, p.pos, TOWER_HOLD - 15.0).map(|s| (t, s)));
    // What the bot is doing this frame, logged when it changes.
    let intent;
    let goal = if let (true, Some((e, tf, _))) = (shoot_rocks, rock.filter(|r| r.1.translation.distance(p.pos) < 400.0)) {
        intent = "shoot a rock";
        target.0 = Some(e);
        Some(tf.translation)
    } else if let Some((at, range, covered)) = lob {
        intent = "mortar a volume";
        // In reach: hold; with no mount covering the point, bow on it (gunnery drops a shot outside the mount's arc).
        let d = at.distance(p.pos);
        let stopped = ships.iter().find(|s| s.primary).is_some_and(|s| s.ship.speed.abs() < 0.1);
        if covered.is_none() && (d <= range * LOB_REACH || (d < range && stopped)) {
            face = Some((-(at.x - p.pos.x)).atan2(-(at.z - p.pos.z)));
        }
        Some(if face.is_some() || covered.is_some() { p.pos } else { at })
    } else if let Some(from) = wants.iter().find_map(|x| match x {
        Want::Leave(at) => Some(*at),
        _ => None,
    }) {
        intent = "leave a volume";
        // Out of the volume, straight away from its centre (mission 11: dragging the galleon clear of the black hole).
        Some(p.pos + (p.pos - from).normalize_or(Vec3::X) * LEAVE_STEP)
    } else if let Some(foe) = kidnapper.filter(|_| !flee).or(foe.filter(|f| f.pos.distance(p.pos) < 700.0)) {
        intent = "fight a ship";
        target.0 = Some(foe.e);
        // A ship moored behind an island (mission 6's pirates round the stockade) is not shot through it: sail round to a spot that
        // sees it once close; farther out, straight at it.
        let hidden = foe.pos.distance(p.pos) < FOE_SIGHT_RANGE && !sight.clear(p.pos + Vec3::Y * GUN_HEIGHT, foe.pos + Vec3::Y * GUN_HEIGHT);
        Some(hidden.then(|| sight.firing_spot(foe.pos + Vec3::Y * GUN_HEIGHT, p.pos, FOE_SPOT)).flatten().unwrap_or(foe.pos))
    } else if let Some(((e, at, local), spot)) = tower_spot {
        intent = "shoot a base from a firing spot";
        // The tower or gate is behind the island from here: sail to the nearest open-water spot that sees it (#353). Once there, stop and
        // turn a gun onto it: creeping forward (order 1) from the spot slid the hull along the island and the guns never bore.
        target.0 = Some(e);
        target.1 = Some(local);
        if spot.distance(p.pos) < SPOT_REACHED {
            face = Some(tower_face((-(at.x - p.pos.x)).atan2(-(at.z - p.pos.z)), &sight.guns));
        }
        Some(if face.is_some() { p.pos } else { spot })
    } else if let Some((e, at, local)) = tower {
        intent = "shoot a base";
        // Aim at the tower being approached, as a click on it would: without a click point the guns pick the tower nearest the
        // island centre, which can be the other one, out of the Lancer's arc or range.
        target.0 = Some(e);
        target.1 = Some(local);
        // Straight at the live tower, then hold with the gun nearest the bow on it (the bow itself for a forward gun), keeping that heading.
        // Holding creeps forward (order 1), so a ship that closes inside `TOWER_MIN` (roof too steep above, hull against the island)
        // sails back out to the hold distance before it turns bow-on again.
        let at = at.with_y(0.0);
        let off = p.pos - at;
        let d = off.length();
        let backing_out = ships.iter().find(|s| s.primary).is_some_and(|s| !s.ship.waypoints.is_empty());
        // Up to SPOT_REACHED beyond the hold distance counts: a hull pinned on the island just outside it never closed the gap (#353).
        if (TOWER_MIN..=TOWER_HOLD + SPOT_REACHED).contains(&d) && (!backing_out || d >= TOWER_HOLD - 15.0) {
            // Heading 0 faces -Z, rotating about +Y; turned so a gun that can hit bases bears.
            face = Some(tower_face((-(at.x - p.pos.x)).atan2(-(at.z - p.pos.z)), &sight.guns));
            // A hold that lands no hit (hull pinned on the island with the tower out of the gun's reach) moves round the tower.
            let damage = w.iter().find(|u| u.e == e).map_or(0.0, |u| u.damage);
            if hold_stalled(sc.time, damage, &mut st.tower_stall) {
                let round = Quat::from_rotation_y(TOWER_ROUND) * off.normalize_or(Vec3::X);
                start_detour(sc.time, at + round * TOWER_HOLD * 1.3, &mut st.stuck);
            }
        }
        Some(if face.is_some() { p.pos } else { at + off.normalize_or(Vec3::X) * TOWER_HOLD })
    } else if let Some((at, _)) = lifeboats.iter().filter(|b| sc.hostile(b.1, &p.owner) && b.0.distance(p.pos) < 1500.0).nearest_to(p.pos, |b| b.0) {
        intent = "collect a lifeboat";
        Some(*at) // touching an enemy lifeboat collects it (mission 2's PickUpLifeboats counts it as a capture)
    } else if dock {
        intent = "dock with the tender";
        w.iter().find(|u| u.name == "RLS Bounty").map(|u| u.pos)
    } else {
        intent = "objective point, then what the rules wait for";
        // No objective point (mission 8's "Dock with Mining Camp"): sail for what the rules wait for the player to dock with.
        // An objective point already reached that nothing answers (mission 5's "Dock @ Elzrand" waits for a warning shot) gives way
        // to what the rules want.
        objective_at(sc, data)
            .filter(|at| at.distance(p.pos) > POINT_REACHED || wanted_at.is_none())
            .or_else(|| sc.tasks.iter().any(|t| t.active && !t.complete && is_dock_task(t)).then(|| dock_target(sc, w, p.pos).map(|t| t.1)).flatten())
            .or(wanted_at)
            // Still nowhere to go (mission 10's "Defend Merchant Fleet"): stay with the nearest friendly ship outside the player's fleet.
            // Only once the ship is idle: a course the rules gave (mission 1's "Player Ordered Around Archipelago" path) wins.
            .or_else(|| {
                let idle = ships.iter().find(|s| s.primary).is_some_and(|s| s.ship.waypoints.is_empty());
                w.iter()
                    .filter(|_| idle)
                    .filter(|u| u.alive && u.visible && u.owner != p.owner && !sc.hostile(&u.owner, &p.owner) && ships.contains(u.e))
                    .nearest_to(p.pos, |u| u.pos)
                    .filter(|u| u.pos.distance(p.pos) < 2000.0)
                    .map(|u| u.pos)
            })
    };
    // Sailing for a unit the rules want shot (the fallback below the objective point): it is the target as well.
    if let (Some(g), Some(u)) = (goal, wanted_hit)
        && g == u.pos
    {
        target.0 = Some(u.e);
    }
    if std::mem::replace(&mut st.intent, intent) != intent {
        info!("autopilot intent: {intent} (target {:?}, goal {:.0?})", target.0.and_then(|e| w.iter().find(|u| u.e == e)).map(|u| u.name.as_str()), goal);
    }
    // Once a game minute: what the bot is after, for reading a stalled autoplay log (#68).
    let minute = (sc.time / 60.0) as u32;
    if std::mem::replace(&mut st.minute, minute) != minute {
        let helm = ships
            .iter()
            .find(|s| s.primary)
            .map(|s| (s.ship.order, s.ship.waypoints.first().map(|w| w.round()), s.ship.throttle, s.ship.speed, s.ship.cap, s.ship.heading, face, mortars.len(), target.2.shot));
        let target_damage = target.0.and_then(|e| w.iter().find(|u| u.e == e)).map(|u| (u.name.as_str(), u.damage));
        info!(
            "autopilot t={:.0} pos={:.0} goal={:.0?} target={:?} target_name_damage={:?} helm(order,wp,throttle,speed,cap,heading,face,mortar,shot)={:?} wants={:?}",
            sc.time,
            p.pos,
            goal,
            target.0,
            target_damage,
            helm,
            (wants.len(), &wants[..wants.len().min(6)])
        );
    }
    // Dev shortcut for end-to-end runs: PROCYON_AUTOKILL=1 sinks the pirate once the player is close.
    if std::env::var_os("PROCYON_AUTOKILL").is_some()
        && let Some(wr) =
            w.iter().find(|u| u.name == "Warrior" && u.alive && u.visible && u.pos.distance(p.pos) < 250.0 && sc.hits_by.get(&(PLAYER.to_string(), u.e)).is_some_and(|n| *n >= 3))
        && let Ok(mut x) = ships.get_mut(wr.e)
    {
        x.health.core = 0.0;
    }
    // Dev switch for unattended runs (tools/autoplay.py): PROCYON_AUTOINVULN=1 keeps the player's fleet (every ship the player owns) from
    // taking damage, so the bot survives mission 2's island towers and mission 3's escort outlives the chase, and the later rules get
    // exercised. Re-applied every tick, overriding the rules' own vulnerability changes for those ships; a "lost" outcome then only
    // comes from rules that do not need a player ship to sink.
    if std::env::var_os("PROCYON_AUTOINVULN").is_some() {
        for mut s in ships.iter_mut().filter(|s| s.mission.owner == p.owner) {
            s.mission.invuln = 1.0;
        }
    }
    // Round the islands rather than into them (a human plans their own route; the original has no planner for ships).
    let hull = ships.iter().find(|s| s.primary).map_or(10.0, |s| lib.classes[s.ship.class].radius) + HULL_MARGIN;
    let unrouted = goal;
    let goal = if face.is_none() { goal.map(|g| route(st, &sight.land, hull, p.pos, g)) } else { goal };
    let before = st.stuck.as_ref().and_then(|s| s.detour.map(|d| d.1));
    let detour = unstick(sc.time, p.pos, goal.filter(|_| face.is_none()), &mut st.stuck);
    if let (Some(_), Some(g)) = (detour.filter(|d| Some(*d) != before), goal) {
        wedged(st, p.pos, g);
    }
    if detour.is_some() {
        face = None;
    }
    let goal = detour.or(goal);
    if let Some(mut s) = ships.iter_mut().find(|s| s.primary)
        && let Some(g) = goal
    {
        if let Some(h) = face {
            s.ship.waypoints.clear();
            s.ship.goal = Some(h);
            s.ship.order = 1;
            s.ship.throttle = None;
        } else if g.distance(p.pos) > 60.0 || unrouted.is_some_and(|u| u.distance(p.pos) > 60.0) || foe.is_none() {
            // Near the goal itself (not a route point on the way, mission 6's shore) the bot creeps instead (order 1).
            s.ship.waypoints = vec![g];
            // Emergency Speed while the goal is far (a human's "+"; the computer captains run in at it too), Ahead Full for the last stretch
            // so the slower Emergency turn does not overshoot it.
            s.ship.order = if unrouted.is_some_and(|u| u.distance(p.pos) > EMERGENCY_DIST) { 3 } else { 2 };
            s.ship.goal = None;
            s.ship.throttle = None; // a move order releases a manual throttle (the opening NIS leaves it at 0)
        } else {
            s.ship.waypoints.clear();
            s.ship.order = 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wedged_bot_backs_off_sideways_then_resumes_its_goal() {
        let mut cell = None;
        let goal = Some(Vec3::new(1000.0, 0.0, 0.0));
        let at = Vec3::ZERO;
        // Moving: no detour.
        assert_eq!(unstick(0.0, at, goal, &mut cell), None);
        assert_eq!(unstick(10.0, Vec3::new(100.0, 0.0, 0.0), goal, &mut cell), None);
        // Pinned in place past STUCK_SECS: a detour behind and to one side of the course.
        let pinned = Vec3::new(100.0, 0.0, 0.0);
        assert_eq!(unstick(25.0, pinned, goal, &mut cell), None);
        let d = unstick(31.0, pinned, goal, &mut cell).expect("detour");
        assert!(d.x < pinned.x && d.z.abs() > 100.0, "{d:?}");
        // Kept until it runs out, then the goal stands again.
        assert_eq!(unstick(35.0, pinned, goal, &mut cell), Some(d));
        let over = 31.0 + DETOUR_STEP / DETOUR_SPEED + 1.0;
        assert_eq!(unstick(over, pinned, goal, &mut cell), None);
        // Wedged again no nearer the goal: the same side, a longer step; after MAX_TRIES the other side.
        let mut t = over + STUCK_SECS + 1.0;
        let d2 = unstick(t, pinned, goal, &mut cell).expect("second detour");
        assert!(d2.z * d.z > 0.0 && d2.distance(pinned) > d.distance(pinned), "{d:?} {d2:?}");
        let mut last = d2;
        for _ in 2..=MAX_TRIES {
            t += 10_000.0 / DETOUR_SPEED;
            assert_eq!(unstick(t, pinned, goal, &mut cell), None, "the detour ran out");
            t += STUCK_SECS + 1.0;
            last = unstick(t, pinned, goal, &mut cell).expect("another detour");
        }
        assert!(last.z * d.z < 0.0 && (last.distance(pinned) - d.distance(pinned)).abs() < 1.0, "{d:?} {last:?}");
        // A near goal never counts as wedged.
        let mut cell = None;
        assert_eq!(unstick(0.0, at, Some(Vec3::new(100.0, 0.0, 0.0)), &mut cell), None);
        assert_eq!(unstick(60.0, at, Some(Vec3::new(100.0, 0.0, 0.0)), &mut cell), None);
        // A tower hold stalls only when the target's damage stays put for HOLD_STALL_SECS of continuous holding.
        let mut hold = None;
        let mut ticks = |from: f32, to: f32, damage: f32| (0..=((to - from) as i32)).any(|i| hold_stalled(from + i as f32, damage, &mut hold));
        assert!(!ticks(0.0, 20.0, 0.5));
        assert!(!ticks(21.0, 45.0, 0.6), "damage rising restarts the clock");
        assert!(ticks(46.0, 60.0, 0.6));
        assert!(!ticks(100.0, 120.0, 0.6), "leaving the hold starts over");
    }

    #[test]
    fn the_bot_routes_round_an_island_instead_of_into_it() {
        // A wall of land from z = -200 to 200 at x = 50, between the ship at the origin and its goal at x = 150.
        let (a, b, c, d) = (Vec3::new(50.0, -20.0, -200.0), Vec3::new(50.0, 60.0, -200.0), Vec3::new(50.0, 60.0, 200.0), Vec3::new(50.0, -20.0, 200.0));
        let (w1, w2) = (crate::collision::Obstacle::from_triangles(&[[a, b, c]]), crate::collision::Obstacle::from_triangles(&[[a, c, d]]));
        let land = [&w1, &w2];
        let (from, goal) = (Vec3::ZERO, Vec3::new(150.0, 0.0, 0.0));
        let mut st = State::default();
        let next = route(&mut st, &land, 15.0, from, goal);
        assert!(next.z.abs() > 200.0, "steers for the wall's end, not through it: {next}");
        let (_, path) = st.route.clone().unwrap();
        assert_eq!(*path.last().unwrap(), goal);
        assert!(path.windows(2).all(|p| line_clear(&mut st.blocked, &land, 15.0, p[0], p[1])), "every leg is clear");
        // Wedged on the way round: the cells it was steering into are closed and the next plan keeps out of them.
        let at = Vec3::new(30.0, 0.0, next.z.signum() * 180.0);
        let closed: Vec<_> = (1..=WEDGED_CELLS).map(|i| cell_of(at + (next - at).with_y(0.0).normalize() * CELL * i as f32)).collect();
        wedged(&mut st, at, next);
        assert!(st.route.is_none(), "a new plan");
        route(&mut st, &land, 15.0, at, goal);
        let (_, path) = st.route.clone().unwrap();
        assert!(!path.is_empty() && path.iter().all(|q| !closed.contains(&cell_of(*q))), "{closed:?} {path:?}");
        // Open water: straight at the goal, no plan.
        let mut st = State::default();
        assert_eq!(route(&mut st, &[], 15.0, from, goal), goal);
        assert!(st.route.is_none());
    }

    /// #353: the stockade gate and the pirates moored round the island are hidden from most sides; the autopilot picks the nearest
    /// open-water spot with a clear shot instead of firing into the island.
    #[test]
    fn a_target_behind_an_island_is_shot_from_a_spot_that_sees_it() {
        // An island: a tall wall at x = 50 from z = -200 to 200, and a ground slab under x 50..60 (land, not water).
        let (h, l) = (Vec3::new(50.0, -20.0, -200.0), Vec3::new(50.0, 60.0, 200.0));
        let wall = crate::collision::Obstacle::from_triangles(&[
            [h, Vec3::new(50.0, 60.0, -200.0), l],
            [h, l, Vec3::new(50.0, -20.0, 200.0)],
            [Vec3::new(50.0, 0.0, -200.0), Vec3::new(60.0, 0.0, -200.0), Vec3::new(60.0, 0.0, 200.0)],
            [Vec3::new(50.0, 0.0, -200.0), Vec3::new(60.0, 0.0, 200.0), Vec3::new(50.0, 0.0, 200.0)],
        ]);
        let sight = Sight { guns: vec![], land: vec![&wall] };
        let target = Vec3::new(100.0, GUN_HEIGHT, 0.0);
        assert!(!sight.clear(Vec3::new(0.0, GUN_HEIGHT, 0.0), target), "the wall hides it from the west");
        assert!(sight.clear(Vec3::new(150.0, GUN_HEIGHT, 0.0), target));
        assert!(!sight.open(Vec3::new(55.0, 0.0, 0.0)) && sight.open(Vec3::new(0.0, 0.0, 0.0)));
        let spot = sight.firing_spot(target, Vec3::new(0.0, 0.0, 0.0), 40.0).expect("a spot east of the wall");
        assert!(spot.x > 60.0 && (spot.distance(target.with_y(0.0)) - 40.0).abs() < 1e-3, "{spot}");
        assert!(sight.clear(spot + Vec3::Y * GUN_HEIGHT, target));
        // A section just inside the hull still counts as seen (shells strike a hair past the surface).
        assert!(sight.clear(Vec3::new(0.0, GUN_HEIGHT, 0.0), Vec3::new(50.0 + SIGHT_SLACK * 0.5, GUN_HEIGHT, 0.0)));
    }

    #[test]
    fn a_tower_is_faced_with_the_gun_nearest_the_bow() {
        use std::f32::consts::FRAC_PI_2;
        let (port, starboard) = (Quat::from_rotation_y(FRAC_PI_2), Quat::from_rotation_y(-FRAC_PI_2));
        assert!((mount_angle(Quat::IDENTITY)).abs() < 1e-5);
        assert!((mount_angle(port) - FRAC_PI_2).abs() < 1e-5);
        // Mission 2: a forward gun keeps the bow on the tower.
        assert!((tower_face(1.0, &[mount_angle(port), 0.0, mount_angle(starboard)]) - 1.0).abs() < 1e-5);
        // Mission 6 (#353): broadside guns only, so the side turns to the gate; the gun's world direction is the bow-on heading.
        let h = tower_face(1.0, &[mount_angle(port), mount_angle(starboard)]);
        let dir = Quat::from_rotation_y(h) * port * Vec3::NEG_Z;
        let want = Quat::from_rotation_y(1.0) * Vec3::NEG_Z;
        assert!(dir.distance(want) < 1e-4, "{dir} points at {want}");
        assert_eq!(tower_face(0.5, &[]), 0.5);
    }

    #[test]
    fn a_tow_task_names_its_ship_by_name_suffix_or_in_its_text() {
        let task = |name: &str, text: &str| super::super::eval::Task { name: name.into(), text: text.into(), active: true, complete: false, failed: false };
        let unit = |n: u32, name: &str| super::super::eval::Unit {
            e: Entity::from_raw_u32(n).unwrap(),
            id: n as i64,
            name: name.into(),
            owner: "Navy".into(),
            owner_is_named_group: false,
            groups: vec![],
            pos: Vec3::ZERO,
            alive: true,
            visible: true,
            primary: n == 1,
            damage: 0.0,
            flag: String::new(),
            attack: vec![],
        };
        let w = [unit(1, "RLS Sea Sparrow"), unit(2, ""), unit(3, "RLS Bachanalia"), unit(4, "Harvester")];
        let me = w[0].e;
        // Mission 4 (#351): the task name says "tender", the text names the ship.
        let m4 = task("Tow tender to safety", "Tow the RLS Bachanalia to the Rally Point.");
        assert_eq!(task_towee(&m4, &w, me).map(|u| u.e), Some(w[2].e));
        assert_eq!(task_towee(&task("TowHarvester", ""), &w, me).map(|u| u.e), Some(w[3].e), "a name suffix still wins");
        assert!(task_towee(&task("Tow the wreck", "Tow the wreck home."), &w, me).is_none(), "an unnamed ship never matches the text");
        assert!(task_towee(&task("Protect the RLS Bachanalia", "Protect the RLS Bachanalia."), &w, me).is_none(), "only Tow tasks");
        let w = [unit(1, "RLS Sea Sparrow"), unit(5, "RLS Yorkshire")];
        let sloop = task("MO4 free stuck sloop", "Tow the RLS Yorkshire Clear of the Island it has Run Aground On.");
        assert_eq!(task_towee(&sloop, &w, me).map(|u| u.e), Some(w[1].e), "a task whose text says Tow");
        let w = [unit(1, "RLS Sea Sparrow"), unit(6, "RLS Indomitable")];
        let mow = task("rescue bh man o war", "Tow the RLS Indominable Clear of the Vanov Black Hole.");
        assert_eq!(task_towee(&mow, &w, me).map(|u| u.e), Some(w[1].e), "the misspelt name still matches");
    }

    #[test]
    fn autopilot_flees_on_cover_and_escape_objectives_but_fights_the_rest() {
        assert!(flee_task("Take Cover in the Asteroid Field."));
        assert!(flee_task("Avoid the Pirate Trap. Return to Crescentia Right Away."));
        assert!(!flee_task("Hold off Pirate Attack until Repairs are Complete."));
        assert!(!flee_task("Destroy Both Camp Cannon Towers."));
        assert!(defend_task("Defend Her Majesty Aboard Parliament."));
        assert!(defend_task("Keep the Pirates From Firing on Dorlinor."));
        assert!(!defend_task("Search Nearby Islands for Ships."));
    }
}
