//! Not retail: a skirmish map's own script (`maps/<id>/script.json`), the same interpreter as the campaign, driving the map's convoys, timers and
//! team points. Its effects are applied by the code the campaign uses (`systems::apply`). Retail maps keep their end rules with
//! victory.rs (`without_end_rules`); a map that opts in with `"victory": "script"` lets its own rules end the game (`script_end`).
use bevy::prelude::*;
use serde_json::Value;
use std::collections::HashMap;

use super::eval::schema;
use super::systems::{RuleShip, apply};
use super::*;
use crate::combat::Destroyed;
use crate::{Ship, Side};
#[derive(Resource)]
pub struct SkirmishScript {
    pub script: Script,
    /// The side each map player fights for (an owner change moves a ship to the new owner's side).
    pub sides: HashMap<String, Side>,
    paths: Value,
    /// The script's environment block (space animal and dragon definitions).
    pub environment: Value,
    /// The script's map text (place names the star map labels).
    pub map_text: Value,
}

impl SkirmishScript {
    /// The map's script, if it has one. `teams`: (team name id, its players); `sides`: the side of each map player.
    pub fn load(map: &str, teams: Vec<(String, Vec<String>)>, sides: HashMap<String, Side>) -> Option<Self> {
        let dir = crate::assets_dir().join(format!("maps/{map}"));
        let data = crate::data::try_json(&dir.join("script.json"))?;
        // Optional speech lines, end messages and ship names, as a mission's `media.json`.
        let media = crate::data::try_json(&dir.join("media.json")).unwrap_or(Value::Null);
        let decides = script_decides(&data);
        warn_campaign_only(map, &data);
        // A retail map's end rules stay with victory.rs; a map that opts in lets its own end rules (and team objectives) decide.
        let mut script = Script::new(&if decides { data.clone() } else { without_end_rules(&data) }, media);
        script.set_teams(teams);
        script.decide_game(decides);
        Some(SkirmishScript { script, sides, paths: data["paths"].clone(), environment: data["environment"].clone(), map_text: data["map_text"].clone() })
    }
    /// Whether the map's `script.json` decides how the game ends (`"victory": "script"`; `"objectives"` too, the name the campaign
    /// gives its task-driven endings).
    pub fn decides(map: &str) -> bool {
        crate::data::try_json(&crate::assets_dir().join(format!("maps/{map}/script.json"))).is_some_and(|d| script_decides(&d))
    }
    /// The rules' names of the map-placed ships ('Setup Ship' names) by world-object id; none when the map has no script.
    pub fn ship_names(map: &str) -> Option<HashMap<i64, String>> {
        let data = crate::data::try_json(&crate::assets_dir().join(format!("maps/{map}/script.json")))?;
        Some(data["ships"].as_array()?.iter().filter_map(|s| Some((s["id"].as_i64()?, s["name"].as_str()?.to_string()))).collect())
    }
    /// Team points so far (victory.rs 'Team has X points').
    pub fn points(&self) -> Vec<(String, i32)> {
        self.script.team_points.iter().map(|(t, p)| (t.clone(), *p)).collect()
    }
}

pub fn plugin(app: &mut App) {
    app.add_systems(Update, run.in_set(crate::FrameRules).run_if(in_state(crate::menu::Screen::Battle)).run_if(resource_exists::<SkirmishScript>));
}

/// The ECS access the shared effect application (`systems::apply`) needs besides the ships and animals.
#[derive(bevy::ecs::system::SystemParam)]
struct SkirmishFx<'w, 's> {
    bases: apply::Bases<'w, 's>,
    mines: apply::Mines<'w, 's>,
    dragons: Query<'w, 's, &'static mut crate::dragons::Dragon>,
    difficulty: Option<Res<'w, crate::scenario::Difficulty>>,
    dock: MessageWriter<'w, crate::docking::DockMsg>,
    board: MessageWriter<'w, crate::boarding::BoardMsg>,
    asteroids: MessageWriter<'w, crate::combat::AsteroidSetup>,
    meteors: MessageWriter<'w, crate::nebula::MeteorRoute>,
    detonate: MessageWriter<'w, crate::combat::Detonate>,
    event_fx: Query<'w, 's, (Entity, &'static super::systems::EventFx)>,
    fxlib: Option<Res<'w, crate::fx::FxLib>>,
    sfx: Option<ResMut<'w, Messages<crate::audio::Sfx>>>,
    toggles: Option<ResMut<'w, crate::panels::Toggles>>,
}

#[allow(clippy::too_many_arguments)]
fn run(
    mut sk: ResMut<SkirmishScript>,
    time: Res<Time>,
    lib: Res<crate::data::Library>,
    mut killed: MessageReader<Destroyed>,
    mut captured: MessageReader<crate::boarding::Captured>,
    mut ships: Query<RuleShip>,
    mut animals: Query<(Entity, &mut crate::environment::SpaceAnimal, &mut Transform, &mut Visibility), Without<Ship>>,
    rocks: Query<(Entity, &Transform, &crate::combat::Asteroid), (Without<Ship>, Without<crate::environment::SpaceAnimal>)>,
    mut fx: SkirmishFx,
    assets: Res<AssetServer>,
    mut commands: Commands,
) {
    let sk = &mut *sk;
    let mut world: Vec<Unit> = ships
        .iter()
        .map(|s| Unit {
            name: s.mission.name.clone(),
            owner: s.mission.owner.clone(),
            alive: !s.health.destroyed,
            visible: s.mission.visible,
            primary: s.primary,
            flag: s.mission.flag.clone(),
            attack: s.mission.attack.clone(),
            ..Unit::new(s.entity, s.mission.id, super::systems::sim_pos(s.transform, s.interp))
        })
        .collect();
    sk.script.keep_sunk(&mut world);
    world.extend(animals.iter().map(|(e, a, tf, _)| a.unit(e, tf)));
    // Moving belts: the rocks are units too, tagged with the script's groups by their world-object id.
    world.extend(rocks.iter().map(|(e, tf, a)| a.unit(e, tf)));
    world.extend(fx.mines.iter().map(|(e, tf, m, _)| m.unit(e, tf)));
    sk.script.tag(&mut world);
    for d in killed.read() {
        if let Some(by) = world.iter().find(|u| u.e == d.by) {
            sk.script.record_destroyed(&by.owner.clone(), d.victim);
        }
    }
    for c in captured.read() {
        sk.script.record_capture(&c.by, &c.victim, c.unit, false);
    }
    let (fxs, voice) = sk.script.step(&world, time.delta_secs(), false);
    if let Some(file) = voice {
        commands.spawn((
            crate::Scenario,
            crate::briefing::Voice,
            AudioPlayer::new(assets.load(crate::audio::sound(&file))),
            PlaybackSettings::DESPAWN.with_volume(bevy::audio::Volume::Linear(crate::menu::volume(crate::menu::Setting::Dialogue))),
        ));
    }
    let mut env = apply::FxEnv {
        script: &mut sk.script,
        paths: &sk.paths,
        lib: &lib,
        bases: &mut fx.bases,
        dragons: &mut fx.dragons,
        dock: &mut fx.dock,
        board: &mut fx.board,
        asteroids: &mut fx.asteroids,
        meteors: &mut fx.meteors,
        detonate: &mut fx.detonate,
        event_fx: &fx.event_fx,
        fxlib: fx.fxlib.as_deref(),
        sfx: fx.sfx.as_deref_mut(),
        toggles: fx.toggles.as_deref_mut(),
        difficulty: fx.difficulty.as_deref().copied().unwrap_or_default().0,
        orbit: None,
        sides: Some(&sk.sides),
        mines: Some(&mut fx.mines),
    };
    apply::apply(fxs, &mut env, &mut commands, &mut ships, &mut animals);
}

/// `"victory": "script"` (or `"objectives"`) in a map's `script.json`: the script's own end rules decide the game.
fn script_decides(data: &Value) -> bool {
    matches!(data["victory"].as_str(), Some("script" | "objectives"))
}

/// A skirmish has no cutscene camera, mission HUD or player-ship handoff: a rule that uses a campaign-only Type (the rule schema marks
/// them, `--check-mission` reports them as errors) changes nothing there. Say so once per rule and Type when the map loads.
fn warn_campaign_only(map: &str, data: &Value) {
    let mut seen = std::collections::HashSet::new();
    for r in data["rules"].as_array().into_iter().flatten() {
        for (role, key) in [(schema::Role::Condition, "conditions"), (schema::Role::Action, "actions")] {
            for e in r[key].as_array().into_iter().flatten() {
                let (rule, ty) = (r["name"].as_str().unwrap_or("").trim(), e["Type"].as_str().unwrap_or(""));
                if schema::lookup(role, ty).is_some_and(|d| d.campaign_only) && seen.insert((rule.to_string(), ty.to_string())) {
                    warn!("map '{map}': rule '{rule}' uses '{ty}', which only campaign missions support; it does nothing in a skirmish");
                }
            }
        }
    }
}
