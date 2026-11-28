//! PROCYON_TRACE=<file.csv>: log every ship and shell of a battle once per simulation step, for comparing a run against
//! measurements of the original laid out the same way. Game axes (x starboard, y forward, z up).
//! - `<file>.csv`: gt, ship, x, y, vx, vy, fx, fy, hp (section hit points joined by '/'); `ship` is the ship's creation number
//!   (`scenario::SpawnIndex`) and indexes `<file>.ships.json`.
//! - `<file>.shells.csv`: gt, id, class, x, y, z, vx, vy, vz; `id` is the shell's creation number; its last row is where it hit or died.
//!
//! Rows go in creation order. `gt` starts again from 0 when a battle starts (a restart), so a restarted battle lines up with the first.
use bevy::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

use crate::Ship;
use crate::combat::{Health, Shell};

#[derive(Resource)]
struct Trace {
    base: String,
    ships: File,
    shells: File,
    time: f32,
    names: HashMap<u32, String>,
}

pub fn plugin(app: &mut App) {
    let Ok(path) = std::env::var("PROCYON_TRACE") else { return };
    let base = path.strip_suffix(".csv").unwrap_or(&path).to_string();
    let open = |name: String, header: &str| {
        let mut f = File::create(&name).unwrap_or_else(|e| panic!("PROCYON_TRACE: cannot write {name}: {e}"));
        writeln!(f, "{header}").unwrap();
        f
    };
    let ships = open(format!("{base}.csv"), "gt,ship,x,y,vx,vy,fx,fy,hp");
    let shells = open(format!("{base}.shells.csv"), "gt,id,class,x,y,z,vx,vy,vz");
    app.insert_resource(Trace { base, ships, shells, time: 0.0, names: HashMap::new() })
        .add_systems(FixedUpdate, write.in_set(crate::battle::Step::Report).run_if(in_state(crate::menu::Screen::Battle)));
}

/// Bevy (x, y up, -z forward) to game axes (x, y forward, z up).
fn game(v: Vec3) -> (f32, f32, f32) {
    (v.x, -v.z, v.y)
}

#[allow(clippy::type_complexity)]
fn write(
    time: Res<Time>,
    lib: Res<crate::Library>,
    mut trace: ResMut<Trace>,
    battle: Res<crate::scenario::Difficulty>,
    ships: Query<(&crate::scenario::SpawnIndex, &Ship, &Transform, &Health)>,
    shells: Query<(&crate::scenario::SpawnIndex, &Shell, &Transform)>,
) {
    // `Difficulty` is inserted at every battle start (`scenario::reset_sim`) and never written otherwise.
    if battle.is_changed() {
        trace.time = 0.0;
    }
    trace.time += time.delta_secs();
    let gt = trace.time;
    let (mut a, mut b) = (String::new(), String::new());
    let mut ships: Vec<_> = ships.iter().filter(|s| !s.3.destroyed).collect();
    ships.sort_by_key(|s| *s.0);
    for (i, ship, tf, health) in ships {
        if let std::collections::hash_map::Entry::Vacant(v) = trace.names.entry(i.0) {
            v.insert(lib.classes[ship.class].name.clone());
            let names: serde_json::Map<_, _> = trace.names.iter().map(|(i, n)| (i.to_string(), n.clone().into())).collect();
            std::fs::write(format!("{}.ships.json", trace.base), serde_json::Value::Object(names).to_string()).unwrap();
        }
        let (p, f) = (game(tf.translation), game(*tf.forward()));
        let hp: Vec<String> = health.sections.iter().map(|h| format!("{:.0}", h.max(0.0))).collect();
        a += &format!("{gt:.3},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{}\n", i.0, p.0, p.1, f.0 * ship.speed, f.1 * ship.speed, f.0, f.1, hp.join("/"));
    }
    let mut shells: Vec<_> = shells.iter().collect();
    shells.sort_by_key(|s| *s.0);
    for (i, shell, tf) in shells {
        let (class, vel) = shell.class_and_velocity();
        let (p, v) = (game(tf.translation), game(vel));
        b += &format!("{gt:.3},{},{},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3}\n", i.0, class.trim_start_matches("Bullet_"), p.0, p.1, p.2, v.0, v.1, v.2);
    }
    trace.ships.write_all(a.as_bytes()).unwrap();
    trace.shells.write_all(b.as_bytes()).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_axes() {
        // A ship at heading 0 faces Bevy -Z, which is the game's +y (forward); Bevy's up is the game's z.
        assert_eq!(game(Vec3::NEG_Z), (0.0, 1.0, 0.0));
        assert_eq!(game(Vec3::new(3.0, 7.0, -5.0)), (3.0, 5.0, 7.0));
    }
}
