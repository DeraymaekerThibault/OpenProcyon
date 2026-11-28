//! Boarding and grappling (docs/re/boarding.md): crew strengths, the grapple link, the boarding fight and capture.
//! A ship boards a hostile ship only while grappled to it (harpoon), a base only while beside one of its boarding points and slow;
//! the fight then runs as in the original: one casualty per interval, a coin flip for who takes it, until a side's
//! strength is gone. The winner captures: the loser's owner changes (mission rules count it) and the grapple is released.
//! PROVISIONAL: crew skill (generic crew combat ability, no leader bonus, no per-man crews), the harpoon flight (no projectile; the hook
//! lands after distance / muzzle speed), the winch that pulls grappled hulls together, AI boarding of boardable
//! bases (the original has no AI path to the command), first-mate lines picked by meaning, and the boarding status bar placement.
use crate::Nearest;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use serde_json::Value;
use std::sync::OnceLock;

use crate::audio::Sfx;
use crate::campaign::{Mission, MissionBase, MissionShip};
use crate::collision::Drift;
use crate::combat::{Health, Library, SimRng};
use crate::data::conv;
use crate::fx::FxLib;
use crate::{PlayerShip, Ship, Side, assets_dir};

mod ai;
mod fight;
mod grapple;
mod odds;
mod order;
use ai::ai_boarding;
pub use fight::Boarders;
use fight::{fights, handle_messages, per_tick, say, set_flags, settled};
pub use grapple::Ropes;
use grapple::{
    Body, HARPOON_SPEED, cancel_grapple_order, grapple_limit, grapple_orders, harpoon_bears, has_harpoon, hook_flight, in_range, radius, ropes, velocity, winch_pull, winch_upkeep,
};
use odds::points;
pub use odds::{Outcome, Skill, Strength, casualty, counts, interval, register, settle};
use order::board_orders;
/// A ship or base that can be boarded. `on` is the target's Boardable flag (WOT 'Boardable' / 'Set Group/Unit Boardable').
#[derive(Component, Clone, Debug)]
pub struct Boardable {
    pub on: bool,
    pub strength: Strength,
    pub skill: Skill,
    counts: [u32; 8],
    points: Vec<Vec3>,
}
impl Boardable {
    pub fn ship(entity: &str, skill: Skill, on: bool) -> Self {
        let counts = counts(entity);
        Self { on, strength: Strength::ship(counts, skill.member()), skill, counts, points: points(entity) }
    }
    fn replace_strength(&mut self, mut strength: Strength) {
        let losses = (self.strength.full - self.strength.defend).max(0);
        strength.defend = (strength.defend - losses).max(0);
        strength.normalise();
        self.strength = strength;
    }

    pub fn reset_crew(&mut self) {
        self.replace_strength(Strength::ship(self.counts, self.skill.member()));
    }

    /// Named crew replace generic hands; `bonus` (`crew_arms::leadership_bonus`) lifts every slot but the captain's and first mate's.
    pub fn apply_posts(&mut self, posts: &crate::crew_arms::CrewPosts, bonus: f32) {
        let mut totals = [0.0; 8];
        for (role, total) in totals.iter_mut().enumerate() {
            let b = if matches!(role, 1 | 2) { 0.0 } else { bonus };
            let members = posts.combat(role, self.skill.member());
            let hands = self.counts[role].saturating_sub(members.len() as u32) as f32;
            *total = members.iter().map(|v| (v + b).min(1.0)).sum::<f32>() + hands * (self.skill.member() + b).min(1.0);
        }
        let fighters = (totals[6] * 10.0).round() as i32;
        let crew = (totals.iter().enumerate().filter(|(i, _)| *i != 6).map(|(_, v)| v).sum::<f32>() * 10.0).round() as i32;
        self.replace_strength(Strength { fighters, crew, attack: crew / 2 + fighters, defend: crew + fighters, full: crew + fighters, base: false });
    }

    pub fn base(entity: &str, combat: i32, on: bool) -> Self {
        Self { on, strength: Strength::base(combat), skill: Skill::Average, counts: [0; 8], points: points(entity) }
    }
}

/// The boarding fight this ship (the boarder) is in.
#[derive(Component)]
pub struct Boarding {
    target: Entity,
    interval: f32,
    since: f32,
    /// False while the order waits for the grapple to bring the hulls into range; once in range, leaving it ends the fight.
    live: bool,
    fx: Vec<Entity>,
}
impl Boarding {
    pub fn target(&self) -> Entity {
        self.target
    }
    /// The hulls are in range and the crews are fighting.
    pub fn live(&self) -> bool {
        self.live
    }
}

/// What a boarding status bar shows: the friendly side's share of the two strengths still standing (the boarder's attack, the
/// target's defend).
pub fn friendly_share(boarder: &Strength, target: &Strength, target_friendly: bool) -> f32 {
    let (a, d) = (boarder.attack.max(0) as f32, target.defend.max(0) as f32);
    let (mine, theirs) = if target_friendly { (d, a) } else { (a, d) };
    if mine + theirs > 0.0 { mine / (mine + theirs) } else { 0.5 }
}
/// A harpoon on its way (`flight` > 0) or the grapple it became: `grappler` holds `target`. Each grapple is its own entity, so a ship
/// with several harpoon banks can hold several (up to `grapple_limit`), at most one on a given target.
#[derive(Component)]
pub struct Grapple {
    grappler: Entity,
    target: Entity,
    flight: f32,
    /// Centre distance when the hook bit (0 until then).
    span: f32,
    /// Rope length: the distance at the hit, shortened while a boarding reels (0 until the hook bit).
    length: f32,
    /// Per end (grappler, target): the hull axis the other ship lay along when the hook bit, pointing toward it.
    sides: [Vec3; 2],
}
impl Grapple {
    fn new(grappler: Entity, target: Entity, flight: f32) -> Self {
        Grapple { grappler, target, flight, span: 0.0, length: 0.0, sides: [Vec3::ZERO; 2] }
    }
}

/// The hull axis (local bow-stern or beam, signed) closest to the direction toward `other`: "grappled on the front" or "on the side".
fn side_of(tf: &Transform, other: Vec3) -> Vec3 {
    let d = tf.rotation.inverse() * (other - tf.translation);
    if d.z.abs() >= d.x.abs() { Vec3::Z * d.z.signum() } else { Vec3::X * d.x.signum() }
}
/// The Grapple order: steer for `target` until it is inside harpoon range, then fire.
#[derive(Component)]
pub struct GrappleOrder(pub(crate) Entity);

#[derive(Message, Debug)]
pub enum BoardMsg {
    /// Every order names its ship: the original has no "the player's ship" default (#699).
    Board {
        boarder: Entity,
        target: Entity,
    },
    Unboard {
        boarder: Entity,
    },
    Grapple {
        grappler: Entity,
        target: Entity,
    },
    Ungrapple {
        grappler: Entity,
    },
    /// 'Set Group/Unit Boardable'.
    Boardable(Entity, bool),
    /// Not retail: the mission rule `OP Board Ship`. The boarder takes the order a player gives by hand, a grapple and then the
    /// boarding once the hook has bitten (`order::board_orders`); a boarder with no harpoon, or a target it is not hostile to, only logs.
    Order {
        boarder: Entity,
        target: Entity,
    },
}
/// A capture, for the mission rules' capture counts (owner names; skirmish sides are "Player" / "Enemy").
#[derive(Message)]
pub struct Captured {
    pub by: crate::names::Owner,
    pub victim: String,
    pub unit: Entity,
}
#[derive(Message)]
struct Settled {
    boarder: Entity,
    target: Entity,
    outcome: Outcome,
}

/// What the player's ship can do to each ship right now, for the options menu (refreshed every frame).
#[derive(Resource, Default)]
pub struct BoardStatus {
    can_grapple: bool,
    /// Per ship: (the player is boarding it, the player may board it, the player holds a grapple on it).
    targets: std::collections::HashMap<Entity, (bool, bool, bool)>,
}
impl BoardStatus {
    /// The Board slot: (shows stop, enabled). Stop is always enabled.
    pub fn board(&self, target: Entity) -> (bool, bool) {
        self.targets.get(&target).map_or((false, false), |s| if s.0 { (true, true) } else { (false, s.1) })
    }
    /// The Grapple slot: (shows stop, enabled).
    pub fn grapple(&self, target: Entity) -> (bool, bool) {
        self.targets.get(&target).map_or((false, false), |s| if s.2 { (true, true) } else { (false, self.can_grapple) })
    }
}
/// The boarding menu's state for the local player's ship (view).
fn status(boarders: Boarders, mut st: ResMut<BoardStatus>, ships: Query<Entity, With<Ship>>, player: Query<Entity, With<PlayerShip>>) {
    st.targets.clear();
    let Ok(me) = player.single() else { return };
    st.can_grapple = boarders.can_grapple(me);
    for e in ships.iter().filter(|e| *e != me) {
        st.targets.insert(e, (boarders.boarding(me, e), boarders.can_board(me, e), boarders.grappling(me, e)));
    }
}

pub fn plugin(app: &mut App) {
    // Backspace's grapple cancel is an order (`commands::Kind::DropGrapple`).
    app.add_message::<BoardMsg>()
        .init_resource::<crate::commands::Outbox>()
        .add_message::<Captured>()
        .add_message::<Settled>()
        .init_resource::<BoardStatus>()
        .init_resource::<Ropes>()
        // Orders only record an order (step 3); fights and the rope's break and reel test are the mission tick (6); the rope's pull is the
        // link keeper (7.1); the computer's decision to board runs after collision (8).
        .add_systems(FixedUpdate, (handle_messages, set_flags, grapple_orders, board_orders).chain().in_set(crate::battle::Step::Orders).in_set(crate::battle::BattleActive))
        .add_systems(FixedUpdate, (hook_flight, winch_upkeep, fights, settled).chain().in_set(crate::BattleSet::MissionTick).in_set(crate::battle::BattleActive))
        .add_systems(FixedUpdate, winch_pull.in_set(crate::battle::Step::Links).after(crate::docking::docked_hold).in_set(crate::battle::BattleActive))
        .add_systems(FixedUpdate, ai_boarding.in_set(crate::battle::Step::AlwaysOn).in_set(crate::battle::BattleActive))
        .add_systems(Update, (dev_board, cancel_grapple_order, status, ropes).chain().before(crate::input).in_set(crate::battle::BattleActive));
}

/// Whether two objects (by their sides, none for a thing without one) are hostile in the alliance table.
fn hostile_pair(table: &crate::alliance::Alliances, a: Option<Side>, b: Option<Side>) -> bool {
    a.zip(b).is_some_and(|(a, b)| table.hostile(a, b))
}
/// The side of a ship, else of a base (`Alliances::base_side`).
fn thing_side(table: &crate::alliance::Alliances, ship: Option<&Ship>, base: Option<&MissionBase>, side: Option<&crate::combat::BaseSide>) -> Option<Side> {
    ship.map(|s| s.side).or_else(|| table.base_side(base, side))
}

/// Dev hook `--script board`: the player grapples the nearest enemy and boards it as soon as the menu would allow (smoke test, screenshots).
fn dev_board(
    cfg: Res<crate::Config>,
    status: Res<BoardStatus>,
    mut msg: MessageWriter<BoardMsg>,
    mut step: Local<u8>,
    me: Query<(Entity, &Transform), With<PlayerShip>>,
    foes: Query<(Entity, &Transform, &Ship)>,
) {
    if cfg.script.as_deref() != Some("board") || cfg.frame < 5 {
        return;
    }
    let Ok((e, me)) = me.single() else { return };
    let Some((foe, ..)) = foes.iter().filter(|f| f.2.side != Side::Player).nearest_to(me.translation, |a| a.1.translation) else {
        return;
    };
    match *step {
        0 => {
            msg.write(BoardMsg::Grapple { grappler: e, target: foe });
            *step = 1;
        }
        1 if status.board(foe).1 => {
            msg.write(BoardMsg::Board { boarder: e, target: foe });
            *step = 2;
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;
