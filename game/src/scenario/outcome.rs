//! Battle outcome: the end rules' pass, the end message and the end line.
use super::*;

#[derive(Resource, PartialEq, Clone, Copy, Debug)]
pub enum Outcome {
    Playing,
    Victory,
    Defeat,
}

/// The end message (string id) of the map rule that decided the battle, for the Game Summary; none: the default VICTORY/DEFEAT.
#[derive(Resource, Default)]
pub struct EndMessage(pub Option<String>);

/// The string id of the line the battlefield shows while a decided skirmish winds down (retail: "Game Lost!  Exiting..." under the
/// camera's centre; the battle plays on under it until the Game Summary opens).
#[derive(Resource, Default)]
pub struct EndLine(pub Option<String>);

/// Seconds the end line shows before the Game Summary opens (read from the original's code: 10 s after the end rule fires; the
/// rules run once a second, and a 1 frame/s recording saw about 13 s from the sinking).
pub const END_LINE_SECS: f32 = 10.0;

/// The battlefield's game-over line (read from the original's code): the firing rule's own message for the human's side, else
/// "Game Won!" when the human is among the winners and "Game Lost!" otherwise. (Border Dispute's no-human-ships rule names "Game
/// Lost" for winners and losers alike, which is why that line showed under a VICTORY! summary.)
pub(super) fn end_line_id(outcome: Outcome, message: Option<&str>) -> Option<String> {
    (outcome != Outcome::Playing).then(|| message.map_or_else(|| format!("IDGS_TPINGAMEMESSAGE_GAME_{}", if outcome == Outcome::Victory { "WON" } else { "LOST" }), str::to_string))
}

/// How often the original tests the battle's end rules: a pass runs once the game clock is more than this many seconds past the last pass.
pub const RULE_PASS_SECS: f32 = 1.0;

/// The rule-pass timer of one battle (game seconds since it began, and the clock at the last pass; the first pass is the first
/// frame more than a second in. PROVISIONAL: the original's starting value of the last-pass time was not read).
#[derive(Default)]
pub struct RulePass {
    clock: f32,
    last: f32,
}
impl RulePass {
    /// Advances the clock by `dt`; true when the rules are tested this frame.
    pub fn due(&mut self, dt: f32) -> bool {
        self.clock += dt;
        let due = self.clock - self.last > RULE_PASS_SECS;
        if due {
            self.last = self.clock;
        }
        due
    }
}

#[allow(clippy::too_many_arguments)]
pub fn outcome(
    time: Res<Time>,
    fresh: Query<(), Added<Scenario>>,
    mut pass: Local<RulePass>,
    mut out: ResMut<Outcome>,
    mut message: ResMut<EndMessage>,
    mut line: ResMut<EndLine>,
    mut cfg: ResMut<Config>,
    player: Query<&Health, With<PlayerShip>>,
    ships: Query<(&Health, &Ship), Without<PlayerShip>>,
    rules: (Option<Res<victory::Victory>>, Option<Res<combat::Library>>, Option<Res<ship::Fleets>>, Option<Res<crate::rules::skirmish::SkirmishScript>>),
    all: Query<(&Health, &Ship, &Transform, Option<&ship::FleetMember>, Option<&MapObject>), Without<crate::lifeboats::Lifeboat>>,
    bases: Query<(&campaign::MissionBase, &combat::ArmedBase, &Health, Option<(&combat::BaseSide, &combat::BaseOrigin)>)>,
) {
    let enemies = || ships.iter().filter(|(_, s)| s.side != Side::Player).map(|(h, _)| h);
    // missions end through their own rules; a decided battle stays decided (the end flow despawns ships, which must not turn a
    // victory into a defeat)
    if campaign::is_mission(cfg.map.as_deref()) || *out != Outcome::Playing {
        return;
    }
    // The original tests its end rules once a second of game time, so a battle ends up to a second after the cause.
    if !fresh.is_empty() {
        *pass = RulePass::default();
    }
    if !pass.due(time.delta_secs()) {
        return;
    }
    let mut end_message = None;
    let new = if let (Some(victory), Some(lib)) = (&rules.0, &rules.1) {
        // The map's own rules decide: a base falling, the human fleet or every ship lost.
        let local = rules.2.as_ref().map(|f| f.local.as_str());
        let states: Vec<victory::ShipState> = all
            .iter()
            .map(|(h, s, tf, m, o)| victory::ShipState {
                side: s.side,
                start: o.map_or(s.side, |o| o.start),
                alive: !h.destroyed,
                human: m.is_some_and(|m| Some(m.0.as_str()) == local),
                id: o.and_then(|o| o.id),
                at: Vec2::new(tf.translation.x, -tf.translation.z),
            })
            .collect();
        let damage: Vec<(i64, f32)> = bases.iter().map(|(b, a, h, _)| (b.id, combat::base_mission_damage(&lib.classes[a.class], h))).collect();
        let captured: Vec<i64> = bases.iter().filter(|b| b.3.is_some_and(|(side, origin)| side.0 != origin.0)).map(|b| b.0.id).collect();
        let points = rules.3.as_ref().map(|s| s.points()).unwrap_or_default();
        let decided = victory.decide_with_message(&victory::Snapshot { ships: &states, bases: &damage, captured: &captured, points: &points });
        end_message = decided.as_ref().and_then(|d| d.1.clone());
        decided.map_or(Outcome::Playing, |d| d.0)
    } else if player.single().is_err() || player.single().is_ok_and(|h| h.destroyed) {
        Outcome::Defeat
    } else if enemies().count() > 0 && enemies().all(|h| h.destroyed) {
        Outcome::Victory
    } else {
        Outcome::Playing
    };
    if new != *out && new != Outcome::Playing {
        info!("{}", if new == Outcome::Victory { "VICTORY" } else { "DEFEAT" });
        cfg.ended = Some(cfg.frame);
    }
    *out = new;
    line.0 = end_line_id(new, end_message.as_deref());
    message.0 = end_message;
}
