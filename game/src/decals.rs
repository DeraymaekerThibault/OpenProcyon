//! Damage decals (AV-03, docs/fidelity-backlog.md): a shell that strikes a hull leaves a scar texture from `BD_DecalTextures`
//! (assets/decals, data/decals.json from tools/export_decals.py): the bucket for the struck section's material, the size class of
//! the bullet's 'Decal Damage Size'. PROVISIONAL: the original's trigger, projection and per-ship cap are unknown; here every hit that
//! damages a section leaves a camera-lit quad facing back along the shell's path, up to `CAP` per ship (oldest first out), and a
//! dock repair that brings every section back to full clears them.
use bevy::prelude::*;
use std::collections::HashMap;

use crate::combat::{Health, Library};

const CAP: usize = 24;
/// Quad edge per size class (world units).
const SIZE: [f32; 3] = [1.2, 2.0, 3.0];

/// A section of `ship` was struck at `local` (ship frame) by a shell travelling along `dir` (ship frame).
#[derive(Message)]
pub struct Scar {
    pub ship: Entity,
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

#[derive(Component, Default)]
struct Scars(Vec<Entity>);

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
        // Nearest size class that has art for the material.
        let Some(names) = [s.size, 1, 0, 2].into_iter().find_map(|z| lib.pool.get(&(s.material.to_string(), z)).filter(|v| !v.is_empty()).map(|v| (z, v))) else { continue };
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
                list.0.push(e);
                if list.0.len() > CAP {
                    let old = list.0.remove(0);
                    commands.entity(old).try_despawn();
                }
            }
            None => {
                commands.entity(s.ship).insert(Scars(vec![e]));
            }
        }
    }
}

/// A ship whose every section is back at full (a dock refit) loses its scars.
fn mend(mut commands: Commands, lib: Res<Library>, mut ships: Query<(&crate::Ship, &Health, &mut Scars)>) {
    for (ship, h, mut scars) in &mut ships {
        let class = &lib.classes[ship.class];
        if !scars.0.is_empty() && h.sections.iter().zip(&class.sections).all(|(hp, s)| *hp >= s.hp) {
            for e in scars.0.drain(..) {
                commands.entity(e).try_despawn();
            }
        }
    }
}
