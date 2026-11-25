//! Lifeboats (docs/re/lifeboats.md): a destroyed ship whose Create Lifeboats flag is set leaves `Number of lifeboats` boats of its race,
//! which drift away until any live ship touches one; the nearest such ship collects it. Lifeboats are identified by the class flag
//! `Is Lifeboat` only, never by name (`Ship_Navy_Lifeboat_Mission9` is a combat ship). Boarding captures are a separate feature.
use crate::EngineOrder;
use crate::campaign::{Mission, MissionShip};
use crate::combat::{Health, Library};
use crate::{PlayerShip, Ship, Side};
use bevy::prelude::*;

/// A lifeboat. `owner` is the rules' player name of the ship that left it; pre-placed boats (mission 4) also carry a `MissionShip`,
/// whose owner then wins so 'Set Group/Unit Owner' keeps working. Spawned boats have no `MissionShip`: rules address groups, not them.
#[derive(Component)]
pub struct Lifeboat {
    pub owner: crate::names::Owner,
}

/// Not retail: a ship whose crew was ordered off by the mission rule `OP Abandon Ship`. The hull stays afloat and alive for the rules, but
/// it cannot move or fire (the mission AI holds its throttle cap at 0, gunnery skips it); it can still be boarded and towed. Its lifeboats
/// launched when this was added, so a later destruction of the hull leaves none.
#[derive(Component)]
pub struct Abandoned;

/// A ship was destroyed in combat (core hit points gone, or the dev kill keys). Scripted `Destroy Group/Unit` is not reported:
/// whether it starts the original's lifeboat-spawning explosion state is unverified (docs/re/lifeboats.md §4.3).
#[derive(Message)]
pub struct ShipLost(pub Entity);

pub const PICKUP_SOUND: &str = "HUD_Lifeboat_Pickup";
/// Victory points for the local player collecting a lifeboat of a player it is not allied with.
pub const PICKUP_POINTS: u32 = 2;

/// The lifeboat class a ship of `race` leaves; unknown races leave Civilian boats, as in the original.
pub fn class_for(race: &str) -> String {
    format!("Ship_{}_Lifeboat", if matches!(race, "Navy" | "Civilian" | "Pirate" | "Procyon") { race } else { "Civilian" })
}

/// One boat's random drift: the original draws two uniform values in [-100, 100) as an offset (the path goal is 10000x that far away, i.e.
/// "sail that way forever") and a throttle `0.2 + 0.8u`. Returns the game-axes offset (x starboard, y forward) and the throttle.
pub fn drift(rng: &mut crate::combat::Rng) -> (Vec2, f32) {
    let mut u = || rng.f() * 200.0 - 100.0;
    let offset = Vec2::new(u(), u());
    (offset, (0.2 + 0.8 * rng.f()).clamp(0.0, 1.0))
}

/// Surface distance between two bounding spheres; <= 0 means contact.
pub fn surface_distance(a: (Vec3, f32), b: (Vec3, f32)) -> f32 {
    a.0.distance(b.0) - a.1 - b.1
}

fn world_sphere(lib: &Library, ship: &Ship, tf: &Transform) -> (Vec3, f32) {
    let (c, r) = lib.classes[ship.class].sphere;
    (tf.transform_point(c), r)
}

pub fn plugin(app: &mut App) {
    use crate::menu::Screen;
    app.add_message::<ShipLost>().add_systems(
        FixedUpdate,
        // Consume combat deaths before the next rules tick.
        lifeboats.in_set(crate::BattleSet::Cleanup).run_if(in_state(Screen::Battle)).run_if(crate::menu::overlay_closed),
    );
}

/// Spawns the boats of lost ships, runs the pickup test and keeps the rules' view of which boats exist.
fn lifeboats(
    mut commands: Commands,
    assets: Res<AssetServer>,
    lib: Res<Library>,
    fxl: Res<crate::fx::FxLib>,
    (mut rng, spawn_order): (ResMut<crate::combat::SimRng>, Query<&crate::scenario::SpawnIndex>),
    mut lost: MessageReader<ShipLost>,
    (abandoned, new_abandoned): (Query<(), With<Abandoned>>, Query<Entity, Added<Abandoned>>),
    mut sfx: MessageWriter<crate::audio::Sfx>,
    mission: Option<ResMut<Mission>>,
    dying: Query<(&Ship, &Transform, Option<&MissionShip>), Without<Lifeboat>>,
    boats: Query<(Entity, &Ship, &Transform, &Lifeboat, Option<&MissionShip>)>,
    // Not retail: an abandoned hull (`OP Abandon Ship`) has nobody aboard to take a boat in, its own or anyone's.
    ships: Query<(&Ship, &Transform, &Health, Option<&MissionShip>), (Without<Lifeboat>, Without<Abandoned>)>,
) {
    let owner = |l: &Lifeboat, m: Option<&MissionShip>| m.map_or_else(|| l.owner.clone(), |m| m.owner.clone());
    let mut present: Vec<(Option<Entity>, crate::names::Owner)> = boats.iter().map(|(e, _, _, l, m)| (Some(e), owner(l, m))).collect();
    // In spawn order of the lost ships: each boat's drift draws from the battle generator. A hull that was abandoned already
    // launched its boats (Not retail: `OP Abandon Ship`), so its destruction leaves none.
    for ShipLost(e) in crate::scenario::in_spawn_order(&spawn_order, lost.read(), |l| l.0) {
        if !abandoned.contains(*e) {
            launch(&mut commands, &assets, &lib, &fxl, &mut rng, *e, &dying, &mut present);
        }
    }
    // Not retail: the crews of ships that were just ordered off (`OP Abandon Ship`) take to the boats the same way, by the same flag.
    for e in crate::scenario::in_spawn_order(&spawn_order, new_abandoned.iter(), |e| *e) {
        launch(&mut commands, &assets, &lib, &fxl, &mut rng, e, &dying, &mut present);
    }
    let mut mission = mission;
    // Each boat is tested against the nearest live non-lifeboat ship (any owner, human or AI); contact of the bounding spheres collects it.
    for (boat, bship, btf, l, bm) in &boats {
        let sphere = world_sphere(&lib, bship, btf);
        let Some((d, cm)) =
            ships.iter().filter(|(_, _, h, _)| !h.destroyed).map(|(s, tf, _, m)| (surface_distance(sphere, world_sphere(&lib, s, tf)), m)).min_by(|a, b| a.0.total_cmp(&b.0))
        else {
            continue;
        };
        if d > 0.0 {
            continue;
        }
        info!("lifeboat picked up: owner {} by {:?} at {:?} d={d}", owner(l, bm), cm.map(|m| (&m.name, &m.owner)), btf.translation);
        crate::queue_removal(&mut commands, boat);
        present.retain(|p| p.0 != Some(boat));
        sfx.write(crate::audio::Sfx::ui(PICKUP_SOUND));
        if let Some(m) = mission.as_mut() {
            m.script.lifeboat_saved(cm.map_or("", |m| m.owner.as_str()), &owner(l, bm));
        }
    }
    if let Some(m) = mission.as_mut() {
        m.script.set_lifeboat_owners(present.into_iter().map(|p| p.1.to_string()).collect());
    }
}

/// The boats a ship leaves, if its Create Lifeboats flag is set and its class has any: used by a destruction and by `OP Abandon Ship`.
#[allow(clippy::too_many_arguments)]
fn launch(
    commands: &mut Commands,
    assets: &AssetServer,
    lib: &Library,
    fxl: &crate::fx::FxLib,
    rng: &mut crate::combat::SimRng,
    e: Entity,
    dying: &Query<(&Ship, &Transform, Option<&MissionShip>), Without<Lifeboat>>,
    present: &mut Vec<(Option<Entity>, crate::names::Owner)>,
) {
    let Ok((ship, tf, ms)) = dying.get(e) else { return };
    let class = &lib.classes[ship.class];
    let Some(ms) = ms.filter(|m| m.create_lifeboats && class.lifeboats > 0 && !class.is_lifeboat) else { return };
    let Some(boat) = lib.classes.iter().find(|c| c.name == class_for(&class.race)) else {
        warn!("no {} class: {} leaves no lifeboats", class_for(&class.race), class.name);
        return;
    };
    for _ in 0..class.lifeboats {
        let (offset, throttle) = drift(rng);
        let to = Vec3::new(offset.x, 0.0, -offset.y);
        let heading = if to.length_squared() > 1e-6 { crate::geom::heading_of(to) } else { ship.heading };
        let id = crate::spawn_ship(commands, assets, lib, fxl, &boat.name, Side::Player, tf.translation, heading, None);
        commands.entity(id).remove::<PlayerShip>().remove::<crate::boarding::Boardable>().insert((
            Lifeboat { owner: ms.owner.clone() },
            Ship {
                heading,
                order: EngineOrder::Full,
                waypoints: vec![tf.translation + to * 10000.0],
                throttle: Some(throttle),
                // Side::Player: never an enemy, the original treats lifeboats as friendly to everyone
                ..Ship::new(lib.index(&boat.name), Side::Player)
            },
        ));
        present.push((None, ms.owner.clone()));
    }
}

#[cfg(test)]
mod tests;
