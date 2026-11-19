//! Lifeboats (docs/re/lifeboats.md): a destroyed ship whose Create Lifeboats flag is set leaves `Number of lifeboats` boats of its race,
//! which drift away until any live ship touches one; the nearest such ship collects it. Lifeboats are identified by the class flag
//! `Is Lifeboat` only, never by name (`Ship_Navy_Lifeboat_Mission9` is a combat ship). Boarding captures are a separate feature.
use crate::campaign::{Mission, MissionShip};
use crate::combat::{Health, Library};
use crate::{PlayerShip, Ship, Side};
use bevy::prelude::*;

/// A lifeboat. `owner` is the rules' player name of the ship that left it; pre-placed boats (mission 4) also carry a `MissionShip`,
/// whose owner then wins so 'Set Group/Unit Owner' keeps working. Spawned boats have no `MissionShip`: rules address groups, not them.
#[derive(Component)]
pub struct Lifeboat {
    pub owner: String,
}

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
    mut rng: ResMut<crate::combat::Rng>,
    mut lost: MessageReader<ShipLost>,
    mut sfx: MessageWriter<crate::audio::Sfx>,
    mission: Option<ResMut<Mission>>,
    dying: Query<(&Ship, &Transform, Option<&MissionShip>), Without<Lifeboat>>,
    boats: Query<(Entity, &Ship, &Transform, &Lifeboat, Option<&MissionShip>)>,
    ships: Query<(&Ship, &Transform, &Health, Option<&MissionShip>), Without<Lifeboat>>,
) {
    let owner = |l: &Lifeboat, m: Option<&MissionShip>| m.map_or_else(|| l.owner.clone(), |m| m.owner.clone());
    let mut present: Vec<(Option<Entity>, String)> = boats.iter().map(|(e, _, _, l, m)| (Some(e), owner(l, m))).collect();
    for ShipLost(e) in lost.read() {
        let Ok((ship, tf, ms)) = dying.get(*e) else { continue };
        let class = &lib.classes[ship.class];
        let Some(ms) = ms.filter(|m| m.create_lifeboats && class.lifeboats > 0 && !class.is_lifeboat) else { continue };
        let Some(boat) = lib.classes.iter().find(|c| c.name == class_for(&class.race)) else {
            warn!("no {} class: {} leaves no lifeboats", class_for(&class.race), class.name);
            continue;
        };
        for _ in 0..class.lifeboats {
            let (offset, throttle) = drift(&mut rng);
            let to = Vec3::new(offset.x, 0.0, -offset.y);
            let heading = if to.length_squared() > 1e-6 { (-to.x).atan2(-to.z) } else { ship.heading };
            let id = crate::spawn_ship(&mut commands, &assets, &lib, &fxl, &boat.name, Side::Player, tf.translation, heading, None);
            commands.entity(id).remove::<PlayerShip>().remove::<crate::boarding::Boardable>().insert((
                Lifeboat { owner: ms.owner.clone() },
                Ship {
                    heading,
                    order: 2,
                    waypoints: vec![tf.translation + to * 10000.0],
                    throttle: Some(throttle),
                    // Side::Player: never an enemy, the original treats lifeboats as friendly to everyone
                    ..Ship::new(lib.index(&boat.name), Side::Player)
                },
            ));
            present.push((None, ms.owner.clone()));
        }
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
        m.script.set_lifeboat_owners(present.into_iter().map(|p| p.1).collect());
    }
}

#[cfg(test)]
mod tests;
