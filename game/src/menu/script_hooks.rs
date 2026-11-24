//! Test-script hooks of the front end (`--menu <screen> --script <name>`): they press buttons and fill choices so smoke runs and
//! screenshots reach a screen's state without a player. They are not part of the menu's behaviour; `menu.rs` only registers them.

use bevy::prelude::*;

use super::{Action, BakeJob, BuildMenu, Kind, ModList, RunButtons, Screen, Skirmish, SkyBake, apply};
use crate::combat::Library;
use crate::scenario::Race;
use crate::smoke::Script;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, fleet_hook.run_if(in_state(Screen::Fleet)))
        .add_systems(Update, mods_bake_hook.run_if(in_state(Screen::Mods)))
        .add_systems(Update, missions_play_hook.after(BuildMenu).before(RunButtons).run_if(in_state(Screen::Missions)))
        .add_systems(Update, map_info_hook.before(super::autolaunch).run_if(in_state(Screen::Lobby)));
}

/// Screenshot hook (`--menu lobby --script map-info`): opens the Map Description dialog.
fn map_info_hook(mut sk: ResMut<Skirmish>, cfg: Res<crate::Config>) {
    if !sk.map_info && cfg.script == Some(Script::MapInfo) {
        sk.map_info = true;
    }
}

/// Smoke hook (`--menu mods --script mods-bake`): picks the Remaster look and presses "Bake HD skies", so a screenshot shows the
/// bake running, then presses Stop after the late shot (`_late.png`) if it still runs, so the run also checks that stopping
/// ends the baker; a late frame past the bake's end shows it finished instead.
/// It bakes into the real user folder: point XDG_DATA_HOME at a scratch folder to keep a player's own skies.
fn mods_bake_hook(mut mods: ResMut<ModList>, mut job: ResMut<BakeJob>, cfg: Res<crate::Config>, mut pressed: Local<u8>) {
    if cfg.script != Some(crate::smoke::Script::ModsBake) {
        return;
    }
    let press = match *pressed {
        0 => true,
        1 => cfg.frame > cfg.late_frame + 20 && matches!(mods.bake, SkyBake::Running { .. }),
        _ => false,
    };
    if press {
        *pressed += 1;
        mods.picked = mods.mods.iter().position(|m| m.id == "remaster");
        mods.bake_or_stop(&mut job);
    }
}

/// Smoke hook (`--menu missions --script missions-play`): presses the Custom Missions screen's Play button once it is drawn, so the
/// run starts the first custom mission the way a click does (needs a mission mod switched on or passed with `--mod`).
#[allow(deprecated)] // Interaction, see pointer.rs
fn missions_play_hook(cfg: Res<crate::Config>, mut buttons: Query<(&mut Interaction, &Action)>, mut done: Local<bool>) {
    if *done || cfg.script != Some(crate::smoke::Script::MissionsPlay) || cfg.frame < 30 {
        return;
    }
    for (mut i, a) in &mut buttons {
        if matches!(a, Action::PlayMission(_)) {
            *i = Interaction::Pressed;
            *done = true;
        }
    }
}

/// Screenshot hook (`--menu fleet --script fleet-crew|fleet-arms`): an auto-assigned Navy fleet with the Crew / Arms screen open;
/// `fleet-profile` / `fleet-profile-crew` open the Save Fleet / Save Crew dialog on it, typed with a name.
fn fleet_hook(mut sk: ResMut<Skirmish>, lib: Res<Library>, cfg: Res<crate::Config>, mut done: Local<bool>) {
    use crate::smoke::Script as S;
    if *done || !matches!(cfg.script, Some(S::FleetCrew | S::FleetArms | S::FleetProfile | S::FleetProfileCrew)) {
        return;
    }
    *done = true;
    if let Some(i) = sk.slots.iter().position(|s| s.kind == Kind::Human) {
        sk.slots[i].race = Race::Navy;
    }
    sk.fleet_size = 5;
    apply(&mut sk, &lib, &Action::AutoAssign);
    sk.arms_open = cfg.script != Some(S::FleetProfile);
    sk.arms_tab = cfg.script == Some(S::FleetArms);
    match cfg.script {
        Some(S::FleetProfile) => sk.open_profile(crate::profiles::Kind::Fleet, true, &lib),
        Some(S::FleetProfileCrew) => sk.open_profile(crate::profiles::Kind::Crew, true, &lib),
        _ => {}
    }
    if let Some(d) = sk.profile.as_mut() {
        d.name = "Strike fleet".into();
    }
    // The fleet profile is saved, then listed in the Load dialog with its points.
    if cfg.script == Some(S::FleetProfile) {
        sk.do_profile(&lib);
        sk.open_profile(crate::profiles::Kind::Fleet, false, &lib);
        sk.pick_profile(0);
    }
}
