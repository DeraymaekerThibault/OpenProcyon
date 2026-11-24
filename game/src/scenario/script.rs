//! Test-script steps of the battle (`--script <name>` with `--shot`): runtime self-checks and screenshot setups that press keys, set
//! orders and pick targets at fixed frames. They are not game rules; production systems in `scenario.rs` know nothing about them.

use bevy::prelude::*;

use crate::smoke::Script;
use crate::{Config, EngineOrder, Health, Ship, Side, lifeboats};
use crate::{Orbit, Target};

/// Scripted runs: `pause` / `pause-restart` check the Pause key, `speedtest` cycles the engine order, `course` sets waypoints,
/// `target` / `fxtest` pick the first enemy so the target reticle can be screenshotted (`fxtest`: so it fires).
pub fn scripted_steps(
    cfg: Res<Config>,
    mut target: ResMut<Target>,
    mut orbit: ResMut<Orbit>,
    mut ships: Query<(Entity, &mut Ship)>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    time: Res<Time<Virtual>>,
    loading: Option<Res<crate::loading::Loading>>,
) {
    // The frame count stands still while a battle loads: a step must not repeat on every loading frame (a press of R would restart again).
    if loading.is_some() {
        return;
    }
    // Runtime self-check: Pause key freezes simulation time, then resumes it while the HUD stays usable.
    if matches!(cfg.script, Some(Script::Pause | Script::PauseRestart)) {
        let resume_key = if cfg.script == Some(Script::PauseRestart) { KeyCode::KeyR } else { KeyCode::Pause };
        match cfg.frame {
            30 => {
                keys.press(KeyCode::Pause);
            }
            31 => {
                keys.release(KeyCode::Pause);
            }
            60 => {
                keys.press(resume_key);
            }
            61 => {
                keys.release(resume_key);
            }
            40 => {
                assert!(time.is_paused(), "Pause must pause the simulation");
                assert_eq!(time.delta_secs(), 0.0, "paused simulation time must not advance");
            }
            70 => assert!(!time.is_paused(), "resume/restart must unpause the simulation"),
            _ => {}
        }
    }
    if cfg.script == Some(Script::Speedtest) {
        for (_, mut s) in &mut ships {
            if s.side == Side::Player {
                s.order = EngineOrder::from_index(((cfg.frame / 90) % 4) as usize).unwrap_or_default();
            }
        }
    }
    if cfg.script == Some(Script::Course) && cfg.frame == 3 {
        for (_, mut s) in &mut ships {
            if s.side == Side::Player {
                s.waypoints = vec![Vec3::new(-90.0, 0.0, -80.0), Vec3::new(40.0, 0.0, -200.0)];
                s.order = EngineOrder::Full;
                s.goal = None;
            }
        }
    }
    // fxtest clicks the enemy too: the command ship only fires at a clicked target.
    if matches!(cfg.script, Some(Script::Target | Script::FxTest)) && cfg.frame >= 5 && target.0.is_none() {
        target.0 = ships.iter().find(|(_, s)| s.side == Side::Enemy).map(|(e, _)| e);
        if cfg.script == Some(Script::Target) {
            orbit.focus = target.0; // screenshots look at the target
        }
    }
}

/// Whether a script presses H (hurt the player's ship), K (destroy the enemies) or J (destroy the player) at this frame, as
/// `(hurt, kill_enemies, kill_player)`: `damage` hurts at frame 30; `victory` and `restart` kill the enemies and `defeat` the player at
/// frame 300 of a screenshot run (demos of the end screens).
pub fn script_kills(script: Option<Script>, shot: bool, frame: u32) -> (bool, bool, bool) {
    let at = shot && frame == 300;
    (shot && script == Some(Script::Damage) && frame == 30, at && matches!(script, Some(Script::Victory | Script::Restart)), at && script == Some(Script::Defeat))
}

/// Applies the scripted kills of `script_kills`.
pub fn scripted_kills(cfg: Res<Config>, mut ships: Query<(&Ship, &mut Health), Without<lifeboats::Lifeboat>>) {
    let (hurt, kill_enemies, kill_player) = script_kills(cfg.script, cfg.shot.is_some(), cfg.frame);
    if hurt || kill_enemies || kill_player {
        super::apply_kills(&mut ships, hurt, kill_enemies, kill_player);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_kill_only_at_their_frame_in_a_screenshot_run() {
        assert_eq!(script_kills(Some(Script::Damage), true, 30), (true, false, false));
        assert_eq!(script_kills(Some(Script::Damage), false, 30), (false, false, false));
        assert_eq!(script_kills(Some(Script::Victory), true, 300), (false, true, false));
        assert_eq!(script_kills(Some(Script::Restart), true, 300), (false, true, false));
        assert_eq!(script_kills(Some(Script::Defeat), true, 300), (false, false, true));
        assert_eq!(script_kills(Some(Script::Defeat), true, 299), (false, false, false));
        assert_eq!(script_kills(None, true, 300), (false, false, false));
    }
}
