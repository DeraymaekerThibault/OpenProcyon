//! Scripted wiring checks; these are not substitutes for manual gameplay validation.
mod fleet;
mod lifeboats;
mod report;
mod shells;
pub use fleet::fleet_smoke;

use crate::{Config, campaign::Mission, rules::RuleShip};
use bevy::prelude::*;

// One list defines accepted CLI names and their existing string representation.
macro_rules! scripts {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Script { $($variant),+ }

        impl Script {
            pub fn parse(name: &str) -> Result<Self, String> {
                match name {
                    $($name => Ok(Self::$variant),)+
                    _ => Err(format!("unknown --script {name:?}; expected one of: {}", [$($name),+].join(", "))),
                }
            }
        }

        impl std::ops::Deref for Script {
            type Target = str;
            fn deref(&self) -> &str {
                match self { $(Self::$variant => $name),+ }
            }
        }

        #[cfg(test)]
        #[test]
        fn script_names_round_trip() {
            $(assert_eq!(Script::parse($name).unwrap(), Script::$variant);
              assert_eq!(&*Script::$variant, $name);)+
            assert!(Script::parse("dock-clik").is_err());
            assert!(Script::parse("lifeboat-typo").is_err());
            assert!(Script::parse("").is_err());
        }
    };
}

scripts! {
    Asteroid => "asteroid", Board => "board", BriefingSkip => "briefing-skip",
    Course => "course", CurrentM3 => "current-m3", Damage => "damage", Defeat => "defeat",
    Dock => "dock", DockClick => "dock-click", DockMenu => "dock-menu",
    Fleet => "fleet", FleetPrimary => "fleet-primary", FleetCrew => "fleet-crew", FleetArms => "fleet-arms", FleetProfile => "fleet-profile", FleetProfileCrew => "fleet-profile-crew",
    FxTest => "fxtest", HuntM3 => "hunt-m3",
    Ingame => "ingame", IngameLoad => "ingame-load", IngameDoLoad => "ingame-do-load",
    IngameSave => "ingame-save", IngameOptions => "ingame-options", IngameControls => "ingame-controls",
    LifeboatM1 => "lifeboat-m1", LifeboatM1Fallback => "lifeboat-m1-fallback",
    LifeboatM2 => "lifeboat-m2", LifeboatM2Ladyj => "lifeboat-m2-ladyj",
    LifeboatM4 => "lifeboat-m4", LifeboatM9 => "lifeboat-m9", LifeboatKill => "lifeboat-kill",
    MapInfo => "map-info", MenuSelected => "menu-selected", MenuFriendly => "menu-friendly", MenuEnemy => "menu-enemy",
    Mission => "mission", Pause => "pause", PauseRestart => "pause-restart", Quick => "quick",
    Restart => "restart", Shells => "shells", Sight => "sight", Speedtest => "speedtest",
    StormM3 => "storm-m3", StormM3Damage => "storm-m3-damage", Talk => "talk", Target => "target", Victory => "victory",
}

pub fn plugin(app: &mut App) {
    report::plugin(app);
    app.add_systems(PreUpdate, fleet::fleet_smoke.after(bevy::ui::UiSystems::Focus).before(crate::panels::hud_input))
        .add_systems(PreUpdate, lifeboats::smoke.after(bevy::ui::UiSystems::Focus))
        .add_systems(Update, shells::shells);
}

/// Jump to the tender and damage the player to exercise repair; called at the original point in rule dispatch.
pub(crate) fn dock(cfg: &Config, mission: &mut Mission, skipped: &mut bool, ships: &mut Query<RuleShip>) {
    if !matches!(cfg.script, Some(Script::Dock | Script::DockClick | Script::DockMenu)) || mission.opening.is_some() || *skipped {
        return;
    }
    *skipped = true;
    mission.script.skip_to_tender();
    let tender = ships.iter().find(|s| s.mission.name == "RLS Bounty").map(|s| s.transform.translation).expect("mission 1 tender");
    let mut k = 0.0;
    for mut s in ships.iter_mut() {
        if s.primary {
            s.transform.translation = tender + Vec3::new(-100.0, 0.0, 0.0);
            s.health.scale(0.5);
        } else if s.mission.owner == "Small Ships (Red Team)" {
            k += 1.0;
            s.transform.translation = tender + Vec3::new(-170.0, 0.0, 45.0 * k - 110.0);
        } else {
            continue;
        }
        s.ship.waypoints.clear();
        s.ship.order = 0;
        s.ship.speed = 0.0;
    }
}

/// State of the PROCYON_*_SHOT hooks.
#[derive(Default)]
pub struct ShotHooks {
    nis_age: f32,
    nis_shot: bool,
    end_age: f32,
    end_shot: bool,
}

/// PROCYON_END_SHOT=<png> saves a screenshot 3 s after the mission ends and quits; PROCYON_NIS_SHOT=<png> saves one 4 s into the
/// third script NIS (rule time > 400 s) and quits shortly after.
pub fn screenshot_hooks(mut commands: Commands, time: Res<Time>, mission: Res<Mission>, mut hooks: Local<ShotHooks>) {
    static ENV: std::sync::OnceLock<(Option<std::ffi::OsString>, Option<std::ffi::OsString>)> = std::sync::OnceLock::new();
    let (end_path, nis_path) = ENV.get_or_init(|| (std::env::var_os("PROCYON_END_SHOT"), std::env::var_os("PROCYON_NIS_SHOT")));
    let dt = time.delta_secs();
    let shoot = |commands: &mut Commands, path: &std::ffi::OsString| {
        use bevy::render::view::screenshot::{Screenshot, save_to_disk};
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(std::path::PathBuf::from(path)));
    };
    if let (Some(path), Some(_)) = (end_path, mission.script.ended.as_ref()) {
        hooks.end_age += dt;
        if hooks.end_age > 3.0 && !hooks.end_shot {
            hooks.end_shot = true;
            shoot(&mut commands, path);
        } else if hooks.end_age > 8.0 {
            std::process::exit(0);
        }
    }
    if let Some(path) = nis_path {
        if mission.script.nis && mission.opening.is_none() {
            hooks.nis_age += dt;
        } else {
            hooks.nis_age = 0.0;
        }
        if hooks.nis_age > 4.0 && !hooks.nis_shot && mission.script.time > 400.0 {
            hooks.nis_shot = true;
            shoot(&mut commands, path);
        } else if hooks.nis_shot {
            hooks.nis_age += dt;
            if hooks.nis_age > 12.0 {
                std::process::exit(0);
            }
        }
    }
}

/// Game seconds a single NIS may run, or the player's ship stay within `STUCK_RANGE` units of one spot, before autoplay calls it stuck.
const STUCK_NIS: f32 = 300.0;
const STUCK_RANGE: f32 = 50.0;

/// PROCYON_AUTOPLAY=<game seconds> (with `--script mission`, see tools/autoplay.py): when the mission ends, an NIS runs past
/// `STUCK_NIS`, the player's ship stops getting anywhere or the time limit passes, log one `autoplay end` line (outcome and the rules that never fired) and quit.
pub fn autoplay_end(
    time: Res<Time>,
    mission: Res<Mission>,
    player: Query<&Transform, With<crate::PlayerShip>>,
    mut nis: Local<f32>,
    mut since_end: Local<f32>,
    mut still: Local<(Vec3, f32)>,
) {
    static LIMIT: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
    let Some(limit) = *LIMIT.get_or_init(|| std::env::var("PROCYON_AUTOPLAY").ok().and_then(|v| v.parse().ok())) else { return };
    let sc = &mission.script;
    *nis = if sc.nis && mission.opening.is_none() { *nis + time.delta_secs() } else { 0.0 };
    // The autopilot has no path planning: a ship pinned on a coast stays within `STUCK_RANGE` of one spot.
    if let Ok(tf) = player.single() {
        if tf.translation.distance(still.0) > STUCK_RANGE || sc.nis {
            *still = (tf.translation, 0.0);
        }
        still.1 += time.delta_secs();
    }
    if sc.ended.is_some() {
        *since_end += time.delta_secs(); // let the end rules (score, speech) run before reporting
    }
    let outcome = match &sc.ended {
        Some((_, true)) if *since_end > 3.0 => "won",
        Some((_, false)) if *since_end > 3.0 => "lost",
        _ if *nis > STUCK_NIS => "stuck in NIS",
        _ if still.1 > STUCK_NIS => "player stuck",
        _ if sc.time > limit => "time limit",
        _ => return,
    };
    info!("autoplay end outcome={outcome:?} time={:.0} never_fired={:?}", sc.time, sc.never_fired());
    std::process::exit(0);
}
