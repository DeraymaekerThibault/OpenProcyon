//! Scripted wiring checks; these are not substitutes for manual gameplay validation.
mod fleet;
mod lifeboats;
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
    Fleet => "fleet", FleetPrimary => "fleet-primary", FleetCrew => "fleet-crew", FleetArms => "fleet-arms",
    FxTest => "fxtest", HuntM3 => "hunt-m3",
    Ingame => "ingame", IngameLoad => "ingame-load", IngameDoLoad => "ingame-do-load",
    IngameSave => "ingame-save", IngameOptions => "ingame-options", IngameControls => "ingame-controls",
    LifeboatM1 => "lifeboat-m1", LifeboatM1Fallback => "lifeboat-m1-fallback",
    LifeboatM2 => "lifeboat-m2", LifeboatM2Ladyj => "lifeboat-m2-ladyj",
    LifeboatM4 => "lifeboat-m4", LifeboatM9 => "lifeboat-m9", LifeboatKill => "lifeboat-kill",
    MapInfo => "map-info", MenuSelected => "menu-selected", MenuFriendly => "menu-friendly", MenuEnemy => "menu-enemy",
    Mission => "mission", Pause => "pause", PauseRestart => "pause-restart", Quick => "quick",
    Restart => "restart", Sight => "sight", Speedtest => "speedtest",
    StormM3 => "storm-m3", StormM3Damage => "storm-m3-damage", Talk => "talk", Target => "target", Victory => "victory",
}

pub fn plugin(app: &mut App) {
    app.add_systems(PreUpdate, fleet::fleet_smoke.after(bevy::ui::UiSystems::Focus).before(crate::panels::hud_input))
        .add_systems(PreUpdate, lifeboats::smoke.after(bevy::ui::UiSystems::Focus));
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
