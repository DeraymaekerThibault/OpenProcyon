//! A skirmish map's own script (`maps/<id>/script.json`): the same interpreter as the campaign, driving the map's convoys, timers and
//! team points. The end rules stay with victory.rs; only the effects the skirmish maps use are applied here (paths, owners,
//! visibility, hold, throttle caps, stances, damage).
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
}

impl SkirmishScript {
    /// The map's script, if it has one. `teams`: (team name id, its players); `sides`: the side of each map player.
    pub fn load(map: &str, teams: Vec<(String, Vec<String>)>, sides: HashMap<String, Side>) -> Option<Self> {
        let data = crate::data::try_json(&crate::assets_dir().join(format!("maps/{map}/script.json")))?;
        let mut script = Script::new(&without_end_rules(&data), Value::Null);
        script.set_teams(teams);
        Some(SkirmishScript { script, sides, paths: data["paths"].clone() })
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
    app.add_systems(Update, run.run_if(in_state(crate::menu::Screen::Battle)).run_if(resource_exists::<SkirmishScript>));
}

fn run(
    mut sk: ResMut<SkirmishScript>,
    time: Res<Time>,
    mut killed: MessageReader<Destroyed>,
    mut captured: MessageReader<crate::boarding::Captured>,
    mut ships: Query<(Entity, &mut Ship, &Transform, &mut Health, &mut MissionShip, &mut Visibility, Has<PlayerShip>)>,
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
                info!("map script: {} ship(s) take route '{path}'", units.len());
                let pts: Vec<Vec3> = sk.paths[path.as_str()].as_array().map(|a| a.iter().map(game_to_bevy).collect()).unwrap_or_default();
                for e in &units {
                    let Ok((_, mut sh, tf, _, mut ms, ..)) = ships.get_mut(*e) else { continue };
                    let from = if closest { (0..pts.len()).min_by(|a, b| pts[*a].distance(tf.translation).total_cmp(&pts[*b].distance(tf.translation))).unwrap_or(0) } else { 0 };
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
                    let Ok((_, mut sh, _, _, mut ms, ..)) = ships.get_mut(*e) else { continue };
                    if let Some(side) = sk.sides.get(&owner) {
                        sh.side = *side;
                    }
                    ms.owner = owner.clone();
                }
            }
            Fx::Visible(units, on) => {
                for e in &units {
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
