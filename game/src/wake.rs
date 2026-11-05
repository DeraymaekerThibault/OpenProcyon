//! Ship wakes (docs/re/effects.md "Wakes"): a Trail primitive attached to the ship's wake placement, read from the original's code.
//! Segments are laid in world space every 15 engine ticks and then drift on their own; the newest one follows the stern.
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::{Scenario, Ship};

/// Engine tick (retail constant 0.033).
const TICK: f32 = 1.0 / 30.0;
/// A segment lives 300 ticks; it fades in over the first 60 and out over the rest.
const SEG_LIFE: i32 = 300;
const SEG_EVERY: i32 = 15;
/// The speed-to-wake multiplier is refreshed once per second (the keeper's update gate).
const REFRESH: f32 = 1.0;
/// Alpha per tick: up 1/60 for the first 60 ticks, then down 1/240; a stopped trail fades x0.9 per tick.
const FADE_IN: f32 = 1.0 / 60.0;
const FADE_OUT: f32 = 1.0 / 240.0;
const FADE_STOPPED: f32 = 0.9;
/// Texture scroll along the trail per tick; the offset wraps at 0.5. PROVISIONAL: how the offset enters the texture coordinates.
const SCROLL: f32 = 0.01;

/// A Trail primitive's authored numbers (`BackDrift`, `OutDrift`, `InitialWidth` of the effect file).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrailParams {
    pub back: f32,
    pub out: f32,
    pub width: f32,
}

impl TrailParams {
    fn load(effect: &str) -> Self {
        let j = crate::data::try_json(&crate::assets_dir().join(format!("fx/{effect}.json"))).unwrap_or_default();
        let p = &j["Primitive"][0];
        let f = |k: &str, d: f32| p[k].as_f64().map_or(d, |v| v as f32);
        // Defaults of the original's constructor.
        Self { back: f("BackDrift", 0.15), out: f("OutDrift", 0.05), width: f("InitialWidth", 3.0) }
    }
}

/// Where the trail's node is: its position and the node's starboard and astern axes (the wake placement faces astern).
#[derive(Clone, Copy)]
pub struct Frame {
    pub pos: Vec3,
    pub right: Vec3,
    pub astern: Vec3,
}

struct Seg {
    a: Vec3,
    b: Vec3,
    out: Vec3,
    back: Vec3,
    life: i32,
    alpha: f32,
}

/// The trail state of one ship, stepped at 30 Hz.
pub struct Trail {
    p: TrailParams,
    segs: Vec<Seg>,
    countdown: i32,
    /// Set when the multiplier reaches 0: no new segments until the old ones are gone.
    stopped: bool,
    /// The ship's speed fraction (0..1), scaling the backward drift of segments laid from now on.
    pub mult: f32,
    scroll: f32,
    rng: u32,
}

impl Trail {
    pub fn new(p: TrailParams, mult: f32, seed: u32) -> Self {
        Self { p, segs: vec![], countdown: 0, stopped: false, mult, scroll: 0.0, rng: seed | 1 }
    }

    fn rand(&mut self) -> f32 {
        self.rng = self.rng.wrapping_mul(0x19660d).wrapping_add(0x3c6ef35f);
        f32::from_bits(self.rng & 0x7fffff | 0x3f800000) - 1.0
    }

    /// Per-tick random wobble of an edge: +-0.07 along the horizontal axes, +-0.02 vertically (Bevy axes).
    fn jitter(&mut self) -> Vec3 {
        Vec3::new(self.rand() * 0.14 - 0.07, self.rand() * 0.04 - 0.02, self.rand() * 0.14 - 0.07)
    }

    fn seg_at(&self, f: &Frame) -> Seg {
        let out = f.right * self.p.out;
        Seg { a: f.pos + out * self.p.width, b: f.pos - out * self.p.width, out, back: f.astern * self.p.back * self.mult, life: SEG_LIFE, alpha: 0.0 }
    }

    /// One engine tick with the node at `frame`.
    pub fn tick(&mut self, frame: &Frame) {
        if self.mult <= 0.0 {
            self.stopped = true;
        } else if !self.stopped {
            self.countdown -= 1;
            if self.countdown < 1 {
                self.countdown = SEG_EVERY;
                let s = self.seg_at(frame);
                self.segs.push(s);
            } else if let Some(last) = self.segs.len().checked_sub(1) {
                // Between laying points the newest segment is rewritten from the node, so the trail stays joined to the stern.
                self.segs[last] = self.seg_at(frame);
            }
        }
        if self.mult == 0.0 {
            for s in &mut self.segs {
                s.alpha *= FADE_STOPPED;
            }
        }
        for i in 0..self.segs.len() {
            let (ja, jb) = (self.jitter(), self.jitter());
            let s = &mut self.segs[i];
            s.life -= 1;
            s.a += s.out + s.back + ja;
            s.b += -s.out + s.back + jb;
            s.alpha = if SEG_LIFE - s.life < 60 { (s.alpha + FADE_IN).min(1.0) } else { (s.alpha - FADE_OUT).max(0.0) };
        }
        self.segs.retain(|s| s.life > 0);
        if self.segs.is_empty() {
            self.stopped = false;
        }
        self.scroll = (self.scroll - SCROLL).rem_euclid(0.5);
    }
}

#[derive(Component)]
pub struct Wake {
    ship: Entity,
    /// Index into the class's wake placements (twin-hull ships have two, one trail each).
    idx: usize,
    trail: Trail,
    acc: f32,
    since_refresh: f32,
}

impl Wake {
    /// The ship this wake trails.
    pub fn ship(&self) -> Entity {
        self.ship
    }
}

/// Multiplier of the wake texture per fleet type, from the optional `data/fx/wake_tint.json` (`{"Navy": [r, g, b]}`, values may exceed 1).
/// Absent: the retail colour for everyone. A mod ships the file, so wakes can tell the factions apart without new textures.
pub fn wake_tint(race: &str) -> Option<[f32; 3]> {
    static TINTS: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    let tints = TINTS.get_or_init(|| crate::data::try_json(&crate::assets_dir().join("data/fx/wake_tint.json")).unwrap_or_default());
    let v = tints.get(race)?.as_array()?;
    Some([v.first()?.as_f64()? as f32, v.get(1)?.as_f64()? as f32, v.get(2)?.as_f64()? as f32])
}

/// The ship's speed as a fraction of Full, 0..1. PROVISIONAL: the original multiplies two physics values we have not named.
fn speed_fraction(sh: &Ship, lib: &crate::combat::Library) -> f32 {
    (sh.speed / crate::ship::full_speed(&lib.classes[sh.class])).clamp(0.0, 1.0)
}

fn frame(tf: &Transform, at: Vec3) -> Frame {
    // The placement faces astern (yaw pi): its y axis points astern and its x axis to port; segments are symmetric about it.
    Frame { pos: tf.translation + tf.rotation * at, right: tf.rotation * Vec3::NEG_X, astern: tf.rotation * Vec3::Z }
}

pub fn update(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<crate::fx::EffectMaterial>>,
    ships: Query<(Entity, &Transform, &Ship, Option<&crate::combat::Health>), With<Ship>>,
    lib: Res<crate::combat::Library>,
    mut wakes: Query<(Entity, &mut Wake, &Mesh3d)>,
    cloaks: Query<&crate::cloak::Cloak>,
) {
    let dt = time.delta_secs().min(0.25);
    for (s, _, sh, health) in &ships {
        if health.is_some_and(|h| h.destroyed) {
            continue;
        }
        let class = &lib.classes[sh.class];
        for (idx, (_, effect)) in class.wake.iter().enumerate() {
            if wakes.iter().any(|(_, w, _)| w.ship == s && w.idx == idx) {
                continue;
            }
            let mesh = meshes.add(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()));
            let mut material = crate::fx::additive_material(assets.load("fx/tex/wake.png"));
            if let Some([r, g, b]) = wake_tint(&class.race) {
                material.base.base_color = Color::LinearRgba(LinearRgba::new(r, g, b, 1.0));
            }
            let trail = Trail::new(TrailParams::load(effect), speed_fraction(sh, &lib), s.index().index() * 2 + idx as u32);
            let wake = Wake { ship: s, idx, trail, acc: 0.0, since_refresh: 0.0 };
            commands.spawn((Scenario, wake, Mesh3d(mesh), MeshMaterial3d(mats.add(material)), Transform::default(), Visibility::default(), NoFrustumCulling));
        }
    }
    for (e, mut w, mesh) in &mut wakes {
        let Ok((_, tf, sh, health)) = ships.get(w.ship) else {
            commands.entity(e).despawn();
            continue;
        };
        if health.is_some_and(|h| h.destroyed) {
            commands.entity(e).despawn();
            continue;
        }
        let Some((at, _)) = lib.classes[sh.class].wake.get(w.idx) else { continue };
        w.since_refresh += dt;
        if w.since_refresh > REFRESH {
            w.since_refresh = 0.0;
            // The wake fades with the hull as a ship cloaks (1 - level), gone at full cloak.
            w.trail.mult = speed_fraction(sh, &lib) * (1.0 - cloaks.get(w.ship).map_or(0.0, |c| c.level()));
        }
        w.acc += dt;
        let node = frame(tf, *at);
        while w.acc >= TICK {
            w.acc -= TICK;
            w.trail.tick(&node);
        }
        let (mut pos, mut uv, mut col, mut idx) = (vec![], vec![], vec![], vec![]);
        for (i, s) in w.trail.segs.iter().enumerate() {
            // PROVISIONAL: one half texture per segment, scrolled along the trail.
            let v = 0.5 * i as f32 + w.trail.scroll;
            pos.extend([s.a, s.b].map(|v| v.to_array()));
            uv.extend([[0.0, v], [1.0, v]]);
            col.extend([[s.alpha, s.alpha, s.alpha, 1.0]; 2]); // fade baked into RGB for ONE/ONE blending
            if i > 0 {
                let b = (i * 2) as u32;
                idx.extend([b - 2, b - 1, b, b - 1, b + 1, b]);
            }
        }
        // An empty mesh is only written once, to clear the last frame's wake: rewriting it every frame makes Bevy warn "no vertices".
        if (!pos.is_empty() || meshes.get(&mesh.0).is_some_and(|m| m.count_vertices() > 0))
            && let Some(mut m) = meshes.get_mut(&mesh.0)
        {
            m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
            m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
            m.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
            m.insert_indices(Indices::U32(idx));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs imported assets"]
    fn twin_hull_ships_have_two_wakes() {
        let lib = crate::combat::Library::load(crate::assets_dir());
        let n = |name: &str| lib.classes.iter().find(|c| c.name == name).map(|c| c.wake.len());
        assert_eq!(n("Ship_Procyon_Dreadnought"), Some(2));
        assert_eq!(n("Ship_Navy_Frigate"), Some(1));
    }

    fn trail(mult: f32) -> Trail {
        Trail::new(TrailParams { back: 0.13, out: 0.04, width: 40.0 }, mult, 1)
    }
    const NODE: Frame = Frame { pos: Vec3::ZERO, right: Vec3::X, astern: Vec3::Z };

    #[test]
    fn a_point_is_laid_every_15_ticks_and_lives_300() {
        let mut t = trail(1.0);
        (0..31).for_each(|_| t.tick(&NODE));
        assert_eq!(t.segs.len(), 3, "ticks 1, 16 and 31");
        (0..300).for_each(|_| t.tick(&NODE));
        assert!(t.segs.iter().all(|s| s.life > 0));
        // A point is rewritten from the node for its first 14 ticks, so its 300 start when the next one is laid.
        assert_eq!(t.segs.len(), 21, "the first points are gone after 300 ticks");
    }

    #[test]
    fn a_laid_point_widens_and_drifts_astern_by_the_speed_multiplier() {
        let far = Frame { pos: Vec3::new(500.0, 0.0, 0.0), ..NODE };
        let (mut full, mut half) = (trail(1.0), trail(0.5));
        for t in [&mut full, &mut half] {
            (0..100).for_each(|_| t.tick(&far));
        }
        // The newest point is rewritten from the node every tick until the next one is laid, so the first one stops at tick 15.
        let (f, h) = (&full.segs[0], &half.segs[0]);
        let n = (SEG_LIFE - f.life) as f32;
        assert_eq!(n, 86.0);
        let width = (f.a - f.b).x.abs();
        assert!((width - 2.0 * 0.04 * (40.0 + n)).abs() < 2.0, "initial width 2 x OutDrift x InitialWidth, then 2 x OutDrift per tick: {width}");
        assert!((f.a.z - 0.13 * n).abs() < 2.0 && (h.a.z - 0.5 * 0.13 * n).abs() < 2.0, "back drift scales with the multiplier");
    }

    #[test]
    fn alpha_rises_for_60_ticks_then_falls_and_a_stopped_wake_waits_for_its_trail() {
        let far = Frame { pos: Vec3::new(500.0, 0.0, 0.0), ..NODE };
        let mut t = trail(1.0);
        (0..74).for_each(|_| t.tick(&far));
        assert!(t.segs[0].alpha > 0.95, "peak after the 59 ticks since it was last rewritten: {}", t.segs[0].alpha);
        t.mult = 0.0;
        (0..40).for_each(|_| t.tick(&far));
        assert!(t.segs[0].alpha < 0.05, "a stopped wake fades fast");
        t.mult = 1.0;
        let before = t.segs.len();
        t.tick(&far);
        assert!(t.segs.len() <= before, "no new points while the old trail is still there");
    }
}
