//! Cloaking (docs/re/cloaking.md): the Procyon submersible's cloak, ordered from the right-click menu (CLOAK_START/STOP)
//! or by the rules ('Set Group/Unit Cloak State', mission 12). Data: WOT 'Is Cloakable' on the hull and
//! 'Can Be Fired While Cloaked' per gun (torpedoes, gatlings); effects Cloaking / DeCloaking / FullyCloaked and the IC_Sub_*
//! sounds. PROVISIONAL: the fade time, the 40-unit "seen anyway" distance and AI override behaviour are the remake's; the original's
//! cloak timing is unknown.
use bevy::prelude::*;

use crate::audio::Sfx;
use crate::{Ship, Side};

/// Seconds to fade fully in or out.
const FADE: f32 = 2.0;
/// Closer than this a cloaked ship is still seen.
pub const SEEN_WITHIN: f32 = 40.0;

#[derive(Component, Default)]
pub struct Cloak {
    pub on: bool,
    /// 0 visible .. 1 fully cloaked.
    pub level: f32,
    /// The rules let its AI drop the cloak to fire ('AI Can Override').
    pub ai_override: bool,
    shown: Option<Entity>,
    sound: Option<Entity>,
}
impl Cloak {
    pub fn new(on: bool, ai_override: bool) -> Self {
        Cloak { on, level: if on { 1.0 } else { 0.0 }, ai_override, shown: None, sound: None }
    }
    pub fn hidden(&self) -> bool {
        self.level > 0.5
    }
}

/// Cloak gate used by shared sensors; close-range exception remains provisional.
pub fn seen(hidden: bool, at: Vec3, from: Vec3) -> bool {
    !hidden || at.distance(from) < SEEN_WITHIN
}

pub fn plugin(app: &mut App) {
    app.add_systems(Update, fade.run_if(in_state(crate::menu::Screen::Battle)));
}

fn fade(
    time: Res<Time>,
    mut commands: Commands,
    assets: Res<AssetServer>,
    fxl: Option<Res<crate::fx::FxLib>>,
    mut ships: Query<(Entity, &mut Cloak, &Ship, &Transform, &mut Visibility, Option<&crate::campaign::MissionShip>)>,
    mut sfx: MessageWriter<Sfx>,
) {
    let dt = time.delta_secs();
    for (e, mut c, ship, tf, mut vis, ms) in &mut ships {
        let was = c.level;
        c.level = (c.level + if c.on { dt / FADE } else { -dt / FADE }).clamp(0.0, 1.0);
        let started = was == 0.0 && c.level > 0.0;
        let ended = was == 1.0 && c.level < 1.0;
        if started || ended {
            sfx.write(Sfx { name: if started { "IC_Sub_Cloak" } else { "IC_Sub_Uncloak" }.into(), at: Some(tf.translation), vol: 1.0, max: 600.0 });
            if let Some(fxl) = fxl.as_deref() {
                let local = Transform::default();
                crate::fx::spawn(&mut commands, fxl, if started { "Cloaking" } else { "DeCloaking" }, Some(e), local, *tf, true);
            }
        }
        // While fully cloaked: the shimmer on the hull and the generator hum.
        let full = c.level >= 1.0;
        match (full, c.shown) {
            (true, None) => {
                if let Some(fxl) = fxl.as_deref() {
                    c.shown = crate::fx::spawn(&mut commands, fxl, "FullyCloaked", Some(e), Transform::default(), *tf, false);
                }
                c.sound = Some(
                    commands
                        .spawn((
                            ChildOf(e),
                            AudioPlayer::new(assets.load(crate::audio::sound("IC_Sub_CloakingLoop"))),
                            PlaybackSettings::LOOP.with_volume(bevy::audio::Volume::Linear(0.3)),
                        ))
                        .id(),
                );
            }
            (false, Some(f)) => {
                commands.entity(f).try_despawn();
                if let Some(s) = c.sound.take() {
                    commands.entity(s).try_despawn();
                }
                c.shown = None;
            }
            _ => {}
        }
        // A cloaked enemy disappears from the player's view; the player's own cloaked ships stay drawn (with the shimmer).
        let script_visible = ms.is_none_or(|m| m.visible);
        let want = if !script_visible || (ship.side == Side::Enemy && c.hidden()) { Visibility::Hidden } else { Visibility::Inherited };
        vis.set_if_neq(want);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cloak_hides_beyond_close_range_only_once_mostly_faded() {
        let mut c = Cloak::new(false, false);
        assert!(seen(c.hidden(), Vec3::X * 300.0, Vec3::ZERO));
        c.level = 0.8;
        assert!(!seen(c.hidden(), Vec3::X * 300.0, Vec3::ZERO));
        assert!(seen(c.hidden(), Vec3::X * 10.0, Vec3::ZERO));
        assert!(seen(false, Vec3::X * 300.0, Vec3::ZERO));
    }
}
