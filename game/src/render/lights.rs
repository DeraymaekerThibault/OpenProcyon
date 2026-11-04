//! Not retail: point lights the render profile's `lights` list puts on effects by file name (part of render/mod.rs's look).
//! A flash fades out quadratically over `seconds`; a steady light (`seconds` 0) lives as long as its effect and may flicker.

use bevy::prelude::*;
use serde_json::Value;
use std::collections::HashMap;

use super::{Look, num, rgb};
use crate::fx::{EffectStarted, FxLib};

/// A point light the profile gives every effect whose file name contains `effects`.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectLight {
    pub effects: String,
    pub color: Color,
    /// Peak intensity in lumens.
    pub lumens: f32,
    pub range: f32,
    /// How long it fades out (quadratically); 0 = steady for as long as the effect lives.
    pub seconds: f32,
    /// Share of the intensity that flickers (fires).
    pub flicker: f32,
}

/// The `lights` list; an entry without `effects` is dropped.
pub fn parse(v: &Value) -> Vec<EffectLight> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|l| {
            Some(EffectLight {
                effects: l["effects"].as_str()?.to_string(),
                color: rgb(&l["color"]).map_or(Color::WHITE, |c| Color::linear_rgb(c.x, c.y, c.z)),
                lumens: num(&l["lumens"], 1e6),
                range: num(&l["range"], 100.0),
                seconds: num(&l["seconds"], 0.3),
                flicker: num(&l["flicker"], 0.0),
            })
        })
        .collect()
}

/// The light for one effect file name: the first entry whose `effects` it contains.
pub fn light_for<'a>(lights: &'a [EffectLight], name: &str) -> Option<&'a EffectLight> {
    lights.iter().find(|l| name.contains(&l.effects))
}

pub fn plugin(app: &mut App) {
    app.add_systems(Update, (effect_lights, fade_lights));
}

/// A light following an effect (its child), fading over `seconds` (0: steady), part of it flickering.
#[derive(Component)]
pub struct EffectGlow {
    pub peak: f32,
    pub seconds: f32,
    pub age: f32,
    pub flicker: f32,
}

fn effect_lights(
    mut commands: Commands,
    look: Res<Look>,
    lib: Option<Res<FxLib>>,
    mut started: MessageReader<EffectStarted>,
    mut by_effect: Local<Option<HashMap<usize, EffectLight>>>,
) {
    let Some(lib) = lib else { return };
    let table = by_effect.get_or_insert_with(|| lib.names().filter_map(|(n, i)| Some((i, light_for(&look.lights, n)?.clone()))).collect());
    for m in started.read() {
        let e = m.entity;
        let Some(l) = table.get(&m.effect) else { continue };
        // a flash starts at its peak; a steady light starts at a random point of its flicker, so fires do not pulse in step
        let age = if l.seconds > 0.0 { 0.0 } else { e.index().index() as f32 * 0.37 % 7.0 };
        let light = (
            PointLight { color: l.color, intensity: l.lumens, range: l.range, shadow_maps_enabled: false, ..default() },
            EffectGlow { peak: l.lumens, seconds: l.seconds, age, flicker: l.flicker },
            Transform::default(),
        );
        // short effects can be gone by the time commands apply
        commands.entity(e).queue_silenced(move |mut fx: EntityWorldMut| {
            fx.with_child(light);
        });
    }
}

/// Intensity of a light `age` seconds in: the quadratic fade (if any) times the flicker.
fn glow_at(g: &EffectGlow) -> Option<f32> {
    let fade = if g.seconds > 0.0 {
        let left = 1.0 - g.age / g.seconds;
        if left <= 0.0 {
            return None;
        }
        left * left
    } else {
        1.0
    };
    let t = g.age;
    let noise = 0.5 + 0.5 * ((13.0 * t).sin() * (7.3 * t + 1.1).sin() * 0.7 + (23.0 * t + 2.0).sin() * 0.3);
    Some(g.peak * fade * (1.0 - g.flicker * noise))
}

fn fade_lights(mut commands: Commands, time: Res<Time>, mut lights: Query<(Entity, &mut EffectGlow, &mut PointLight)>) {
    for (e, mut glow, mut light) in &mut lights {
        if glow.seconds <= 0.0 && glow.flicker <= 0.0 {
            continue;
        }
        // a flash's age starts at 0; a steady light's offset only decorrelates its flicker
        glow.age += time.delta_secs();
        match glow_at(&glow) {
            Some(i) => light.intensity = i,
            None => commands.entity(e).despawn(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn the_first_matching_light_wins_and_entries_need_effects() {
        let lights = parse(&serde_json::json!([{"effects": "Explosion_", "lumens": 5.0, "seconds": 1}, {"effects": "_Impact", "color": [1, 0.5, 0.25]}, {"color": [1, 1, 1]}]));
        assert_eq!(lights.len(), 2, "an entry without `effects` is dropped");
        assert_eq!(light_for(&lights, "Explosion_Huge").map(|l| l.lumens), Some(5.0));
        assert_eq!(light_for(&lights, "CannonBall_Heavy_Impact").map(|l| l.color), Some(Color::linear_rgb(1.0, 0.5, 0.25)));
        assert!(light_for(&lights, "NavyEngine_Heavy").is_none());
    }

    #[test]
    fn a_started_effect_gets_the_light_of_its_file_name() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.init_resource::<Messages<EffectStarted>>();
        world.insert_resource(FxLib::with_names(&["Gun_Muzzle_Flash", "NavyEngine_Heavy"]));
        world.insert_resource(Look::parse(&serde_json::json!({"lights": [{"effects": "Muzzle", "lumens": 5.0, "seconds": 0.2}]})));
        let (flash, engine) = world
            .run_system_once(|mut commands: Commands, lib: Res<FxLib>| {
                let at = Transform::from_xyz(1.0, 2.0, 3.0);
                let flash = crate::fx::spawn(&mut commands, &lib, "Gun_Muzzle_Flash", None, at, at, true).unwrap();
                (flash, crate::fx::spawn(&mut commands, &lib, "NavyEngine_Heavy", None, at, at, false).unwrap())
            })
            .unwrap();
        world.run_system_once(effect_lights).unwrap();
        let light = world.get::<Children>(flash).and_then(|c| c.iter().find_map(|c| world.get::<EffectGlow>(c))).expect("the flash carries its light");
        assert_eq!((light.peak, light.seconds, light.age), (5.0, 0.2, 0.0));
        assert!(world.get::<Children>(engine).is_none(), "no rule, no light");
    }

    #[test]
    fn effect_lights_fade_out_and_steady_ones_stay() {
        let mut world = World::new();
        world.insert_resource(Time::<()>::default());
        let flash = world.spawn((EffectGlow { peak: 100.0, seconds: 1.0, age: 0.0, flicker: 0.0 }, PointLight::default())).id();
        let steady = world.spawn((EffectGlow { peak: 100.0, seconds: 0.0, age: 0.0, flicker: 0.0 }, PointLight { intensity: 100.0, ..default() })).id();
        let fire = world.spawn((EffectGlow { peak: 100.0, seconds: 0.0, age: 0.0, flicker: 0.5 }, PointLight { intensity: 100.0, ..default() })).id();
        world.resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.5));
        world.run_system_once(fade_lights).unwrap();
        assert!((world.get::<PointLight>(flash).unwrap().intensity - 25.0).abs() < 1e-3, "quadratic fade: half way is a quarter");
        let flame = world.get::<PointLight>(fire).unwrap().intensity;
        assert!((50.0..=100.0).contains(&flame), "flicker takes at most its share: {flame}");
        world.resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.6));
        world.run_system_once(fade_lights).unwrap();
        assert!(world.get_entity(flash).is_err(), "gone once faded");
        assert_eq!(world.get::<PointLight>(steady).unwrap().intensity, 100.0);
        assert!(world.get_entity(fire).is_ok(), "a fire's light lasts as long as the fire");
    }
}
