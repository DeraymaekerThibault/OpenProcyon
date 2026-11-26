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

/// Seconds the end line shows before the Game Summary opens (10 s after the end rule fires; the
/// rules run once a second, and a 1 frame/s recording saw about 13 s from the sinking).
pub const END_LINE_SECS: f32 = 10.0;

/// The battlefield's game-over line: the firing rule's own message for the human's side, else
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
    player: Query<&Health, With<crate::commands::Commander>>,
    ships: Query<(&Health, &Ship), Without<crate::commands::Commander>>,
    rules: (
        Option<Res<victory::Victory>>,
        Option<Res<combat::Library>>,
        Option<Res<ship::Fleets>>,
        Option<Res<crate::rules::skirmish::SkirmishScript>>,
        Option<Res<campaign::Mission>>,
    ),
    all: Query<(Entity, &Health, &Ship, &Transform, Option<&ship::FleetMember>, Option<&MapObject>), Without<crate::lifeboats::Lifeboat>>,
    mut sunk: Local<Vec<(Entity, victory::ShipState)>>,
    bases: Query<(&campaign::MissionBase, &combat::ArmedBase, &Health, Option<(&combat::BaseSide, &combat::BaseOrigin)>)>,
    table: crate::alliance::Hostility,
) {
    let enemies = || ships.iter().filter(|(_, s)| !table.with_human(s.side)).map(|(h, _)| h);
    // A decided battle stays decided (the end flow despawns ships, which must not turn a victory into a defeat).
    if *out != Outcome::Playing {
        return;
    }
    // A mission ends when its rules end it ('End Game', its objectives), at once and through the same decision. PROVISIONAL: its end
    // speech and end text stay the rules' own, so it shows no end line.
    if let Some(m) = &rules.4 {
        if let Some((_, won)) = &m.script.ended {
            decide(if *won { Outcome::Victory } else { Outcome::Defeat }, None, &mut out, &mut message, &mut line, &mut cfg);
            line.0 = None;
        }
        return;
    }
    // The original tests its end rules once a second of game time, so a battle ends up to a second after the cause.
    if !fresh.is_empty() {
        *pass = RulePass::default();
        sunk.clear();
    }
    let local = rules.2.as_ref().map(|f| f.local.as_str());
    let state = |(_, h, s, tf, m, o): (Entity, &Health, &Ship, &Transform, Option<&ship::FleetMember>, Option<&MapObject>)| victory::ShipState {
        side: s.side,
        start: o.map_or(s.side, |o| o.start),
        alive: !h.destroyed,
        human: m.is_some_and(|m| Some(m.0.as_str()) == local),
        id: o.and_then(|o| o.id),
        at: Vec2::new(tf.translation.x, -tf.translation.z),
    };
    // A sunk ship leaves the world a tick after its break-up, so every frame notes the destroyed ones: a pass a second apart could
    // miss a ship that sank and went between two passes.
    for a in all.iter().filter(|a| a.1.destroyed) {
        let s = state(a);
        match sunk.iter_mut().find(|k| k.0 == a.0) {
            Some(k) => k.1 = s,
            None => sunk.push((a.0, s)),
        }
    }
    let gone: Vec<victory::ShipState> = sunk.iter().filter(|k| !all.contains(k.0)).map(|k| k.1.clone()).collect();
    if !pass.due(time.delta_secs()) {
        return;
    }
    let mut end_message = None;
    let new = if let (Some(victory), Some(lib)) = (&rules.0, &rules.1) {
        // The map's own rules decide: a base falling, the human fleet or every ship lost.
        let states: Vec<victory::ShipState> = all.iter().map(state).chain(gone.iter().cloned()).collect();
        let damage: Vec<(i64, f32)> = bases.iter().map(|(b, a, h, _)| (b.id, combat::base_mission_damage(&lib.classes[a.class], h))).collect();
        let captured: Vec<i64> = bases.iter().filter(|b| b.3.is_some_and(|(side, origin)| side.0 != origin.0)).map(|b| b.0.id).collect();
        let points = rules.3.as_ref().map(|s| s.points()).unwrap_or_default();
        let snap = victory::Snapshot { ships: &states, bases: &damage, captured: &captured, points: &points, table: &table };
        // The map's end rules first, then a script that decides its own game (`"victory": "script"`, rules::skirmish): an End Game,
        // Team X Wins or Remaining Team Wins rule, or a team's required objectives.
        let decided = victory.decide_with_message(&snap).or_else(|| rules.3.as_ref().and_then(|s| s.script.end.as_ref()).map(|end| victory.decide_end(&snap, end)));
        end_message = decided.as_ref().and_then(|d| d.1.clone());
        decided.map_or(Outcome::Playing, |d| d.0)
    } else if player.single().is_err() || player.single().is_ok_and(|h| h.destroyed) {
        Outcome::Defeat
    } else if enemies().count() + gone.iter().filter(|g| !table.with_human(g.side)).count() > 0 && enemies().all(|h| h.destroyed) {
        Outcome::Victory
    } else {
        Outcome::Playing
    };
    decide(new, end_message, &mut out, &mut message, &mut line, &mut cfg);
}

/// Records the battle's outcome: the one end path of a decided battle (`outcome`'s pass, and the developer console's `win` / `lose`).
/// Logs the first decision, stamps the frame the Game Summary countdown starts from, and sets the end line and end message.
pub fn decide(new: Outcome, end_message: Option<String>, out: &mut Outcome, message: &mut EndMessage, line: &mut EndLine, cfg: &mut Config) {
    if new != *out && new != Outcome::Playing {
        info!("{}", if new == Outcome::Victory { "VICTORY" } else { "DEFEAT" });
        cfg.ended = Some(cfg.frame);
    }
    *out = new;
    line.0 = end_line_id(new, end_message.as_deref());
    message.0 = end_message;
}

/// The developer console's `win` / `lose` outside a campaign mission: ends a still-undecided battle as a victory or defeat for the
/// human side through [`decide`]. A decided battle stays decided; returns whether it ended the battle.
pub fn force_outcome(world: &mut World, won: bool) -> bool {
    if !(world.contains_resource::<Outcome>() && world.contains_resource::<EndMessage>() && world.contains_resource::<EndLine>() && world.contains_resource::<Config>()) {
        return false;
    }
    let mut state = bevy::ecs::system::SystemState::<(ResMut<Outcome>, ResMut<EndMessage>, ResMut<EndLine>, ResMut<Config>)>::new(world);
    let Ok((mut out, mut message, mut line, mut cfg)) = state.get_mut(world) else { return false };
    if *out != Outcome::Playing {
        return false;
    }
    decide(if won { Outcome::Victory } else { Outcome::Defeat }, None, &mut out, &mut message, &mut line, &mut cfg);
    true
}
