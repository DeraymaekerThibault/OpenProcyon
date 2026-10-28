use crate::campaign::{Mission, MissionShip};
use crate::lifeboats::{Lifeboat, class_for};
use crate::{Health, Library, Ship, Side};
use bevy::prelude::*;

/// ECS smoke scenarios for `--script lifeboat-m1|lifeboat-m1-fallback|lifeboat-m2|lifeboat-m2-ladyj|lifeboat-m4|lifeboat-m9|lifeboat-kill` (needs `--shot`, which counts frames;
/// docs/re/lifeboats.md §7.10). SCRIPTED: it kills the pirate through its health (the combat-death path, not a fight), teleports the
/// collector onto a boat and, for mission 1, marks the NIS speech as played; everything else is the normal rules and systems.
/// Normal-combat validation is a manual play-through. Panics (non-zero exit) when an expectation fails.
pub(super) fn smoke(world: &mut World, mut stage: Local<(u32, u32)>, mut score: Local<u32>, mut expect: Local<Option<(String, String, u32)>>) {
    let cfg = world.resource::<crate::Config>();
    let Some(script) = cfg.script.as_deref().filter(|s| s.starts_with("lifeboat-")).map(str::to_owned) else { return };
    let (frame, shot) = (cfg.frame, cfg.shot.is_some());
    assert!(shot, "{script} counts frames: run it with --shot");
    if !world.contains_resource::<Mission>() || frame < 90 {
        return;
    }
    let pick = |w: &mut World, name: &str| w.query::<(Entity, &MissionShip)>().iter(w).find(|(_, m)| m.name == name).map_or_else(|| panic!("no ship {name}"), |(e, _)| e);
    if script == "lifeboat-m4" || script == "lifeboat-m9" {
        // Placed boats: mission 4 has three real ones (class flag) that nothing collects at the start; mission 9's longboat is a combat ship.
        if frame == 200 {
            let found: Vec<_> = world.query::<(&Lifeboat, &Ship, &MissionShip)>().iter(world).map(|(l, s, m)| (l.owner.clone(), s.side, m.name.clone())).collect();
            if script == "lifeboat-m4" {
                assert_eq!(found.len(), 3, "mission 4 pre-placed lifeboats survive their first seconds");
                assert!(found.iter().all(|f| f.0 == "navy" && f.1 == Side::Player));
                assert_eq!(world.resource::<Mission>().script.lifeboats_saved_by("tourist", "navy"), 0);
            } else {
                assert!(found.is_empty());
                pick(world, "Longboat");
            }
            info!("{script}: PASS");
            std::process::exit(0);
        }
        return;
    }
    if script == "lifeboat-kill" {
        // Any mission: the first flagged ship that leaves boats is killed; its boats must match its race, count and owner.
        let classes: Vec<(String, String, u32)> = world.resource::<Library>().classes.iter().map(|c| (c.name.clone(), c.race.clone(), c.lifeboats)).collect();
        if frame == 200 {
            let (e, class, owner) = world
                .query::<(Entity, &Ship, &MissionShip, &Health)>()
                .iter(world)
                .find(|(_, s, m, h)| m.create_lifeboats && classes[s.class].2 > 0 && !h.destroyed)
                .map_or_else(|| panic!("{script}: no flagged enemy ship that leaves boats"), |(e, s, m, _)| (e, &classes[s.class], m.owner.clone()));
            *expect = Some((class_for(&class.1), owner.clone(), class.2));
            info!("{script}: killing {} of {owner}, expecting {} x {}", class.0, class.2, class_for(&class.1));
            world.get_mut::<Health>(e).unwrap().core = 0.0;
        } else if let Some((class, owner, n)) = expect.clone() {
            let found = world.query::<(&Lifeboat, &Ship)>().iter(world).map(|(l, s)| (l.owner.clone(), classes[s.class].0.clone())).collect::<Vec<_>>();
            if found.len() as u32 >= n {
                assert!(found.len() as u32 == n && found.iter().all(|f| f.0 == owner && f.1 == class), "{script}: wrong boats {found:?}");
                info!("{script}: PASS");
                std::process::exit(0);
            }
        }
        return;
    }
    let (m1, ladyj, fallback) = (script.starts_with("lifeboat-m1"), script.ends_with("ladyj"), script.ends_with("fallback"));
    let flagged = |w: &mut World, name: &str| {
        let e = pick(w, name);
        w.get::<MissionShip>(e).unwrap().create_lifeboats
    };
    let boats = |w: &mut World| w.query::<(Entity, &Lifeboat, &Transform)>().iter(w).map(|(e, l, t)| (e, l.owner.clone(), t.translation)).collect::<Vec<_>>();
    let (victim, owner) = if m1 { ("Warrior", "Pirate Schooner") } else { ("Ravage", "Pirates") };
    let wait = |s: &mut (u32, u32), ok: bool, what: &str| {
        s.1 += 1;
        assert!(s.1 < 3000, "{script}: timed out waiting for {what}");
        if ok {
            *s = (s.0 + 1, 0);
        }
        ok
    };
    match stage.0 {
        0 => {
            // Rules named the ships at world init (not the whole fleet).
            let names: &[(&str, bool)] = if m1 {
                &[("Warrior", true), ("RLS Providence", true), ("RLS Explorer", true), ("RLS Sovereign", false), ("RLS Bounty", false)]
            } else {
                &[("Ravage", true), ("SS Lady J", true), ("SS Coventry", true), ("SS Denman", false)]
            };
            for (n, want) in names {
                assert_eq!(flagged(world, n), *want, "Create Lifeboats flag of {n}");
            }
            assert!(boats(world).is_empty());
            let e = pick(world, victim);
            // The mission starts the player's ship beside the victim, which would collect the boats at once: move it away.
            let at = world.get::<Transform>(e).unwrap().translation;
            let player = world.query_filtered::<Entity, With<crate::PlayerShip>>().single(world).unwrap();
            world.get_mut::<Transform>(player).unwrap().translation = at + Vec3::new(-400.0, 0.0, 400.0);
            world.get_mut::<Health>(e).unwrap().core = 0.0; // `combat::deaths` then reports the loss
            info!("{script}: {victim} dies");
            *stage = (1, 0);
        }
        1 => {
            let b = boats(world);
            if wait(&mut stage, b.len() == 2, "two lifeboats") {
                assert!(b.iter().all(|x| x.1 == owner));
                let lib = world.resource::<Library>();
                let class = |e: Entity| lib.classes[world.get::<Ship>(e).unwrap().class].name.clone();
                assert!(b.iter().all(|x| class(x.0) == "Ship_Pirate_Lifeboat" && world.get::<crate::boarding::Boardable>(x.0).is_none()));
                if m1 {
                    world.resource_mut::<Mission>().script.played.insert("M01AMELIA17".into()); // scripted: skip the NIS speech
                }
                info!("{script}: boats spawned");
            }
        }
        2 => {
            // Boats exist: the rules see them, the exit does not fire, the objective shows.
            let sc = &world.resource::<Mission>().script;
            assert_eq!(sc.lifeboat_owners().len(), 2);
            assert!(sc.ended.is_none(), "no exit while boats are afloat");
            let objective = if m1 { "Collect Pirate Lifeboat" } else { "PickUpLifeboats" };
            let ready = sc.task_active(objective) && (m1 || sc.played.contains("M02ONYX18"));
            wait(&mut stage, ready, objective);
        }
        3 => {
            let before = world.resource::<Mission>().script.score;
            if fallback {
                // Nobody collects: the retail timers end the mission ~100 s after the kill (55 s + 45 s).
                let ended = world.resource::<Mission>().script.ended.clone();
                if wait(&mut stage, ended.is_some(), "the fallback exit") {
                    assert_eq!(world.resource::<Mission>().script.lifeboat_owners().len(), 2, "uncollected boats stay until the level changes");
                    info!("{script}: PASS (fallback exit)");
                    std::process::exit(0);
                }
                return;
            }
            let collector = if ladyj { pick(world, "SS Lady J") } else { world.query_filtered::<Entity, With<crate::PlayerShip>>().single(world).unwrap() };
            let at = boats(world)[0].2;
            world.get_mut::<Transform>(collector).unwrap().translation = at;
            world.get_mut::<Ship>(collector).unwrap().speed = 0.0;
            *score = before;
            *stage = (4, 0);
        }
        4 => {
            // Both boats of one wreck sit within a few units of each other, so one visit usually collects both.
            let b = boats(world);
            if wait(&mut stage, b.len() < 2, "the first pickup") {
                let sc = &world.resource::<Mission>().script;
                let (who, per_boat) = if ladyj {
                    ("Schooner2", 0)
                } else if m1 {
                    ("Player0 Group", 2)
                } else {
                    ("PlayerStart", 2)
                };
                let got = sc.lifeboats_saved_by(who, owner);
                assert_eq!(got as usize, 2 - b.len());
                assert_eq!(sc.score, *score + per_boat * got, "+2 per boat only for the local player collecting an enemy boat");
                if m1 {
                    assert!(sc.ended.is_none() || b.is_empty());
                } else {
                    assert!(sc.flag("LifeboatsPickedUp") || sc.ended.is_none(), "mission 2 reacts to the first pickup");
                }
                info!("{script}: {got} boat(s) collected");
                if let Some(rest) = b.first() {
                    let collector = world.query_filtered::<Entity, With<crate::PlayerShip>>().single(world).unwrap();
                    world.get_mut::<Transform>(collector).unwrap().translation = rest.2;
                }
            }
        }
        5 => {
            let ended = world.resource::<Mission>().script.ended.clone();
            if wait(&mut stage, ended.is_some(), "the next-level exit") {
                assert!(ended.unwrap().1, "mission won");
                let left = boats(world).len();
                let sc = &world.resource::<Mission>().script;
                if m1 {
                    assert_eq!((sc.lifeboats_saved_by("Player0 Group", owner), left), (2, 0));
                } else {
                    let got = sc.lifeboats_saved_by(if ladyj { "Schooner2" } else { "PlayerStart" }, owner) as usize;
                    assert_eq!(left + got, 2);
                    assert_eq!((sc.task_complete("PickUpLifeboats"), sc.task_active("PickUpLifeboats")), (!ladyj, false));
                }
                info!("{script}: PASS");
                std::process::exit(0);
            }
        }
        _ => {}
    }
}
