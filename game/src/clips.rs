//! Mesh scene animation clips (`data/animations.json`, docs/re/effects.md "Break-apart animation clips"): keyframed offsets of a
//! node's position and yaw/pitch/roll from its rest pose. Ships play theirs once on death, base windmills loop theirs.
use bevy::prelude::*;
use serde_json::Value;
use std::collections::HashMap;

/// One keyframe: time, value, curve kind (0 linear, 1 Hermite with the stored tangents, 2 Kochanek-Bartels), tension, continuity,
/// bias, in tangent, out tangent.
type Key = [f32; 8];

#[derive(Clone, Debug, Default)]
pub struct Clip {
    pub duration: f32,
    /// Per animated element name: six channels (x, y, z in game axes, then yaw, pitch, roll in radians).
    motions: HashMap<String, [Vec<Key>; 6]>,
}

/// Every scene's clips by scene name, as in `data/animations.json`.
pub type Clips = HashMap<String, Vec<(String, Clip)>>;

/// Load `data/animations.json`; empty for imports that predate it.
pub fn load(root: &std::path::Path) -> Clips {
    let Some(doc) = crate::data::try_json(&root.join("data/animations.json")) else { return default() };
    let num = |v: &Value| v.as_f64().unwrap_or(0.0) as f32;
    let scenes = doc.as_object().into_iter().flatten();
    scenes
        .map(|(scene, clips)| {
            let clips = clips.as_array().into_iter().flatten().map(|c| {
                let motions = c["motions"].as_array().into_iter().flatten().filter_map(|m| {
                    let ch = m["channels"].as_array()?;
                    let keys = |i: usize| ch.get(i)?.as_array().map(|k| k.iter().map(|k| std::array::from_fn(|j| num(&k[j]))).collect::<Vec<Key>>());
                    Some((m["target"].as_str()?.to_string(), [keys(0)?, keys(1)?, keys(2)?, keys(3)?, keys(4)?, keys(5)?]))
                });
                (c["name"].as_str().unwrap_or("").to_string(), Clip { duration: num(&c["duration"]), motions: motions.collect() })
            });
            (scene.clone(), clips.collect())
        })
        .collect()
}

/// One channel at time `t`: a segment's first key picks the curve; values are held outside the keys. Matches `tools/skin.py`.
fn sample(keys: &[Key], t: f32) -> f32 {
    let (Some(first), Some(last)) = (keys.first(), keys.last()) else { return 0.0 };
    if t <= first[0] {
        return first[1];
    }
    for (k, w) in keys.windows(2).enumerate() {
        let (a, b) = (w[0], w[1]);
        if t > b[0] {
            continue;
        }
        let u = (t - a[0]) / (b[0] - a[0]).max(1e-6);
        if a[2] == 0.0 {
            return a[1] + (b[1] - a[1]) * u;
        }
        let (u2, u3) = (u * u, u * u * u);
        let h = [2.0 * u3 - 3.0 * u2 + 1.0, u3 - 2.0 * u2 + u, -2.0 * u3 + 3.0 * u2, u3 - u2];
        let (s0, s1) = if a[2] == 1.0 {
            (a[7], b[6])
        } else {
            let p = if k > 0 { keys[k - 1] } else { a };
            let (t0, c0, b0, t1, c1, b1) = (a[3], a[4], a[5], b[3], b[4], b[5]);
            (
                ((1.0 - t0) * (1.0 + c0) * (1.0 + b0) * (a[1] - p[1]) + (1.0 - t0) * (1.0 - c0) * (1.0 - b0) * (b[1] - a[1])) * ((b[0] - a[0]) / (b[0] - p[0]).max(1e-6)),
                (1.0 - t1) * (1.0 - c1) * (1.0 + b1) * (b[1] - a[1]),
            )
        };
        return h[0] * a[1] + h[1] * s0 + h[2] * b[1] + h[3] * s1;
    }
    last[1]
}

impl Clip {
    /// The pose of element `name` at clip time `t` as its rest transform plus the clip's offsets (position added, yaw about up then
    /// pitch about starboard then roll about forward added to the rest angles, here composed after the rest rotation), in Bevy
    /// axes. None if the clip does not animate it.
    pub fn pose(&self, name: &str, t: f32, rest: Transform) -> Option<Transform> {
        let ch = self.motions.get(name)?;
        let v: [f32; 6] = std::array::from_fn(|i| sample(&ch[i], t));
        let turn = Quat::from_rotation_y(v[3]) * Quat::from_rotation_x(v[4]) * Quat::from_rotation_z(-v[5]);
        Some(Transform { translation: rest.translation + crate::data::conv(v[0], v[1], v[2]), rotation: rest.rotation * turn, scale: rest.scale })
    }
    /// Whether the clip animates `name`.
    pub fn animates(&self, name: &str) -> bool {
        self.motions.contains_key(name)
    }
}

/// A base or island whose mesh scene has a looping `ENVIRONMENT` clip (windmills, mine machinery): plays from spawn.
#[derive(Component)]
pub struct SceneLoop(pub String);

/// The animated node's pose before the clip touched it.
#[derive(Component)]
struct Rest(Transform);

pub fn plugin(app: &mut App) {
    app.add_systems(Update, play_loops);
}

fn play_loops(
    mut commands: Commands,
    time: Res<Time>,
    lib: Option<Res<crate::data::Library>>,
    scenery: Query<(Entity, &SceneLoop)>,
    children: Query<&Children>,
    mut nodes: Query<(&Name, &mut Transform, Option<&Rest>)>,
) {
    let Some(lib) = lib else { return };
    for (e, SceneLoop(scene)) in &scenery {
        let Some(clip) = lib.clips.get(scene).and_then(|c| c.iter().find(|(n, _)| n == "ENVIRONMENT")).map(|(_, c)| c) else { continue };
        let t = time.elapsed_secs() % clip.duration.max(1e-3);
        for d in children.iter_descendants(e) {
            let Ok((name, mut tf, rest)) = nodes.get_mut(d) else { continue };
            if !clip.animates(name) {
                continue;
            }
            let Some(rest) = rest else {
                commands.entity(d).insert(Rest(*tf));
                continue;
            };
            if let Some(pose) = clip.pose(name, t, rest.0) {
                *tf = pose;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(keys: [Vec<Key>; 6]) -> Clip {
        Clip { duration: 2.0, motions: HashMap::from([("n".to_string(), keys)]) }
    }

    #[test]
    fn linear_offsets_move_and_turn_from_the_rest_pose() {
        let line = |v: f32| vec![[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], [2.0, v, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]];
        let c = clip([line(4.0), line(2.0), line(0.0), line(1.0), vec![], line(0.0)]);
        let rest = Transform::from_xyz(10.0, 0.0, 0.0);
        let p = c.pose("n", 1.0, rest).unwrap();
        // game +y (forward) is Bevy -z; yaw turns about up
        assert!((p.translation - Vec3::new(12.0, 0.0, -1.0)).length() < 1e-5, "{:?}", p.translation);
        assert!((p.rotation.angle_between(Quat::from_rotation_y(0.5))) < 1e-5);
        assert!(c.pose("other", 1.0, rest).is_none());
        assert_eq!(c.pose("n", 9.0, rest).unwrap().translation, Vec3::new(14.0, 0.0, -2.0), "held after the last key");
    }
}
