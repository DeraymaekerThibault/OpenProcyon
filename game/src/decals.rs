//! Damage decals (AV-03, docs/fidelity-backlog.md): a shell that strikes a hull leaves a scar texture from `BD_DecalTextures`
//! (assets/decals, data/decals.json from tools/export_decals.py): the bucket for the struck section's material, the size class of
//! the bullet's 'Decal Damage Size'. Decoded: every hit that is not a ricochet leaves a scar of 0.25 / 0.5 / 1.0 units, crowded scars merge
//! (`merge`), at most 150 per ship. PROVISIONAL: the original clips the scar onto the hull triangles; here it is a camera-lit quad
//! facing back along the shell's path. A dock repair that brings every section back to full clears them (ours, not retail). A destroyed section's scars go with it (its mesh is swapped
//! for the broken one), and a destroyed ship loses them all before it breaks up (owner's call: they looked wrong on the pieces).
use bevy::prelude::*;
use std::collections::HashMap;

use crate::combat::{Health, Library};

/// No new scar once a ship carries more than this many (decoded; old ones are never replaced).
const CAP: usize = 150;
/// Scar edge per size class (world units, decoded: the decal box edge).
const SIZE: [f32; 3] = [0.25, 0.5, 1.0];
/// How much of a new scar's class (row) an older one (column) may be covered by, in percent of overlap, before the older one is merged
/// away; at or above it the new scar is refused. An older scar of a smaller class is always merged away.
const KEEP_BELOW: [[f32; 3]; 3] = [[50.0, 20.0, 0.0], [75.0, 10.0, 0.0], [99.0, 70.0, 0.0]];

/// The scars that crowd a new scar of class `new` at `at` among `old` (position, class), or None when one of them refuses it.
/// Returns the indices to merge away and the class the new scar ends up with: two nearby small hits grow into a medium.
fn merge(old: &[(Vec3, u8)], at: Vec3, new: u8) -> Option<(Vec<usize>, u8)> {
    let mut gone = vec![];
    for (i, &(p, class)) in old.iter().enumerate() {
        let r = SIZE[class as usize] + SIZE[new as usize];
        let d = p.distance(at);
        if d >= r {
            continue;
        }
        let overlap = (r * r - d * d) / (r * r) * 100.0;
        if class < new || overlap < KEEP_BELOW[new as usize][class as usize] {
            gone.push(i);
        } else {
            return None;
        }
    }
    let largest = gone.iter().map(|&i| old[i].1).max();
    let class = largest.map_or(new, |l| if new <= l { (l + 1).min(2) } else { new });
    Some((gone, class))
}

/// Section `section` of `ship` was struck at `local` (ship frame) by a shell travelling along `dir` (ship frame).
#[derive(Message)]
pub struct Scar {
    pub ship: Entity,
    pub section: usize,
    pub local: Vec3,
    pub dir: Vec3,
    pub material: &'static str,
    pub size: u8,
}

#[derive(Resource, Default)]
struct DecalLib {
    /// (material, size class) -> texture names.
    pool: HashMap<(String, u8), Vec<String>>,
    mats: HashMap<String, Handle<StandardMaterial>>,
    quad: Option<Handle<Mesh>>,
    n: usize,
}

/// A ship's scars with the section each one is on.
#[derive(Component, Default)]
struct Scars(Vec<(Entity, usize, Vec3, u8)>);

pub fn plugin(app: &mut App) {
    let doc = crate::data::try_json(&crate::assets_dir().join("data/decals.json")).unwrap_or_default();
    let mut pool: HashMap<(String, u8), Vec<String>> = HashMap::new();
    for (name, d) in doc.as_object().into_iter().flatten() {
        let size = d["size"].as_u64().unwrap_or(1) as u8;
        for m in d["materials"].as_array().into_iter().flatten().filter_map(|m| m.as_str()) {
            pool.entry((m.to_string(), size)).or_default().push(name.clone());
        }
    }
    for v in pool.values_mut() {
        v.sort();
    }
    app.add_message::<Scar>().insert_resource(DecalLib { pool, ..default() }).add_systems(Update, (place, mend).run_if(in_state(crate::menu::Screen::Battle)));
}

fn place(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut lib: ResMut<DecalLib>,
    mut msgs: MessageReader<Scar>,
    mut ships: Query<Option<&mut Scars>>,
) {
    let lib = &mut *lib;
    for s in msgs.read() {
        let Ok(scars) = ships.get_mut(s.ship) else { continue };
        let old: Vec<(Vec3, u8)> = scars.as_ref().map_or(vec![], |l| l.0.iter().map(|o| (o.2, o.3)).collect());
        if old.len() > CAP {
            continue;
        }
        let Some((gone, class)) = merge(&old, s.local, s.size.min(2)) else { continue };
        // Nearest size class that has art for the material.
        let Some(names) = (class..=2).find_map(|z| lib.pool.get(&(s.material.to_string(), z)).filter(|v| !v.is_empty()).map(|v| (z, v))) else { continue };
        let (size, names) = names;
        lib.n += 1;
        let name = names[lib.n % names.len()].clone();
        let quad = lib.quad.get_or_insert_with(|| meshes.add(Rectangle::new(1.0, 1.0))).clone();
        let mat = lib
            .mats
            .entry(name.clone())
            .or_insert_with(|| {
                materials.add(StandardMaterial {
                    base_color_texture: Some(assets.load(format!("decals/{name}.png"))),
                    alpha_mode: AlphaMode::Blend,
                    double_sided: true,
                    cull_mode: None,
                    depth_bias: 50.0,
                    perceptual_roughness: 1.0,
                    ..default()
                })
            })
            .clone();
        let facing = -s.dir.normalize_or(Vec3::Z);
        let tf = Transform::from_translation(s.local + facing * 0.05)
            .looking_to(-facing, if facing.y.abs() > 0.9 { Vec3::X } else { Vec3::Y })
            .with_scale(Vec3::splat(SIZE[size as usize]));
        let e = commands.spawn((Mesh3d(quad), MeshMaterial3d(mat), tf, ChildOf(s.ship))).id();
        match scars {
            Some(mut list) => {
                for &i in gone.iter().rev() {
                    commands.entity(list.0.remove(i).0).try_despawn();
                }
                list.0.push((e, s.section, s.local, size));
            }
            None => {
                commands.entity(s.ship).insert(Scars(vec![(e, s.section, s.local, size)]));
            }
        }
    }
}

/// Scars go when their section is destroyed, all of them when the ship is destroyed, and all of them when every section is
/// back at full (a dock refit).
fn mend(mut commands: Commands, lib: Res<Library>, mut ships: Query<(&crate::Ship, &Health, &mut Scars)>) {
    for (ship, h, mut scars) in &mut ships {
        let class = &lib.classes[ship.class];
        let mended = h.sections.iter().zip(&class.sections).all(|(hp, s)| *hp >= s.hp);
        scars.0.retain(|&(e, section, ..)| {
            let keep = !h.destroyed && !mended && h.sections.get(section).is_some_and(|hp| *hp > 0.0);
            if !keep {
                commands.entity(e).try_despawn();
            }
            keep
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crowded_small_scars_merge_into_a_medium_and_big_ones_refuse_small_ones() {
        let at = Vec3::ZERO;
        // A small scar half covered by a new small one (36 % overlap) is merged away and the new one grows to Medium.
        assert_eq!(merge(&[(Vec3::new(0.4, 0.0, 0.0), 0)], at, 0), Some((vec![0], 1)));
        // Right under it (96 %) the new one is refused.
        assert_eq!(merge(&[(Vec3::new(0.1, 0.0, 0.0), 0)], at, 0), None);
        // Far apart: nothing happens.
        assert_eq!(merge(&[(Vec3::new(5.0, 0.0, 0.0), 0)], at, 0), Some((vec![], 0)));
        // A Large scar under a new Small one refuses it.
        assert_eq!(merge(&[(Vec3::new(0.1, 0.0, 0.0), 2)], at, 0), None);
        // A smaller old scar is always merged away by a bigger new one.
        assert_eq!(merge(&[(Vec3::new(0.1, 0.0, 0.0), 0)], at, 2), Some((vec![0], 2)));
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn scars_go_with_their_destroyed_section_and_with_a_destroyed_ship() {
        let lib = Library::load(crate::assets_dir());
        let class = lib.index("Ship_Navy_Cutter");
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).add_systems(Update, mend);
        let mut health = Health::new(&lib.classes[class]);
        health.sections[0] -= 1.0; // damaged, so no "mended" clear-out
        app.insert_resource(lib);
        let (a, b) = (app.world_mut().spawn_empty().id(), app.world_mut().spawn_empty().id());
        let ship = crate::Ship {
            class,
            side: crate::Side::Enemy,
            heading: 0.0,
            speed: 0.0,
            ang_vel: 0.0,
            order: 0,
            waypoints: vec![],
            goal: None,
            cap: 1.0,
            throttle: None,
            sail_energy: 1.0,
            crew_speed: 1.0,
            crew_turn: 1.0,
            net: 1.0,
            avoid: 1.0,
        };
        let e = app.world_mut().spawn((ship, health, Scars(vec![(a, 0, Vec3::ZERO, 0), (b, 1, Vec3::ZERO, 0)]))).id();
        app.update();
        assert!(app.world().get_entity(a).is_ok() && app.world().get_entity(b).is_ok(), "scars on living sections stay");
        app.world_mut().get_mut::<Health>(e).unwrap().sections[1] = 0.0;
        app.update();
        assert!(app.world().get_entity(b).is_err(), "the destroyed section's scar is gone");
        assert!(app.world().get_entity(a).is_ok());
        app.world_mut().get_mut::<Health>(e).unwrap().destroyed = true;
        app.update();
        assert!(app.world().get_entity(a).is_err(), "a destroyed ship keeps no scars for its breakup");
    }
}
