//! A skirmish map's own script (`maps/<id>/script.json`): the same interpreter as the campaign, driving the map's convoys, timers and
//! team points. The end rules stay with victory.rs; only the effects the skirmish maps use are applied here (paths, owners,
//! visibility, hold, throttle caps, stances, damage, space animals' paths and speed, storm routes).
use crate::Nearest;
use bevy::prelude::*;
use serde_json::Value;
use std::collections::HashMap;

use super::*;
use crate::campaign::MissionShip;
use crate::combat::{Destroyed, Health};
use crate::{PlayerShip, Ship, Side, game_to_bevy};

#[derive(Resource)]
pub struct SkirmishScript {
    pub script: Script,
    /// The side each map player fights for (an owner change moves a ship to the new owner's side).
    pub sides: HashMap<String, Side>,
    paths: Value,
    /// The script's environment block (space animal and dragon definitions).
    pub environment: Value,
}

impl SkirmishScript {
    /// The map's script, if it has one. `teams`: (team name id, its players); `sides`: the side of each map player.
    pub fn load(map: &str, teams: Vec<(String, Vec<String>)>, sides: HashMap<String, Side>) -> Option<Self> {
        let data = crate::data::try_json(&crate::assets_dir().join(format!("maps/{map}/script.json")))?;
        let mut script = Script::new(&without_end_rules(&data), Value::Null);
        script.set_teams(teams);
        Some(SkirmishScript { script, sides, paths: data["paths"].clone(), environment: data["environment"].clone() })
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
    app.add_systems(FixedUpdate, run.in_set(crate::BattleSet::Rules).run_if(in_state(crate::menu::Screen::Battle)).run_if(resource_exists::<SkirmishScript>));
}

fn run(
    mut sk: ResMut<SkirmishScript>,
    time: Res<Time>,
    mut killed: MessageReader<Destroyed>,
    mut captured: MessageReader<crate::boarding::Captured>,
    mut ships: Query<(Entity, &mut Ship, &Transform, &mut Health, &mut MissionShip, &mut Visibility, Has<PlayerShip>)>,
    mut animals: Query<(Entity, &mut crate::environment::SpaceAnimal, &Transform, &mut Visibility), Without<Ship>>,
    mut meteors: MessageWriter<crate::nebula::MeteorRoute>,
    mut belts: MessageWriter<crate::combat::AsteroidSetup>,
    rocks: Query<(Entity, &Transform, &crate::combat::Asteroid), (Without<Ship>, Without<crate::environment::SpaceAnimal>)>,
    mut dragons: Query<&mut crate::dragons::Dragon>,
) {
    let sk = &mut *sk;
    let mut world: Vec<Unit> = ships
        .iter()
        .map(|(e, _, tf, h, m, _, primary)| Unit {
            e,
            id: m.id,
            name: m.name.clone(),
            owner: m.owner.clone(),
            owner_is_named_group: false,
            groups: vec![],
            pos: tf.translation,
            alive: !h.destroyed,
            visible: m.visible,
            primary,
            damage: 0.0,
            flag: String::new(),
            attack: vec![],
        })
        .collect();
    world.extend(animals.iter().map(|(e, a, tf, _)| a.unit(e, tf)));
    // Moving belts: the rocks are units too, tagged with the script's groups by their world-object id.
    world.extend(rocks.iter().map(|(e, tf, a)| a.unit(e, tf)));
    sk.script.tag(&mut world);
    for d in killed.read() {
        if let Some(by) = world.iter().find(|u| u.e == d.by) {
            sk.script.record_destroyed(&by.owner.clone(), d.victim);
        }
    }
    for c in captured.read() {
        sk.script.record_capture(&c.by, &c.victim, c.unit, false);
    }
    let (fxs, _) = sk.script.step(&world, time.delta_secs(), false);
    for fx in fxs {
        match fx {
            Fx::Path { units, path, closest, looping } => {
                info!("map script: {} unit(s) take route '{path}'", units.len());
                let pts: Vec<Vec3> = sk.paths[path.as_str()].as_array().map(|a| a.iter().map(game_to_bevy).collect()).unwrap_or_default();
                for e in &units {
                    if let Ok((_, mut animal, tf, _)) = animals.get_mut(*e) {
                        animal.follow(&pts, tf.translation, closest, looping);
                        continue;
                    }
                    let Ok((_, mut sh, tf, _, mut ms, ..)) = ships.get_mut(*e) else { continue };
                    let from = if closest { (0..pts.len()).nearest_to(tf.translation, |a| pts[*a]).unwrap_or(0) } else { 0 };
                    sh.throttle = None;
                    sh.waypoints = pts[from.min(pts.len())..].to_vec();
                    sh.order = 2;
                    sh.goal = None;
                    ms.stop_at_end = !looping;
                    ms.looping = looping.then(|| pts.clone());
                }
            }
            Fx::Owner(units, owner) => {
                info!("map script: {} ship(s) now belong to '{owner}'", units.len());
                for e in &units {
                    if let Ok((_, mut animal, ..)) = animals.get_mut(*e) {
                        animal.owner.clone_from(&owner);
                    }
                    let Ok((_, mut sh, _, _, mut ms, ..)) = ships.get_mut(*e) else { continue };
                    if let Some(side) = sk.sides.get(&owner) {
                        sh.side = *side;
                    }
                    ms.owner = owner.clone();
                }
            }
            Fx::Visible(units, on) => {
                for e in &units {
                    if let Ok((_, mut animal, _, mut vis)) = animals.get_mut(*e) {
                        animal.visible = on;
                        *vis = if on { Visibility::Inherited } else { Visibility::Hidden };
                    }
                    let Ok((.., mut ms, mut vis, _)) = ships.get_mut(*e) else { continue };
                    ms.visible = on;
                    *vis = if on { Visibility::Inherited } else { Visibility::Hidden };
                }
            }
            Fx::Hold(units, true) => {
                for e in &units {
                    let Ok((_, mut sh, ..)) = ships.get_mut(*e) else { continue };
                    sh.waypoints.clear();
                    sh.order = 0;
                }
            }
            Fx::Throttle(units, cap) => {
                for e in &units {
                    let Ok((_, mut sh, ..)) = ships.get_mut(*e) else { continue };
                    sh.cap = cap;
                }
            }
            Fx::Velocity(units, speed) => {
                for e in units {
                    if let Ok((_, mut animal, ..)) = animals.get_mut(e) {
                        animal.speed = speed;
                    }
                }
            }
            Fx::Asteroids { units, path, closest, mode, speed, tumble } => {
                let path = sk.paths[path.as_str()].as_array().map(|a| a.iter().map(game_to_bevy).collect()).unwrap_or_default();
                belts.write(crate::combat::AsteroidSetup { units, path, closest, mode, speed, tumble });
            }
            Fx::MeteorPath { group, path, closest, mode } => {
                let path = sk.paths[path.as_str()].as_array().map(|a| a.iter().map(game_to_bevy).collect()).unwrap_or_default();
                meteors.write(crate::nebula::MeteorRoute::Path { group, path, closest, mode });
            }
            Fx::MeteorSpeed(group, speed) => {
                meteors.write(crate::nebula::MeteorRoute::Speed { group, speed });
            }
            Fx::DragonStance(units, stance) => {
                for e in units {
                    if let Ok(mut d) = dragons.get_mut(e) {
                        d.stance = stance;
                    }
                }
            }
            Fx::DragonThreshold(units, threshold) => {
                for e in units {
                    if let Ok(mut d) = dragons.get_mut(e) {
                        d.threshold = threshold;
                    }
                }
            }
            Fx::Stance(units, stance) => {
                for e in &units {
                    let Ok((.., mut ms, _, _)) = ships.get_mut(*e) else { continue };
                    ms.stance = stance;
                }
            }
            Fx::Damage(units, part) => {
                for e in &units {
                    let Ok((_, _, _, mut h, ..)) = ships.get_mut(*e) else { continue };
                    h.scale(1.0 - part);
                }
            }
            _ => {}
        }
    }
}
