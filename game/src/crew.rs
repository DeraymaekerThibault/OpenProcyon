//! Crew voice callouts (docs/re/sound.md "Crew voice lines"). `Tracker` turns snapshots of the player's situation into event names
//! (`content/voice/events.json`); `callouts` queues them and plays one line at a time. PROVISIONAL: the event grouping is the community remake's,
//! the thresholds, 15 s per-event cooldown and the fixed Navy human-male crew are guesses; spotting uses shared sensors. Hazards
//! (nebula, storm, black hole, asteroids, dragons, whales, currents, meteors), weapon reports (torpedoes, mortars), fires, the
//! collision alarm and out-of-bounds are reported on entering/starting; the rules' 'Crew Speech - Toggle On Off' mutes them all.

use std::collections::{HashMap, VecDeque};

use bevy::prelude::*;
use serde_json::Value;

use crate::combat::{Health, Library};
use crate::menu::Screen;
use crate::scenario::Outcome;
use crate::{PlayerShip, Ship, Side};

const COOLDOWN: f32 = 15.0;
const RACE: &str = "HumanMale";

/// What `Tracker` watches, gathered from the ECS each frame.
#[derive(Default, Clone)]
pub struct Snapshot {
    /// Hull (core) HP remaining, 0..1.
    pub hull: f32,
    /// Remaining HP fraction of each sail section.
    pub sails: Vec<f32>,
    /// Enemy ships: (id, detected by the player's group, destroyed).
    pub enemies: Vec<(u64, bool, bool)>,
    pub allies_lost: usize,
    pub docked: bool,
    pub boarding: bool,
    pub outcome: Option<bool>,
    /// Faction of each newly spotted enemy, by id ("pirate", "navy", "procyon", "civilian"): picks the faction line when there is one.
    pub races: Vec<(u64, &'static str)>,
    /// Hazards near or around the ship this frame; each callout plays when it starts.
    pub hazards: Vec<&'static str>,
    /// One-shot reports this frame (weapon fired, collision, out of bounds...).
    pub reports: Vec<&'static str>,
    pub fires: usize,
    pub weapons_left: usize,
}

#[derive(Default)]
pub struct Tracker {
    prev: Option<Snapshot>,
    spotted: Vec<u64>,
    hull_level: usize,
    sails_hit: bool,
}

impl Tracker {
    pub fn observe(&mut self, s: Snapshot) -> Vec<&'static str> {
        let mut out = vec![];
        if let Some(p) = &self.prev {
            // Hull callouts at 75/50/25 % remaining, once each.
            let level = [0.75, 0.5, 0.25].iter().filter(|t| s.hull <= **t).count();
            if level > self.hull_level {
                self.hull_level = level;
                out.push("hull_damage");
            }
            if !self.sails_hit && s.sails.iter().any(|f| *f < 0.75) {
                self.sails_hit = true;
                out.push("sails_damaged");
            }
            let gone = |v: &[f32]| v.iter().filter(|f| **f <= 0.0).count();
            if !s.sails.is_empty() && gone(&s.sails) == s.sails.len() && gone(&p.sails) < p.sails.len() {
                out.push("all_sails_destroyed");
            } else if gone(&s.sails) > gone(&p.sails) {
                out.push("sails_destroyed");
            }
            for &(id, detected, dead) in &s.enemies {
                if dead && p.enemies.iter().any(|e| e.0 == id && e.1 && !e.2) {
                    out.push("enemy_ship_destroyed");
                } else if !dead && detected && !self.spotted.contains(&id) {
                    self.spotted.push(id);
                    out.push(match s.races.iter().find(|r| r.0 == id).map(|r| r.1) {
                        Some("pirate") => "enemy_ship_spotted_pirate",
                        Some("navy") => "enemy_ship_spotted_navy",
                        Some("procyon") => "enemy_ship_spotted_procyon",
                        Some("civilian") => "enemy_ship_spotted_civilian",
                        _ => "enemy_ship_spotted",
                    });
                }
            }
            for h in &s.hazards {
                if !p.hazards.contains(h) {
                    out.push(h);
                }
            }
            out.extend(s.reports.iter().copied());
            if s.fires > p.fires {
                out.push("put_out_fire");
            }
            if s.weapons_left == 0 && p.weapons_left > 0 {
                out.push("all_weapons_destroyed");
            }
            if s.allies_lost > p.allies_lost {
                out.push("ship_lost");
            }
            if s.docked != p.docked {
                out.push(if s.docked { "dock" } else { "undock" });
            }
            if s.boarding && !p.boarding {
                out.push("landing_party");
            }
            if s.outcome.is_some() && p.outcome.is_none() {
                out.push(if s.outcome == Some(true) { "mission_won" } else { "mission_failed" });
            }
        }
        self.prev = Some(s);
        out
    }
}

#[derive(Component)]
pub struct CrewVoice;

fn events() -> &'static Value {
    static EVENTS: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    EVENTS.get_or_init(|| crate::data::try_json(&crate::assets_dir().join("voice/events.json")).unwrap_or(Value::Null))
}

/// A voice file for `event`, rotating through its variants with `n`.
fn line(event: &str, n: usize) -> Option<String> {
    let files = events()[RACE][event].as_array().filter(|a| !a.is_empty())?;
    files[n % files.len()].as_str().map(str::to_string)
}

pub fn plugin(app: &mut App) {
    app.add_systems(PostUpdate, callouts.run_if(in_state(Screen::Battle)));
}

/// The world around the player that crew callouts react to (beyond the ships).
#[derive(bevy::ecs::system::SystemParam)]
struct Surroundings<'w, 's> {
    mission: Option<ResMut<'w, crate::campaign::Mission>>,
    sensors: Res<'w, crate::environment::Sensors>,
    holes: Query<'w, 's, &'static GlobalTransform, With<crate::blackhole::BlackHole>>,
    rocks: Query<'w, 's, (&'static Transform, &'static crate::combat::Asteroid), Without<PlayerShip>>,
    animals: Query<'w, 's, (Entity, &'static crate::environment::SpaceAnimal, Has<crate::dragons::Dragon>), Without<PlayerShip>>,
    currents: Query<'w, 's, &'static crate::environment::Current>,
    meteors: Query<'w, 's, &'static GlobalTransform, With<crate::nebula::MeteorEmitter>>,
    fired: MessageReader<'w, 's, crate::combat::Fired>,
    rams: MessageReader<'w, 's, crate::collision::Ram>,
    fires: Query<'w, 's, &'static crate::combat::Fires, With<PlayerShip>>,
    guns: Query<'w, 's, &'static crate::combat::GunStates, With<PlayerShip>>,
}

fn callouts(
    mut commands: Commands,
    assets: Res<AssetServer>,
    time: Res<Time>,
    lib: Res<Library>,
    outcome: Option<Res<Outcome>>,
    player: Option<Single<(Entity, &Transform, &Ship, &Health, Has<crate::docking::Docker>, Has<crate::boarding::Boarding>), With<PlayerShip>>>,
    ships: Query<(Entity, &Transform, &Ship, &Health), Without<PlayerShip>>,
    busy: Query<(), Or<(With<CrewVoice>, With<crate::panels::Helmsman>)>>,
    mut world: Surroundings,
    mut tracker: Local<Tracker>,
    mut queue: Local<VecDeque<&'static str>>,
    mut last: Local<HashMap<&'static str, f32>>,
    mut n: Local<usize>,
) {
    let Some(player) = player else { return };
    let (me, tf, ship, hp, docked, boarding) = *player;
    let here = tf.translation;
    let mut hazards = vec![];
    let mut reports = vec![];
    if let Some(m) = world.mission.as_deref_mut() {
        let inside = |n: &Value| crate::rules::in_game_volume(&m.data["volumes"][n["Polygon Name"].as_str().unwrap_or("")], here.x, -here.z);
        for n in m.data["environment"]["nebulae"].as_array().into_iter().flatten().filter(|n| inside(n)) {
            if n["Solar Storm Effect Name"] == "SolarStorm" {
                hazards.push("solar_storm");
            } else if n["Nebula Cloud Effect Name"] == "Nebula" {
                hazards.push("nebula");
            }
        }
        if std::mem::take(&mut m.script.helm_off_course) {
            reports.push("out_of_bounds");
        }
    }
    let near = |p: Vec3, r: f32| p.distance(here) < r;
    if world.holes.iter().any(|h| near(h.translation(), 600.0)) {
        hazards.push("black_hole");
    }
    if world.rocks.iter().any(|(t, a)| a.hp > 0.0 && near(t.translation, 250.0)) {
        hazards.push("asteroids");
    }
    for (e, a, dragon) in &world.animals {
        if a.visible && !a.dead && world.sensors.player_sees(e, world.mission.as_deref()) {
            hazards.push(if dragon { "dragons" } else { "whale_spotted" });
        }
    }
    if world.currents.iter().any(|c| c.force(here) != Vec3::ZERO) {
        hazards.push("ethereum_current");
    }
    if world.meteors.iter().any(|m| near(m.translation(), 300.0)) {
        hazards.push("meteor_shower");
    }
    for f in world.fired.read().filter(|f| f.shooter == me) {
        if f.bullet.starts_with("Bullet_Torpedo") {
            reports.push("torpedoes_away");
        } else if f.bullet.contains("Mortar") {
            reports.push("fire_mortar");
        }
    }
    if world.rams.read().any(|r| r.a == me || r.b == me) {
        reports.push("collision_alarm");
    }
    let fires = world.fires.single().map_or(0, |f| f.list.len());
    let weapons_left = world.guns.single().map_or(1, |_| {
        let class = &lib.classes[ship.class];
        class.mounts.iter().filter(|m| m.parent.is_none_or(|p| hp.sections[p] > 0.0)).count()
    });
    let crew_speech = world.mission.as_deref().is_none_or(|m| m.script.crew_speech);
    let class = &lib.classes[ship.class];
    let sails = class.sections.iter().zip(&hp.sections).filter(|(c, _)| c.material.contains("Sail")).map(|(c, h)| h / c.hp.max(1.0)).collect();
    let mut enemies = vec![];
    let mut allies_lost = 0;
    let mut races = vec![];
    for (e, _, s, h) in &ships {
        match s.side {
            Side::Enemy => {
                enemies.push((e.to_bits(), world.sensors.player_sees(e, world.mission.as_deref()), h.destroyed));
                let race = lib.classes[s.class].race.to_lowercase();
                races.push((e.to_bits(), ["pirate", "navy", "procyon", "civilian"].into_iter().find(|r| race.contains(r)).unwrap_or("")));
            }
            Side::Player => allies_lost += h.destroyed as usize,
        }
    }
    let now = time.elapsed_secs();
    let snap = Snapshot {
        hull: hp.core / class.core_hp.max(1.0),
        sails,
        enemies,
        allies_lost,
        docked,
        boarding,
        outcome: outcome.and_then(|o| match *o {
            Outcome::Playing => None,
            o => Some(o == Outcome::Victory),
        }),
        races,
        hazards,
        reports,
        fires,
        weapons_left,
    };
    for ev in tracker.observe(snap).into_iter().filter(|_| crew_speech) {
        if last.get(ev).is_none_or(|t| now - t >= COOLDOWN) && queue.len() < 3 {
            last.insert(ev, now);
            queue.push_back(ev);
        }
    }
    if busy.is_empty()
        && let Some(ev) = queue.pop_front()
    {
        *n += 1;
        if let Some(f) = line(ev, *n) {
            debug!("crew {ev}");
            commands.spawn((
                CrewVoice,
                AudioPlayer::new(assets.load(format!("voice/{f}.ogg"))),
                PlaybackSettings::DESPAWN.with_volume(bevy::audio::Volume::Linear(crate::menu::volume(crate::menu::Setting::Dialogue))),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap() -> Snapshot {
        Snapshot { hull: 1.0, sails: vec![1.0, 1.0], enemies: vec![(1, false, false)], ..default() }
    }

    #[test]
    fn events_fire_once_on_change() {
        let mut t = Tracker::default();
        assert!(t.observe(snap()).is_empty(), "first frame only records");
        let mut s = snap();
        s.hull = 0.7;
        s.sails = vec![0.5, 1.0];
        s.enemies = vec![(1, true, false)];
        assert_eq!(t.observe(s.clone()), ["hull_damage", "sails_damaged", "enemy_ship_spotted"]);
        assert!(t.observe(s.clone()).is_empty());
        s.sails = vec![0.0, 1.0];
        s.enemies = vec![(1, false, true)];
        s.allies_lost = 1;
        assert_eq!(t.observe(s.clone()), ["sails_destroyed", "enemy_ship_destroyed", "ship_lost"]);
        s.sails = vec![0.0, 0.0];
        s.outcome = Some(true);
        assert_eq!(t.observe(s), ["all_sails_destroyed", "mission_won"]);
    }

    #[test]
    fn every_wired_event_has_a_voice_file() {
        for ev in [
            "hull_damage",
            "sails_damaged",
            "sails_destroyed",
            "all_sails_destroyed",
            "enemy_ship_spotted",
            "enemy_ship_destroyed",
            "ship_lost",
            "dock",
            "undock",
            "landing_party",
            "mission_won",
            "mission_failed",
            "solar_storm",
            "nebula",
            "black_hole",
            "asteroids",
            "dragons",
            "whale_spotted",
            "ethereum_current",
            "meteor_shower",
            "torpedoes_away",
            "fire_mortar",
            "collision_alarm",
            "out_of_bounds",
            "put_out_fire",
            "all_weapons_destroyed",
            "enemy_ship_spotted_pirate",
            "enemy_ship_spotted_procyon",
        ] {
            let f = line(ev, 0).unwrap_or_else(|| panic!("{ev}"));
            assert!(crate::assets_dir().join(format!("voice/{f}.ogg")).is_file(), "{f}");
        }
    }
}
