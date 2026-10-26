//! Predicted course of the player ship, drawn like the original with HUD_Waypointline2 (an additive 64x128 dash texture) laid
//! along the path as one quad per dash. PROVISIONAL: dash width/length, colour fade and the end marker are tuned by eye.
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::{
    PlayerShip, Scenario, Ship,
    collision::{self, Obstacle},
    combat::{Health, Library},
    step,
};

// One tile per PITCH of arc length: the 64x128 texture runs across the path (its long axis), so a tile is about twice as wide as long, with gaps between tiles.
const TILE_W: f32 = 2.4;
const TILE_L: f32 = 6.4;
const PITCH: f32 = 8.8;
const MARK: f32 = 9.0; // destination star size

#[derive(Component)]
pub struct Course;
/// Waypoint stars: 0 the course's (accept art), 1 plotted-but-not-accepted points (do-nothing art), 2 the plot's cancel X.
#[derive(Component)]
pub struct Marks(u8);
impl Marks {
    #[cfg(test)]
    pub fn test() -> Self {
        Marks(0)
    }
}

/// Movement type (`MainBar_BeginnerMovementTypeButton`): basic sends the ship at once; advanced plots a sequence of waypoints
/// that the player accepts (the check mark on the last point, or Enter) or cancels (the X beside it, or Backspace).
/// PROVISIONAL: button placement and keys are the remake's reading of the manual's "plot, then accept/cancel".
#[derive(Resource, Default)]
pub struct Plot {
    pub advanced: bool,
    pub pending: Vec<Vec3>,
}
impl Plot {
    /// World positions of the accept and cancel buttons of a pending plot.
    pub fn buttons(&self) -> Option<(Vec3, Vec3)> {
        let last = *self.pending.last()?;
        Some((last, last + Vec3::X * MARK * 1.4))
    }
    /// Which button a ground click at `p` presses: Some(true) accept, Some(false) cancel.
    pub fn pressed(&self, p: Vec3) -> Option<bool> {
        let (a, c) = self.buttons()?;
        if p.with_y(0.0).distance(a.with_y(0.0)) < MARK / 2.0 {
            Some(true)
        } else if p.with_y(0.0).distance(c.with_y(0.0)) < MARK / 2.0 {
            Some(false)
        } else {
            None
        }
    }
}

pub fn update(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    lib: Res<Library>,
    player: Single<(&Ship, &Transform, &Health, Has<crate::docking::Docker>), With<PlayerShip>>,
    obstacles: Query<(&Obstacle, Option<&Visibility>), Without<Ship>>,
    mission: Option<Res<crate::campaign::Mission>>,
    course: Query<&Mesh3d, With<Course>>,
    marks: Query<(&Mesh3d, &Marks), Without<Course>>,
    plot: Option<Res<Plot>>,
    mut odo: Local<(Vec3, f32)>, // (last ship position, distance travelled)
) {
    let Ok(mesh) = course.single() else {
        let mesh = meshes.add(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()));
        let mat = mats.add(StandardMaterial {
            base_color_texture: Some(assets.load("hud/tex/hud_waypointline2.png")),
            unlit: true,
            cull_mode: None,
            double_sided: true,
            alpha_mode: AlphaMode::Add,
            ..default()
        });
        commands.spawn((Scenario, Course, Mesh3d(mesh), MeshMaterial3d(mat), Transform::default(), Visibility::default(), NoFrustumCulling));
        for (k, tex) in ["hud_waypoint_accept", "hud_waypoint_donothing", "hud_waypoint_decline"].into_iter().enumerate() {
            let mesh = meshes.add(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()));
            let mat = mats.add(StandardMaterial {
                base_color_texture: Some(assets.load(format!("hud/tex/{tex}.png"))),
                unlit: true,
                cull_mode: None,
                double_sided: true,
                alpha_mode: AlphaMode::Blend,
                ..default()
            });
            commands.spawn((Scenario, Marks(k as u8), Mesh3d(mesh), MeshMaterial3d(mat), Transform::default(), Visibility::default(), NoFrustumCulling));
        }
        return;
    };
    let (ship, tf, health, docking) = *player;
    let none = Plot::default();
    let plot = plot.as_deref().unwrap_or(&none);
    let shown = !docking && !crate::panels::cinematic(mission.as_deref());
    // Dots sit at fixed arc distances in odometer terms, so they stay put in the world and vanish under the ship as it passes.
    odo.1 += odo.0.distance(tf.translation);
    odo.0 = tf.translation;
    let mut line: Vec<Vec3> = vec![];
    // A pending plot previews its course instead of the one being sailed.
    let plotted = !plot.pending.is_empty();
    let ship = &if plotted { Ship { waypoints: plot.pending.clone(), order: ship.order.max(1), ..ship.clone() } } else { ship.clone() };
    if !ship.waypoints.is_empty() && shown {
        // Run the real movement step forward so the curve shows the turning radius, then stop drawing where the
        // ship's box would first touch scenery. PROVISIONAL: free motion only (no drift/steer-away), 0.1 s sample steps.
        let class = &lib.classes[ship.class];
        let (mut sim, mut st) = (ship.clone(), *tf);
        let mut path = vec![st];
        for _ in 0..600 {
            step(&mut sim, &mut st, class, health, 0.1);
            path.push(st);
            if sim.waypoints.is_empty() {
                break;
            }
        }
        let scenery = collision::scenery(obstacles.iter());
        let keep = collision::clear_prefix(class.collision_bounds, &path, &scenery);
        line = path[..keep].iter().map(|t| t.translation + Vec3::Y * 0.1).collect();
    }
    let (mut pos, mut uv, mut col, mut idx) = (vec![], vec![], vec![], vec![]);
    let mut carried = odo.1 % PITCH;
    let mut samples: Vec<(Vec3, Vec3)> = vec![]; // (position, tangent)
    for w in line.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = a.distance(b);
        let mut d = PITCH - carried;
        while d <= len {
            samples.push((a.lerp(b, d / len), (b - a).normalize_or_zero()));
            d += PITCH;
        }
        carried = len - (d - PITCH);
    }
    let n = samples.len();
    for (i, (p, t)) in samples.iter().enumerate() {
        let side = t.cross(Vec3::Y).try_normalize().unwrap_or(Vec3::X);
        let a = (1.0 - i as f32 / n as f32 * 0.4).clamp(0.0, 1.0);
        let k = pos.len() as u32;
        // texture v (black bands at its ends) runs along the path so the bands make the gaps; u across
        for (da, dc, u, v) in [(-1.0, -1.0, 0.0, 0.0), (1.0, -1.0, 0.0, 1.0), (1.0, 1.0, 1.0, 1.0), (-1.0, 1.0, 1.0, 0.0)] {
            pos.push((*p + *t * da * TILE_L / 2.0 + side * dc * TILE_W / 2.0).to_array());
            uv.push([u, v]);
            col.push([1.0, 1.0, 1.0, a]);
        }
        idx.extend([k, k + 1, k + 2, k, k + 2, k + 3]);
    }
    // Do not re-upload unchanged empty meshes every frame while docking or idle.
    if (!pos.is_empty() || meshes.get(&mesh.0).is_some_and(|m| m.count_vertices() > 0))
        && let Some(mut m) = meshes.get_mut(&mesh.0)
    {
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
        m.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
        m.insert_indices(Indices::U32(idx));
    }
    // Destination stars (HUD_Waypoint_Accept) flat on the plane at every waypoint; a pending plot shows do-nothing stars with the
    // accept star and the cancel X at its last point.
    for (m, Marks(kind)) in &marks {
        let points: Vec<Vec3> = match (kind, plot.buttons()) {
            (0, Some((accept, _))) => vec![accept],
            (0, None) => ship.waypoints.clone(),
            (1, Some(_)) => plot.pending[..plot.pending.len() - 1].to_vec(),
            (2, Some((_, cancel))) => vec![cancel],
            _ => vec![],
        };
        let (mut pos, mut uv, mut idx) = (vec![], vec![], vec![]);
        for w in points.iter().filter(|_| shown) {
            let k = pos.len() as u32;
            for (dx, dz, u, v) in [(-1.0, -1.0, 0.0, 0.0), (1.0, -1.0, 1.0, 0.0), (1.0, 1.0, 1.0, 1.0), (-1.0, 1.0, 0.0, 1.0)] {
                pos.push((*w + Vec3::new(dx, 0.15, dz) * MARK / 2.0).to_array());
                uv.push([u, v]);
            }
            idx.extend([k, k + 1, k + 2, k, k + 2, k + 3]);
        }
        if (!pos.is_empty() || meshes.get(&m.0).is_some_and(|m| m.count_vertices() > 0))
            && let Some(mut mm) = meshes.get_mut(&m.0)
        {
            mm.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
            mm.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
            mm.insert_indices(Indices::U32(idx));
        }
    }
}
