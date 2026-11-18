//! Not retail: hiding the retail drawing of what a render profile replaces (`replace` in the `nebulae` and `black_holes` blocks).
//! A replaced effect keeps running exactly as before (its particles, timers, lights and sounds, the cloud's sim, the hole's pull and
//! vortex trail); only its quads are not drawn (`fx::HiddenFx`, honoured by the particle batcher), so the profile can draw its own
//! instead of stacking on the retail look. The cloud shell is hidden in `nebulae.rs`.
//!
//! Which effects: the retail glow, beam and ribbon layers of a hole (`BlackHoleRibbons_*`) when `black_holes.replace`, and the retail
//! `Lightning` sprite of a storm bolt when a `SolarStorm` rule replaces. Nothing here writes to the simulation.

use bevy::prelude::*;

use super::Look;
use crate::fx::{EffectStarted, FxLib, HiddenFx};

/// Whether an effect (by its file name) is drawn by the profile instead, given the profile.
pub fn replaced(look: &Look, effect: &str) -> bool {
    (look.black_holes.as_ref().is_some_and(|b| b.replace) && effect.starts_with("BlackHoleRibbons_"))
        || (effect == "Lightning" && look.nebulae.as_ref().is_some_and(|n| n.replaces_storms()))
}

pub fn plugin(app: &mut App, look: &Look) {
    if ["BlackHoleRibbons_Small", "Lightning"].iter().any(|n| replaced(look, n)) {
        app.add_systems(Update, hide_replaced);
    }
}

fn hide_replaced(
    mut commands: Commands,
    look: Res<Look>,
    lib: Option<Res<FxLib>>,
    mut started: MessageReader<EffectStarted>,
    mut table: Local<Option<std::collections::HashSet<usize>>>,
) {
    let Some(lib) = lib else { return };
    let table = table.get_or_insert_with(|| lib.names().filter(|(n, _)| replaced(&look, n)).map(|(_, i)| i).collect());
    for m in started.read() {
        if table.contains(&m.effect) {
            // short effects can be gone by the time commands apply
            commands.entity(m.entity).queue_silenced(|mut e: EntityWorldMut| {
                e.insert(HiddenFx);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_replacing_blocks_hide_their_own_effects() {
        let classic = Look::parse(&json!({}));
        let holes = Look::parse(&json!({"black_holes": {"replace": true}}));
        let storms = Look::parse(&json!({"nebulae": {"rules": [{"match": "SolarStorm", "replace": true}]}}));
        let clouds = Look::parse(&json!({"nebulae": {"replace": true, "rules": [{"match": "Nebula"}]}}));
        let stacked = Look::parse(&json!({"black_holes": {}, "nebulae": {"rules": [{"match": "SolarStorm"}]}}));
        for (look, ribbons, bolt) in [(&classic, false, false), (&holes, true, false), (&storms, false, true), (&clouds, false, false), (&stacked, false, false)] {
            assert_eq!(replaced(look, "BlackHoleRibbons_Large"), ribbons);
            assert_eq!(replaced(look, "Lightning"), bolt);
            assert!(!replaced(look, "Explosion_Light") && !replaced(look, "MeteorStorm"), "nothing else is hidden");
        }
    }

    fn app(look: Look) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<EffectStarted>()
            .insert_resource(FxLib::with_names(&["BlackHoleRibbons_Small", "Lightning", "Explosion_Light"]))
            .insert_resource(look);
        app.add_systems(Update, hide_replaced);
        app
    }

    #[test]
    fn a_replaced_effect_is_marked_hidden_and_keeps_its_components() {
        let mut app = app(Look::parse(&json!({"black_holes": {"replace": true}})));
        let (ribbons, other) = (app.world_mut().spawn(Transform::default()).id(), app.world_mut().spawn(Transform::default()).id());
        let started = |entity, effect| EffectStarted { entity, effect, at: Vec3::ZERO, forward: Vec3::Z, shell: None, host: None };
        app.world_mut().resource_mut::<Messages<EffectStarted>>().write(started(ribbons, 0));
        app.world_mut().resource_mut::<Messages<EffectStarted>>().write(started(other, 2));
        app.update();
        assert!(app.world().entity(ribbons).contains::<HiddenFx>() && app.world().entity(ribbons).contains::<Transform>());
        assert!(!app.world().entity(other).contains::<HiddenFx>());
    }
}
