//! Cloaking (docs/re/cloaking.md): the Procyon submersible's cloak, ordered from the right-click menu (CLOAK_START/STOP), by the
//! rules ('Set Group/Unit Cloak State', mission 12) or by its own AI. Four states (off, cloaking, fully cloaked, decloaking) with
//! a 5 s fade each way and a 5 s idle gate before the next start; viewers see the hull at `1 - level`, or at no less than a third
//! when their team knows the ship (its own team at once, enemies by lookout chance). PROVISIONAL: the detection cadence and
//! reach, the AI's cloak/decloak rule, the decloak by projectile impulse and the "dive" offset (not traced) are the remake's.
use std::collections::HashMap;

use bevy::prelude::*;

use crate::audio::Sfx;
use crate::combat::{Health, Hit, Library, Splashed};
use crate::environment::ScanFactor;
use crate::ship::Fleets;
use crate::{PlayerShip, Ship, Side};

/// Seconds a cloak takes to close, and the idle time after creation or a finished decloak before it may start again.
pub const FADE: f32 = 5.0;
const IDLE_GATE: f32 = 5.0;
/// Speed and thrust fall to `1 - SLOWING` at full cloak.
const SLOWING: f32 = 0.6;
/// Opacity a team that knows a cloaked ship still sees, as a floor.
const KNOWN_FLOOR: f32 = 0.33;
/// An enemy ship within this share of the cloaked ship's sight range may spot it. PROVISIONAL (the original's range is unresolved).
const DETECT_REACH: f32 = 0.5;
/// The AI decloaks when a hostile is this close to its guns' range. PROVISIONAL.
const AI_REVEAL: f32 = 1.5;
const AI_PERIOD: f32 = 1.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum State {
    #[default]
    Off,
    Cloaking,
    Full,
    Decloaking,
}

#[derive(Component)]
pub struct Cloak {
    pub state: State,
    /// Seconds in the current state.
    elapsed: f32,
    /// Level when the decloak began and the seconds it takes (a stop during cloaking decloaks over the time already spent).
    from: f32,
    length: f32,
    /// Seconds since the ship was created or last finished decloaking.
    idle: f32,
    /// The rules' 'AI Can Override' (stored whatever the rule says; the default lets every computer sub cloak by itself).
    pub ai_modify: bool,
    /// Teams that see the ship; filled while it is fully cloaked, pruned to its own team when a cloak starts.
    pub knows: Vec<Side>,
    ai_in: f32,
    // Presentation.
    shown: Option<Entity>,
    sound: Option<Entity>,
    seen_state: State,
}
impl Default for Cloak {
    fn default() -> Self {
        Cloak { state: State::Off, elapsed: 0.0, from: 0.0, length: FADE, idle: 0.0, ai_modify: true, knows: vec![], ai_in: 0.0, shown: None, sound: None, seen_state: State::Off }
    }
}
impl Cloak {
    /// A ship that begins the battle fully cloaked (smokes), or plain.
    pub fn new(cloaked: bool) -> Self {
        Cloak { state: if cloaked { State::Full } else { State::Off }, idle: IDLE_GATE, seen_state: if cloaked { State::Full } else { State::Off }, ..default() }
    }
    /// 0 visible .. 1 fully cloaked.
    pub fn level(&self) -> f32 {
        match self.state {
            State::Off => 0.0,
            State::Cloaking => (self.elapsed / FADE).min(1.0),
            State::Full => 1.0,
            State::Decloaking => self.from * (1.0 - self.elapsed / self.length.max(1e-3)).max(0.0),
        }
    }
    pub fn can_start(&self) -> bool {
        self.state == State::Off && self.idle >= IDLE_GATE
    }
    /// The menu shows Start in off and decloaking, Stop in cloaking and fully cloaked.
    pub fn stop_shown(&self) -> bool {
        matches!(self.state, State::Cloaking | State::Full)
    }
    /// The order to cloak; false when it is not allowed now.
    pub fn start(&mut self, side: Side) -> bool {
        if !self.can_start() {
            return false;
        }
        (self.state, self.elapsed) = (State::Cloaking, 0.0);
        self.knows = vec![side]; // pruned to the team's own ships (mutual allies share a side)
        true
    }
    /// The order to decloak: accepted while cloaking or fully cloaked.
    pub fn stop(&mut self) -> bool {
        if !self.stop_shown() {
            return false;
        }
        self.from = self.level();
        self.length = self.from * FADE;
        (self.state, self.elapsed) = (State::Decloaking, 0.0);
        true
    }
    /// How opaque the hull is to a viewer of `team`.
    pub fn alpha_for(&self, team: Side) -> f32 {
        let open = 1.0 - self.level();
        if self.knows.contains(&team) { KNOWN_FLOOR + (1.0 - KNOWN_FLOOR) * open } else { open }
    }
    /// Fully gone to a viewer of `team` (cursor picking, labels, contacts and every AI target filter skip it).
    pub fn hidden_from(&self, team: Side) -> bool {
        self.state == State::Full && !self.knows.contains(&team)
    }
    /// Guns that cannot fire while cloaked hold from the first moment of cloaking until the decloak is over.
    pub fn holds_fire(&self) -> bool {
        self.state != State::Off
    }
    /// Speed and thrust multiplier.
    pub fn speed_factor(&self) -> f32 {
        1.0 - SLOWING * self.level()
    }
}

pub fn plugin(app: &mut App) {
    app.add_systems(FixedUpdate, (attach, advance, ai, slow).chain().in_set(crate::BattleSet::Commands).run_if(in_state(crate::menu::Screen::Battle)))
        .add_systems(Update, (present, fade).chain().run_if(in_state(crate::menu::Screen::Battle)))
        .add_systems(Update, cloak_smoke.before(present).run_if(in_state(crate::menu::Screen::Battle)));
}

/// Every cloakable hull carries the keeper from the start (retail: each ship has one).
fn attach(mut commands: Commands, lib: Res<Library>, new: Query<(Entity, &Ship), (Added<Ship>, Without<Cloak>)>) {
    for (e, ship) in &new {
        if lib.classes[ship.class].cloakable {
            commands.entity(e).insert(Cloak::default());
        }
    }
}

fn advance(
    time: Res<Time>,
    mut rng: Local<Option<crate::combat::Rng>>,
    mut splashes: MessageReader<Splashed>,
    lib: Res<Library>,
    mut ships: Query<(Entity, &mut Cloak, &Ship, &Transform, &Health)>,
    watchers: Query<(&Ship, &Transform, &Health, Option<&ScanFactor>)>,
) {
    let dt = time.delta_secs();
    let rng = rng.get_or_insert_with(crate::combat::Rng::default);
    let splashed: Vec<Entity> = splashes.read().map(|s| s.0).collect();
    for (e, mut c, ship, tf, health) in &mut ships {
        c.idle += dt;
        c.elapsed += dt;
        if health.destroyed || splashed.contains(&e) {
            c.stop();
        }
        match c.state {
            State::Cloaking if c.elapsed >= FADE => (c.state, c.elapsed) = (State::Full, 0.0),
            State::Decloaking if c.elapsed >= c.length => (c.state, c.elapsed, c.idle, c.knows) = (State::Off, 0.0, 0.0, vec![]),
            _ => {}
        }
        if c.state == State::Off {
            continue;
        }
        if !c.knows.contains(&ship.side) {
            c.knows.push(ship.side);
        }
        if c.state != State::Full {
            continue;
        }
        // A watcher of another team within reach spots it with the chance its lookouts' factor exceeds 1, once a second
        // (PROVISIONAL cadence: the factor is a per-tick chance in the original, its tick rate unmeasured).
        let reach = lib.classes[ship.class].sight * DETECT_REACH;
        let from = tf.translation;
        for (w, wtf, wh, scan) in &watchers {
            let chance = scan.map_or(0.0, |f| f.0 - 1.0).max(0.0) * dt;
            if wh.destroyed || w.side == ship.side || c.knows.contains(&w.side) || wtf.translation.distance(from) > reach || rng.f() >= chance {
                continue;
            }
            c.knows.push(w.side);
        }
    }
}

/// Computer submarines: decloak when a hostile is close, cloak when one is in contact and a gun can fire cloaked, decloak when none is.
/// Ships the player commands, and ships whose rules take the AI's say away, are left alone. PROVISIONAL rule (decoded from a read of
/// the original's routine, not observed).
fn ai(
    time: Res<Time>,
    lib: Res<Library>,
    fleets: Res<Fleets>,
    mut ships: Query<(Entity, &mut Cloak, &Ship, &Transform, &Health, Has<PlayerShip>)>,
    others: Query<(&Ship, &Transform, &Health, Option<&ScanFactor>)>,
) {
    for (e, mut c, ship, tf, health, primary) in &mut ships {
        c.ai_in -= time.delta_secs();
        if c.ai_in > 0.0 || !c.ai_modify || primary || health.destroyed || fleets.roster().contains(&e) {
            continue;
        }
        c.ai_in = AI_PERIOD;
        let class = &lib.classes[ship.class];
        let sight = class.sight;
        let nearest = others
            .iter()
            .filter(|(o, otf, oh, _)| o.side != ship.side && !oh.destroyed && otf.translation.distance(tf.translation) < sight)
            .map(|(_, otf, ..)| otf.translation.distance(tf.translation))
            .fold(f32::MAX, f32::min);
        let reach = class.mounts.iter().filter_map(|m| lib.guns.get(&m.gun)).map(|g| g.range[2]).fold(0.0, f32::max);
        let armed = class.mounts.iter().any(|m| lib.guns.get(&m.gun).is_some_and(|g| g.cloak_fire));
        if c.stop_shown() && nearest < reach * AI_REVEAL || nearest == f32::MAX {
            c.stop();
        } else if armed && nearest != f32::MAX && nearest >= reach * AI_REVEAL {
            c.start(ship.side);
        }
    }
}

/// Speed and thrust fall with the level; the nets' factor is where the ship's speed multipliers meet, so the cloak's rides there.
fn slow(mut ships: Query<(&Cloak, &mut Ship), Without<crate::combat::Netted>>) {
    for (c, mut ship) in &mut ships {
        ship.net = c.speed_factor();
    }
}

/// --script cloak (with `--ship Ship_Procyon_Submersible`): the command ship starts a cloak at frame 10 and stops it at frame 600, so
/// a screenshot mid-fade (`--shot-frame 150`) and a late one (`--late-frame 400`, fully cloaked) show the hull alpha, the bubble and the
/// periscope trail.
fn cloak_smoke(world: &mut World) {
    let cfg = world.resource::<crate::Config>();
    if cfg.script.as_deref() != Some("cloak") || ![10, 600].contains(&cfg.frame) {
        return;
    }
    let frame = cfg.frame;
    let Ok(me) = world.query_filtered::<Entity, With<PlayerShip>>().single(world) else { return };
    let side = world.get::<Ship>(me).map_or(Side::Player, |s| s.side);
    let mut keeper = world.entity_mut(me).take::<Cloak>().unwrap_or_default();
    keeper.idle = IDLE_GATE;
    if frame == 10 {
        assert!(keeper.start(side), "cloak smoke: the start was refused");
    } else {
        assert!(keeper.stop(), "cloak smoke: the stop was refused");
    }
    world.entity_mut(me).insert(keeper);
}

#[derive(Component)]
struct Faded {
    /// Mesh entity and the material it came with.
    originals: HashMap<Entity, Handle<StandardMaterial>>,
    clones: HashMap<Entity, Handle<StandardMaterial>>,
}

/// Sounds and effects at each state change, the periscope trail while fully cloaked, and `Cloaked_Impact` on hits.
fn present(
    mut commands: Commands,
    assets: Res<AssetServer>,
    lib: Res<Library>,
    fxl: Option<Res<crate::fx::FxLib>>,
    mut hits: MessageReader<Hit>,
    mut ships: Query<(Entity, &mut Cloak, &Ship, &Transform)>,
    mut sfx: MessageWriter<Sfx>,
) {
    let struck: Vec<Entity> = hits.read().map(|h| h.victim).collect();
    for (e, mut c, ship, tf) in &mut ships {
        let points = lib.classes[ship.class].cloak_points;
        let at = |i: usize| points.map_or(Transform::default(), |p| Transform { translation: p[i].0, rotation: p[i].1, ..default() });
        let (was, now) = (c.seen_state, c.state);
        if was != now {
            c.seen_state = now;
            let (sound, effect, point) = match now {
                State::Cloaking => (Some("IC_Sub_Cloak"), Some("Cloaking"), 0),
                State::Decloaking => (Some("IC_Sub_UnCloak"), Some("DeCloaking"), 0),
                _ => (None, None, 0),
            };
            if let Some(s) = sound {
                sfx.write(Sfx { name: s.into(), at: Some(tf.translation), vol: 1.0, max: 600.0 });
            }
            if let (Some(effect), Some(fxl)) = (effect, fxl.as_deref()) {
                let local = at(point);
                crate::fx::spawn(&mut commands, fxl, effect, Some(e), local, *tf * local, true);
            }
            if let Some(f) = c.shown.take() {
                commands.entity(f).try_despawn(); // the periscope trail ends when the decloak starts
            }
            if let Some(s) = c.sound.take() {
                commands.entity(s).try_despawn();
            }
            if now == State::Full {
                if let Some(fxl) = fxl.as_deref() {
                    let local = at(1);
                    c.shown = crate::fx::spawn(&mut commands, fxl, "FullyCloaked", Some(e), local, *tf * local, false);
                }
                c.sound = Some(
                    commands
                        .spawn((
                            ChildOf(e),
                            AudioPlayer::new(assets.load(crate::audio::sound("IC_Sub_CloakingLoop"))),
                            PlaybackSettings::LOOP.with_volume(bevy::audio::Volume::Linear(0.3 * crate::menu::volume(crate::menu::Setting::Effects))),
                        ))
                        .id(),
                );
            }
        }
        if now == State::Full
            && struck.contains(&e)
            && let Some(fxl) = fxl.as_deref()
        {
            let local = at(0);
            crate::fx::spawn(&mut commands, fxl, "Cloaked_Impact", Some(e), local, *tf * local, true);
        }
    }
}

/// The hull's opacity for the local player's team: every material of the ship's scene is cloned and faded while the level is above
/// zero, and put back when it is not. A ship the player's team does not know disappears at full cloak.
fn fade(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut ships: Query<(Entity, &Cloak, &mut Visibility, Option<&mut Faded>, Option<&crate::campaign::MissionShip>)>,
    children: Query<&Children>,
    mut meshes: Query<&mut MeshMaterial3d<StandardMaterial>>,
) {
    for (e, c, mut vis, faded, ms) in &mut ships {
        let alpha = c.alpha_for(Side::Player);
        let script_visible = ms.is_none_or(|m| m.visible);
        vis.set_if_neq(if !script_visible || c.hidden_from(Side::Player) { Visibility::Hidden } else { Visibility::Inherited });
        if alpha >= 1.0 {
            if let Some(f) = faded {
                for (m, original) in &f.originals {
                    if let Ok(mut mat) = meshes.get_mut(*m) {
                        mat.0 = original.clone();
                    }
                }
                commands.entity(e).remove::<Faded>();
            }
            continue;
        }
        let mut f = faded;
        let mut fresh = Faded { originals: HashMap::new(), clones: HashMap::new() };
        let state = f.as_deref_mut().unwrap_or(&mut fresh);
        for d in children.iter_descendants(e) {
            let Ok(mut mat) = meshes.get_mut(d) else { continue };
            if !state.clones.contains_key(&d) {
                let Some(copy) = materials.get(&mat.0).cloned() else { continue };
                state.originals.insert(d, mat.0.clone());
                state.clones.insert(d, materials.add(copy));
            }
            let clone = state.clones[&d].clone();
            if mat.0 != clone {
                mat.0 = clone.clone();
            }
            if let Some(mut m) = materials.get_mut(&clone) {
                m.alpha_mode = AlphaMode::Blend;
                m.base_color.set_alpha(alpha);
            }
        }
        if f.is_none() {
            commands.entity(e).insert(fresh);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cloak_takes_five_seconds_each_way_and_waits_out_the_idle_gate() {
        let mut c = Cloak::default();
        assert!(!c.start(Side::Enemy), "not before the ship is five seconds old");
        c.idle = IDLE_GATE;
        assert!(c.start(Side::Enemy) && c.state == State::Cloaking);
        c.elapsed = 2.5;
        assert_eq!(c.level(), 0.5);
        // A stop halfway decloaks over the time already spent, without a jump.
        assert!(c.stop());
        assert_eq!((c.state, c.level()), (State::Decloaking, 0.5));
        c.elapsed = 1.25;
        assert_eq!(c.level(), 0.25);
        assert!(!c.can_start(), "start is refused while decloaking");
    }

    #[test]
    fn viewers_see_the_hull_at_the_inverse_level_or_a_third_when_their_team_knows_it() {
        let mut c = Cloak::new(true);
        c.knows = vec![Side::Player];
        assert_eq!(c.alpha_for(Side::Player), KNOWN_FLOOR);
        assert_eq!(c.alpha_for(Side::Enemy), 0.0);
        assert!(c.hidden_from(Side::Enemy) && !c.hidden_from(Side::Player));
        c.state = State::Cloaking;
        c.elapsed = 2.5;
        assert_eq!(c.alpha_for(Side::Enemy), 0.5);
        assert!(!c.hidden_from(Side::Enemy), "only a fully cloaked ship drops out of every list");
        assert!(c.holds_fire() && (c.speed_factor() - 0.7).abs() < 1e-6);
    }

    #[test]
    fn stop_is_only_accepted_while_cloaking_or_fully_cloaked() {
        let mut c = Cloak::default();
        assert!(!c.stop() && !c.stop_shown());
        c.state = State::Full;
        assert!(c.stop() && c.state == State::Decloaking && c.length == FADE);
        assert!(!c.stop());
    }
}
