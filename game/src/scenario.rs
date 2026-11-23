//! Battle lifecycle: spawning ships/scenery from a map or mission, restart, leave, outcome.
use crate::*;

mod lighting;
mod outcome;
mod spawn;

pub use lighting::{Lighting, MapLighting, apply_lighting, lighting_of};
#[cfg(test)]
use outcome::end_line_id;
pub use outcome::{END_LINE_SECS, EndLine, EndMessage, Outcome, RULE_PASS_SECS, RulePass, outcome};
pub use spawn::{HOSTILE_TO_ALL, MapObject, Namer, PlayerColor, faction, game_rotation, game_to_bevy, scenery_transform, spawn_scenario, spawn_ship};
#[cfg(test)]
use spawn::{ambient_effect, setup_strengths};

#[derive(Component)]
pub struct Scenario;

/// Creation order within one kind of object (ships among ships, shells among shells), counted from 0 at every battle start. The
/// original updates objects in creation order (docs/re/tick.md); systems whose random draws or tie-breaks depend on order go by this
/// (`in_spawn_order`), not by query or `Entity` order, which change with despawns and between a battle and its restart.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SpawnIndex(pub u32);

impl SpawnIndex {
    /// `e`'s number, 0 when it has none (unit-test worlds).
    pub fn of(order: &Query<&SpawnIndex>, e: Entity) -> SpawnIndex {
        order.get(e).copied().unwrap_or(SpawnIndex(0))
    }
    /// A fixed draw in [0, 1) for this object (`salt` picks one of several), the same in every run of the battle.
    pub fn draw(self, salt: u32) -> f32 {
        let h = (u64::from(self.0) << 32 | u64::from(salt)).wrapping_add(0x9E37_79B9_7F4A_7C15).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        ((h ^ (h >> 31)) >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// The next `SpawnIndex` of each numbered kind; cleared at battle start.
#[derive(Resource, Default)]
pub struct SpawnCounters(std::collections::HashMap<std::any::TypeId, u32>);

/// Numbers every new `T` in the order it is added (command order, so the order the systems spawned them). One kind per entity.
pub fn number_spawns<T: Component>(app: &mut App) {
    app.init_resource::<SpawnCounters>().add_observer(|add: On<Add<T>>, mut counters: ResMut<SpawnCounters>, mut commands: Commands| {
        let n = counters.0.entry(std::any::TypeId::of::<T>()).or_default();
        commands.entity(add.entity).try_insert(SpawnIndex(*n));
        *n += 1;
    });
}

/// Counts battle starts (the first and every restart). A system that keeps per-entity state in a `Local` asks `BattleStart` and
/// drops it when a new battle began, so nothing of the last battle survives a restart.
#[derive(Resource, Default)]
pub struct BattleEpoch(u32);

/// `is_new()` is true once for each battle start, on the first run of the system after it.
#[derive(bevy::ecs::system::SystemParam)]
pub struct BattleStart<'w, 's> {
    epoch: Option<Res<'w, BattleEpoch>>,
    seen: Local<'s, u32>,
}

impl BattleStart<'_, '_> {
    pub fn is_new(&mut self) -> bool {
        let now = self.epoch.as_ref().map_or(0, |e| e.0);
        std::mem::replace(&mut *self.seen, now) != now
    }
}

/// `items` in spawn order (`entity` names each item's entity; `order` looks up its number). Unnumbered items (unit-test worlds) come
/// first, in their given order, as before.
pub fn in_spawn_order<T>(order: &Query<&SpawnIndex>, items: impl IntoIterator<Item = T>, entity: impl Fn(&T) -> Entity) -> Vec<T> {
    let mut v: Vec<_> = items.into_iter().map(|t| (order.get(entity(&t)).ok().copied(), t)).collect();
    v.sort_by_key(|(i, _)| *i);
    v.into_iter().map(|(_, t)| t).collect()
}

/// The difficulty option (0 Easy, 1 Normal, 2 Hard) as it stood when the battle started: the simulation reads this, not the live
/// option. PROVISIONAL: whether the original applies a difficulty change made during a battle is not observed.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Difficulty(pub u8);

impl Default for Difficulty {
    fn default() -> Self {
        Self(1)
    }
}

/// Battle start (the first one and every restart): reseed the simulation's random numbers, restart spawn numbering, take the
/// difficulty. The seed is logged so a run can be repeated with `--seed`.
pub fn reset_sim(world: &mut World) {
    let seed = combat::SimRng::configured_seed();
    let difficulty = Difficulty(menu::difficulty());
    info!("battle seed {seed} (repeat with --seed {seed}), difficulty {}", difficulty.0);
    world.insert_resource(combat::SimRng::new(seed));
    world.insert_resource(SpawnCounters::default());
    world.insert_resource(difficulty);
    world.insert_resource(fx::DetailScale(menu::detail_scale()));
    world.get_resource_or_insert_with(BattleEpoch::default).0 += 1;
}

/// A fleet type, as the TWT numbers it: 0 Navy, 2 Pirate, 3 Procyon; 4 (and any other number) is no fleet type chosen or known.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Race {
    Navy,
    Pirate,
    Procyon,
    #[default]
    Unchosen,
}

impl Race {
    /// The fleet types a lobby slot can pick, in the lobby's order.
    pub const PLAYABLE: [Race; 3] = [Race::Navy, Race::Pirate, Race::Procyon];

    /// The race of a TWT `race` number (the map JSON keeps the number).
    pub fn from_twt(n: u64) -> Self {
        match n {
            0 => Race::Navy,
            2 => Race::Pirate,
            3 => Race::Procyon,
            _ => Race::Unchosen,
        }
    }

    /// The TWT number (`from_twt(r.twt())` is `r`); 4 for no fleet type.
    pub fn twt(self) -> u8 {
        match self {
            Race::Navy => 0,
            Race::Pirate => 2,
            Race::Procyon => 3,
            Race::Unchosen => 4,
        }
    }

    /// The race of a ship class (`Class::race`: "Navy", "Pirate", "Procyon"); None for any other.
    pub fn from_class(name: &str) -> Option<Self> {
        Self::PLAYABLE.into_iter().find(|r| r.class_name() == Some(name))
    }

    /// The `race` the ship classes of this fleet type carry; None for no fleet type.
    pub fn class_name(self) -> Option<&'static str> {
        match self {
            Race::Navy => Some("Navy"),
            Race::Pirate => Some("Pirate"),
            Race::Procyon => Some("Procyon"),
            Race::Unchosen => None,
        }
    }

    /// The string-table id of the fleet type's name in the lobby's pull-down; None for no fleet type.
    pub fn fleet_text(self) -> Option<&'static str> {
        match self {
            Race::Navy => Some("IDGS_TPFRONTENDTEXT03_FLEET_NAVY"),
            Race::Pirate => Some("IDGS_TPFRONTENDTEXT03_FLEET_PIRATE"),
            Race::Procyon => Some("IDGS_TPFRONTENDTEXT03_FLEET_PROCYON"),
            Race::Unchosen => None,
        }
    }

    /// First flag icon of the fleet type's block of ten (`FrontEnd_Flag_*Icon_NN`): Navy 0, Pirate 10, Procyon 20 (Navy's for none).
    pub fn flag_base(self) -> usize {
        match self {
            Race::Pirate => 10,
            Race::Procyon => 20,
            Race::Navy | Race::Unchosen => 0,
        }
    }
}

/// One fleet of a battle: whose start point it uses (index in the map's player list), which side it fights for, whether the
/// human commands it, and its ships with an explicit start position (historical maps) or none (formation at the start point).
pub struct Group {
    pub player: usize,
    pub side: Side,
    pub human: bool,
    /// Fleet type: which base the fleet defends.
    pub race: Race,
    /// Index in the map's team list (or the lobby team on free-for-all maps), -1: on its own.
    pub team: i32,
    /// The lobby slot behind this fleet (the in-battle player list shows one row per slot): the human's name (none: "Computer") and the
    /// fleet type's flag icon number. None for fleets that are no lobby slot.
    pub row: Option<(Option<String>, usize)>,
    /// The map's world-object id of each ship that the map placed (historical maps), else none.
    pub ids: Vec<Option<i64>>,
    pub ships: Vec<(String, Option<[f32; 3]>, Option<String>)>, // class, start position, scripted name
    pub weapons: Vec<std::collections::BTreeMap<usize, String>>,
    /// Named crew per ship and post (skirmish fleets assembled in the front end).
    pub crew: Vec<[Vec<String>; 8]>,
}

/// Battle start: the front end's choice (map, fleets) if any, else the command line.
pub fn enter_battle(
    mut commands: Commands,
    assets: Res<AssetServer>,
    lib: Res<Library>,
    fxl: Res<fx::FxLib>,
    shapes: Res<collision::Shapes>,
    asteroids: Res<data::AsteroidDefs>,
    mut cfg: ResMut<Config>,
    sk: Res<menu::Skirmish>,
) {
    if sk.map.is_some() {
        cfg.map = sk.map.clone();
        cfg.groups = sk.plan(&lib);
        cfg.ended = None;
    }
    spawn_scenario(&mut commands, &assets, &lib, &fxl, &shapes, &asteroids, &cfg);
}

/// F10: back to the main menu (the retail in-game menu is not done).
pub fn leave_battle(keys: Res<ButtonInput<KeyCode>>, mut commands: Commands, old: Query<Entity, Or<(With<Scenario>, With<Transient>)>>, mut next: ResMut<NextState<menu::Screen>>) {
    if !keys.just_pressed(KeyCode::F10) {
        return;
    }
    for e in &old {
        commands.entity(e).despawn();
    }
    commands.remove_resource::<campaign::Mission>();
    commands.remove_resource::<crate::rules::skirmish::SkirmishScript>();
    commands.insert_resource(ship::Fleets::default());
    commands.queue(|world: &mut World| {
        world.resource_mut::<Orbit>().focus = None;
    });
    next.set(menu::Screen::Main);
}

/// A fresh scenario must not inherit pause from the battle it replaces; keep the chosen game speed.
pub fn reset_fleets(world: &mut World) {
    world.remove_resource::<campaign::Mission>();
    world.insert_resource(ship::Fleets::default());
    world.insert_resource(crate::environment::Sensors::default());
    if let Some(mut orbit) = world.get_resource_mut::<Orbit>() {
        orbit.focus = None;
    }
    world.insert_resource(Target::default());
    world.insert_resource(panels::ChatLog::default());
    if let Some(mut hud) = world.get_resource_mut::<panels::FleetHud>() {
        *hud = default();
    }
    if let Some(menu) = world.get_resource::<options_menu::OptionsMenu>() {
        menu.reset();
    }
    if let Some(mut request) = world.get_resource_mut::<options_menu::BarMenuRequest>() {
        request.0 = None;
    }
}
pub fn resume_scenario(world: &mut World) {
    reset_fleets(world);
    world.resource_mut::<Time<Virtual>>().unpause();
}

/// Dev cheats: K destroys all enemies, J destroys the player. `--script victory|defeat` presses one at frame 300 (demo).
/// Scripted runs (`--script target`): pick the first enemy so the target reticle can be screenshotted (`fxtest`: so it fires).
pub fn shot_target(
    cfg: Res<Config>,
    mut target: ResMut<Target>,
    mut orbit: ResMut<Orbit>,
    mut ships: Query<(Entity, &mut Ship)>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    time: Res<Time<Virtual>>,
    loading: Option<Res<crate::loading::Loading>>,
) {
    // The frame count stands still while a battle loads: a step must not repeat on every loading frame (a press of R would restart again).
    if loading.is_some() {
        return;
    }
    // Runtime self-check: Pause key freezes simulation time, then resumes it while the HUD stays usable.
    if matches!(cfg.script.as_deref(), Some("pause" | "pause-restart")) {
        let resume_key = if cfg.script.as_deref() == Some("pause-restart") { KeyCode::KeyR } else { KeyCode::Pause };
        match cfg.frame {
            30 => {
                keys.press(KeyCode::Pause);
            }
            31 => {
                keys.release(KeyCode::Pause);
            }
            60 => {
                keys.press(resume_key);
            }
            61 => {
                keys.release(resume_key);
            }
            40 => {
                assert!(time.is_paused(), "Pause must pause the simulation");
                assert_eq!(time.delta_secs(), 0.0, "paused simulation time must not advance");
            }
            70 => assert!(!time.is_paused(), "resume/restart must unpause the simulation"),
            _ => {}
        }
    }
    if cfg.script.as_deref() == Some("speedtest") {
        for (_, mut s) in &mut ships {
            if s.side == Side::Player {
                s.order = EngineOrder::from_index(((cfg.frame / 90) % 4) as usize).unwrap_or_default();
            }
        }
    }
    if cfg.script.as_deref() == Some("course") && cfg.frame == 3 {
        for (_, mut s) in &mut ships {
            if s.side == Side::Player {
                s.waypoints = vec![Vec3::new(-90.0, 0.0, -80.0), Vec3::new(40.0, 0.0, -200.0)];
                s.order = EngineOrder::Full;
                s.goal = None;
            }
        }
    }
    // fxtest clicks the enemy too: the command ship only fires at a clicked target.
    if matches!(cfg.script.as_deref(), Some("target") | Some("fxtest")) && cfg.frame >= 5 && target.0.is_none() {
        target.0 = ships.iter().find(|(_, s)| s.side == Side::Enemy).map(|(e, _)| e);
        if cfg.script.as_deref() == Some("target") {
            orbit.focus = target.0; // screenshots look at the target
        }
    }
}

pub fn debug_kill(keys: Res<ButtonInput<KeyCode>>, cfg: Res<Config>, mut ships: Query<(&Ship, &mut Health), Without<lifeboats::Lifeboat>>) {
    // Cheat keys only in debug builds or scripted runs, so a stray H/J/K never ends a real game.
    let keys_on = cfg!(debug_assertions) || cfg.script.is_some();
    let pressed = |k| keys_on && keys.just_pressed(k);
    let hurt = pressed(KeyCode::KeyH) || (cfg.shot.is_some() && cfg.script.as_deref() == Some("damage") && cfg.frame == 30);
    let s = cfg.script.as_deref();
    let at = cfg.shot.is_some() && cfg.frame == 300;
    let (kill_enemies, kill_player) = (pressed(KeyCode::KeyK) || (at && matches!(s, Some("victory") | Some("restart"))), pressed(KeyCode::KeyJ) || (at && s == Some("defeat")));
    for (ship, mut h) in &mut ships {
        if hurt && ship.side == Side::Player {
            for hp in h.sections.iter_mut() {
                *hp *= 0.35;
            }
            h.sections[1] = 0.0; // stern destroyed: shows the Damage_ swap mesh
        }
        if (ship.side != Side::Player && kill_enemies) || (ship.side == Side::Player && kill_player) {
            h.core = 0.0; // a dev kill stands in for a combat death: combat::deaths reports it and breaks the hull up
        }
    }
}

/// Set by the developer console's `restart`: the same as pressing R.
#[derive(Resource, Default)]
pub struct RestartRequest(pub bool);

/// The respawned scenario is a fresh battle for the console: no cheats used, `god` off (the new script is vulnerable again).
/// The developer view survives a restart of the same mission (`show` overlays are rebuilt) but not a different mission.
fn reset_console_state(cheats: &mut crate::rules::console::Cheats, dev: &mut crate::rules::console::DevView, new_mission: bool) {
    *cheats = Default::default();
    if new_mission {
        dev.reset();
    }
}

pub fn restart(
    mut request: Option<ResMut<RestartRequest>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    assets: Res<AssetServer>,
    lib: Res<Library>,
    fxl: Res<fx::FxLib>,
    mut cfg: ResMut<Config>,
    mut cheats: ResMut<crate::rules::console::Cheats>,
    mut dev: ResMut<crate::rules::console::DevView>,
    mission: Option<Res<campaign::Mission>>,
    shapes: Res<collision::Shapes>,
    asteroids: Res<data::AsteroidDefs>,
    old: Query<Entity, Or<(With<Scenario>, With<Transient>)>>,
    loading: Option<Res<crate::loading::Loading>>,
) {
    // Not while the restarted battle loads: the frame count stands still then, and frame 400 would restart it again.
    let scripted = cfg.script.as_deref() == Some("restart") && cfg.frame == 400 && loading.is_none();
    // N after a won mission continues the campaign.
    let next = mission.filter(|m| keys.just_pressed(KeyCode::KeyN) && m.script.ended.as_ref().is_some_and(|e| e.1)).and_then(|m| campaign::next(&m.id));
    let asked = request.as_mut().is_some_and(|r| std::mem::take(&mut r.0));
    if !keys.just_pressed(KeyCode::KeyR) && !scripted && !asked && next.is_none() {
        return;
    }
    let new_mission = next.is_some();
    if new_mission {
        cfg.map = next;
    }
    cfg.ended = None;
    reset_console_state(&mut cheats, &mut dev, new_mission);
    for e in &old {
        commands.entity(e).despawn();
    }
    spawn_scenario(&mut commands, &assets, &lib, &fxl, &shapes, &asteroids, &cfg);
}

#[cfg(test)]
mod tests;
