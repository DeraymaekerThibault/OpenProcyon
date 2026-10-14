//! Read-only ship status derived from guns and sections (HUD banks, icons, damage speed/turn penalties).
use super::*;

/// Exe (Ship update): loss = destroyed / total sections carrying the flag, capped at 0.9; ShipPhysics multiplies
/// max speed and thrust by (1 - loss) for vitalToMaxVelocity and the rudder torque by (1 - loss) for vitalToManeuverability.
pub fn loss(c: &Class, h: &Health, flag: fn(&Section) -> bool) -> f32 {
    let total = c.sections.iter().filter(|s| flag(s)).count();
    if total == 0 {
        return 0.0;
    }
    let lost = c.sections.iter().zip(&h.sections).filter(|(s, hp)| flag(s) && **hp <= 0.0).count();
    (lost as f32 / total as f32).min(0.9)
}

pub fn speed_factor(c: &Class, h: &Health) -> f32 {
    1.0 - loss(c, h, |s| s.speed)
}

pub fn turn_factor(c: &Class, h: &Health) -> f32 {
    1.0 - loss(c, h, |s| s.turn)
}

pub fn is_indirect(gun: &str) -> bool {
    gun.contains("Mortar") || gun.contains("Mine") || gun.contains("GravCharge")
}

/// Weapon-panel banks as (bank number, first mount); point-defence guns are not shown.
pub fn banks(lib: &Library, c: &Class) -> Vec<(usize, usize)> {
    let mut v: Vec<(usize, usize)> = vec![];
    for (i, m) in c.mounts.iter().enumerate() {
        if !lib.guns[&m.gun].point_defense && !v.iter().any(|b| b.0 == m.bank) {
            v.push((m.bank, i));
        }
    }
    v
}

/// Panel frame of a bank: 0 ready and in range / 1 recharging / 2 destroyed / 3 ready, nothing in range.
pub fn bank_state(g: &GunStates, c: &Class, h: &Health, bank: usize) -> usize {
    let live: Vec<&GunState> = c.mounts.iter().zip(&g.0).filter(|(m, _)| m.bank == bank && m.parent.is_none_or(|p| h.sections[p] > 0.0)).map(|(_, s)| s).collect();
    if live.is_empty() {
        2
    } else if live.iter().any(|s| s.cooldown > 0.0) {
        1
    } else if live.iter().any(|s| s.in_range) {
        0
    } else {
        3
    }
}

/// Recharge progress of a bank for the panel's bar: 0 just fired .. 1 loaded (the slowest live gun decides; 0 when no gun is left).
pub fn bank_recharge(lib: &Library, g: &GunStates, c: &Class, h: &Health, bank: usize) -> f32 {
    c.mounts
        .iter()
        .zip(&g.0)
        .filter(|(m, _)| m.bank == bank && m.parent.is_none_or(|p| h.sections[p] > 0.0))
        .map(|(_, s)| if s.cooldown <= 0.0 { 1.0 } else { 1.0 - s.cooldown / lib.guns[&s.gun].reload.max(0.01) })
        .reduce(f32::min)
        .unwrap_or(0.0)
        .clamp(0.0, 1.0)
}

/// HUD icon name for a gun definition name (e.g. Gun_CannonBall_Med -> WeaponBar_GunIcon_Cannon_Medium).
pub fn gun_icon(g: &str) -> String {
    let size = if g.contains("Heavy") {
        "Heavy"
    } else if g.contains("Light") {
        "Light"
    } else {
        "Medium"
    };
    let dual = if g.contains("Dual") { "Dual_" } else { "" };
    let kind = match g {
        _ if g.contains("Harpoon") => "Harpoon".into(),
        _ if g.contains("GravCharge") => "GravityCharge".into(),
        _ if g.contains("EnergyNet") => "EnergyNetLauncher".into(),
        _ if g.contains("Gattling") => "GattlingGun".into(),
        _ if g.contains("ArcturianFire") => "FireLauncher".into(),
        _ if g.contains("NovaMortar") => "NovaMortar".into(),
        _ if g.contains("StarMortar") => "StarMortar".into(),
        _ if g.contains("StarMine") => "StarMine".into(),
        _ if g.contains("Torpedo") => "Torpedo".into(),
        _ if g.contains("Lancer") => format!("Lancer_{dual}{size}"),
        _ if g.contains("Plasma") => format!("PlasmaGun_{dual}{size}"),
        _ if g.contains("Carronade") => format!("Carronaid_{}", if size == "Medium" { "Mediuam" } else { size }), // sic: retail typo
        _ => format!("Cannon_{size}"),
    };
    format!("WeaponBar_GunIcon_{kind}")
}

pub fn guns_ready(g: &GunStates, c: &Class, h: &Health) -> (usize, usize) {
    let live = c.mounts.iter().enumerate().filter(|(_, m)| m.parent.is_none_or(|p| h.sections[p] > 0.0)).map(|(i, _)| i);
    let (mut ready, mut total) = (0, 0);
    for i in live {
        total += 1;
        if g.0[i].cooldown <= 0.0 {
            ready += 1;
        }
    }
    (ready, total)
}

pub fn section_lines(c: &Class, h: &Health) -> String {
    c.sections.iter().zip(&h.sections).map(|(s, hp)| format!("{:<22} {:>3.0}/{:.0}\n", s.label, hp, s.hp)).collect()
}
