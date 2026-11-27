//! `--script commands` (`tools/battle_check.py --replay`): gives the player's ship a fixed list of orders through `commands::Orders`,
//! as the keys and clicks do, including one sent while the battle is paused, so a recording of the run has something to replay.
//! Every human whose orders come from this machine gets the same list, each order sent as that player (#369 phase 2 E): in a lobby
//! with two humans, the second's orders are the script's, as a peer's would arrive; in a replay with `--as-player N`, only N's.
use crate::commands::{Commander, Engine, Kind, Orders, Replay};
use crate::{EngineOrder, Ship};
use bevy::prelude::*;

/// What the script does next, by game seconds since the battle loaded (paused steps count frames).
#[derive(Default)]
pub struct Progress {
    time: f32,
    step: usize,
    paused_frames: u32,
}

#[allow(clippy::type_complexity)]
pub fn orders(
    cfg: Res<crate::Config>,
    loading: Option<Res<crate::loading::Loading>>,
    mut vtime: ResMut<Time<Virtual>>,
    commanders: Query<(Entity, &Commander, &Ship, &Transform)>,
    ships: Query<(Entity, &Ship, &crate::combat::Health, &Transform)>,
    replay: Option<Res<Replay>>,
    mut orders: Orders,
    mut p: Local<Progress>,
) {
    if cfg.script != Some(super::Script::Commands) || loading.is_some() {
        return;
    }
    // In player order, so the orders go out (and are recorded) in one order on every run.
    let mut live: Vec<_> = commanders.iter().filter(|c| replay.as_deref().is_none_or(|r| r.takes_live(c.1.0))).collect();
    live.sort_by_key(|c| c.1.0);
    if live.is_empty() {
        return;
    }
    p.time += vtime.delta_secs();
    let due = |t: f32| p.time >= t;
    // The step's clock and pause actions, once a frame; then the step's order for each live command ship.
    let at_time = [1.0, 4.0, 8.0, 0.0, 0.0, 14.0, 20.0, 26.0, 32.0, 36.0, 38.0, 44.0, 48.0, 52.0, 54.0, 58.0, 60.0, 64.0, 68.0, 70.0, 71.0, 76.0];
    match p.step {
        2 if due(at_time[2]) => vtime.pause(),
        3 => {
            p.paused_frames += 1;
            if p.paused_frames < 3 {
                return;
            }
        }
        4 => {
            p.paused_frames += 1;
            if p.paused_frames < 6 {
                return;
            }
            vtime.unpause();
        }
        n if n < at_time.len() && due(at_time[n]) => {}
        _ => return,
    }
    for &(me, commander, mine, at) in &live {
        if let Some(order) = order(p.step, me, mine, at, &ships) {
            info!("--script commands: step {} player {} {order:?}", p.step, commander.0.0);
            orders.send_as(commander.0, order);
        }
    }
    p.step += 1;
}

/// Step `step`'s order for the command ship `me` (none for the steps that only pause or resume).
fn order(step: usize, me: Entity, mine: &Ship, at: &Transform, ships: &Query<(Entity, &Ship, &crate::combat::Health, &Transform)>) -> Option<Kind> {
    let enemy = || {
        ships
            .iter()
            .filter(|(_, s, h, _)| s.side != mine.side && !h.destroyed)
            .min_by(|a, b| a.3.translation.distance_squared(at.translation).total_cmp(&b.3.translation.distance_squared(at.translation)))
            .map(|(e, ..)| e)
    };
    match step {
        0 => Some(Kind::Engine { ship: me, engine: Engine::To(EngineOrder::Full) }),
        1 => Some(Kind::Target { ship: me, target: enemy(), spot: Some(Vec3::ZERO) }),
        3 => Some(Kind::Engine { ship: me, engine: Engine::To(EngineOrder::Emergency) }),
        5 => Some(Kind::Engine { ship: me, engine: Engine::Slower }),
        6 => Some(Kind::Engine { ship: me, engine: Engine::Faster }),
        7 => Some(Kind::Target { ship: me, target: None, spot: None }),
        8 => Some(Kind::Target { ship: me, target: enemy(), spot: None }),
        9 => Some(Kind::Move { ship: me, to: at.translation + at.forward() * 300.0, append: false }),
        10 => Some(Kind::Move { ship: me, to: at.translation + at.right() * 300.0, append: true }),
        11 => Some(Kind::Cancel { ship: me }),
        12 => Some(Kind::Hold { ship: me, on: true }),
        13 => Some(Kind::Hold { ship: me, on: false }),
        14 => Some(Kind::Engine { ship: me, engine: Engine::To(EngineOrder::Full) }),
        15 => Some(Kind::Stop { ship: me }),
        16 => Some(Kind::Formation { formation: crate::ship::Formation::Diamond, hold: true }),
        17 | 18 => Some(Kind::CeaseFire),
        19 => Some(Kind::Target { ship: me, target: enemy(), spot: None }),
        20 => Some(Kind::FleetAttack),
        21 => Some(Kind::PanicFire { ship: me }),
        _ => None,
    }
}
