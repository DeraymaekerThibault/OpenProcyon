//! `--script commands` (`tools/battle_check.py --replay`): gives the player's ship a fixed list of orders through `commands::Orders`,
//! as the keys and clicks do, including one sent while the battle is paused, so a recording of the run has something to replay.
use crate::commands::{Engine, Kind, Orders};
use crate::{EngineOrder, PlayerShip, Ship};
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
    player: Query<(Entity, &Ship, &Transform), With<PlayerShip>>,
    ships: Query<(Entity, &Ship, &crate::combat::Health, &Transform), Without<PlayerShip>>,
    mut orders: Orders,
    mut p: Local<Progress>,
) {
    if cfg.script != Some(super::Script::Commands) || loading.is_some() {
        return;
    }
    let Ok((me, mine, at)) = player.single() else { return };
    p.time += vtime.delta_secs();
    let enemy = || {
        ships
            .iter()
            .filter(|(_, s, h, _)| s.side != mine.side && !h.destroyed)
            .min_by(|a, b| a.3.translation.distance_squared(at.translation).total_cmp(&b.3.translation.distance_squared(at.translation)))
            .map(|(e, ..)| e)
    };
    let due = |t: f32| p.time >= t;
    let order = match p.step {
        0 if due(1.0) => Some(Kind::Engine { ship: me, engine: Engine::To(EngineOrder::Full) }),
        1 if due(4.0) => Some(Kind::Target { ship: me, target: enemy(), spot: Some(Vec3::ZERO) }),
        2 if due(8.0) => {
            vtime.pause();
            None
        }
        3 => {
            p.paused_frames += 1;
            (p.paused_frames >= 3).then_some(Kind::Engine { ship: me, engine: Engine::To(EngineOrder::Emergency) })
        }
        4 => {
            p.paused_frames += 1;
            if p.paused_frames < 6 {
                return;
            }
            vtime.unpause();
            None
        }
        5 if due(14.0) => Some(Kind::Engine { ship: me, engine: Engine::Slower }),
        6 if due(20.0) => Some(Kind::Engine { ship: me, engine: Engine::Faster }),
        7 if due(26.0) => Some(Kind::Target { ship: me, target: None, spot: None }),
        8 if due(32.0) => Some(Kind::Target { ship: me, target: enemy(), spot: None }),
        _ => return,
    };
    if p.step == 3 && order.is_none() {
        return;
    }
    if let Some(order) = order {
        info!("--script commands: step {} {order:?}", p.step);
        orders.send(order);
    }
    p.step += 1;
}
