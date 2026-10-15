use crate::*;

/// ECS/UI self-check through shared keys and widget markers, usable with either presentation.
#[allow(deprecated)] // Same shared Interaction markers as panels.rs; test driver only.
pub fn fleet_smoke(world: &mut World, mut previous: Local<Option<Entity>>) {
    let cfg = world.resource::<Config>();
    let frame = cfg.frame;
    if cfg.script.as_deref() == Some("current-m3") {
        if frame == 10 {
            let mut m = world.resource_mut::<campaign::Mission>();
            let path = m.data["paths"]["Etherium Current"].as_array().unwrap();
            let pos = game_to_bevy(&path[path.len() / 2]);
            let mut data = m.data.clone();
            data["rules"] = serde_json::json!([]);
            m.script = rules::Script::new(&data, serde_json::json!({}));
            let e = world.resource::<ship::Fleets>().primary().unwrap();
            world.get_mut::<Transform>(e).unwrap().translation = pos;
            world.resource_mut::<Orbit>().dist = 450.0;
        }
        if frame == 30 {
            let currents = world.query_filtered::<Entity, With<crate::environment::Current>>().iter(world).collect::<Vec<_>>();
            assert_eq!(currents.len(), 1);
            let path = world.resource::<campaign::Mission>().data["paths"]["Etherium Current"].as_array().unwrap();
            let (a, b) = (game_to_bevy(&path[0]), game_to_bevy(&path[1]));
            let force = world.get::<crate::environment::Current>(currents[0]).unwrap().force(a.midpoint(b));
            assert!(force.dot(a - b) > 0.0, "Mission 3 current pushes toward the first path point");
            let children = world.get::<Children>(currents[0]).unwrap();
            assert_eq!(children.len(), 2);
            for child in children.iter() {
                let h = world.get::<MeshMaterial3d<crate::fx::EffectMaterial>>(child).unwrap();
                let material = world.resource::<Assets<crate::fx::EffectMaterial>>().get(&h.0).unwrap();
                assert!(material.base.uv_transform.translation.y < 0.0);
            }
            info!("mission 3 current ribbon layers and flowing UVs ECS smoke passed");
        }
        return;
    }
    if cfg.script.as_deref() == Some("hunt-m3") {
        if frame < 10 {
            return;
        }
        let primary = world.resource::<ship::Fleets>().primary().unwrap();
        let exile = world.query::<(Entity, &campaign::MissionShip)>().iter(world).find(|(_, m)| m.id == 32).unwrap().0;
        if frame == 10 {
            let mut m = world.resource_mut::<campaign::Mission>();
            let mut data = m.data.clone();
            data["rules"].as_array_mut().unwrap().retain(|r| {
                matches!(
                    r["name"].as_str(),
                    Some("END NIS 2" | "Pirate Sloop Spotted" | "Stop Looking At Sloop" | "Sloop triggers hunt" | "Pirates On Intercept Course" | "Player Needs Punishment")
                )
            });
            // This isolated smoke skips NIS 4, which normally sets the normal-route flag.
            data["rules"].as_array_mut().unwrap().iter_mut().find(|r| r["name"] == "END NIS 2").unwrap()["actions"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({"Type":"Set Flag Action", "Flag Name":"Player Obeying Mission Flow", "Boolean Value":"TRUE"}));
            m.script = rules::Script::new(&data, serde_json::json!({}));
            m.opening = None;
            world.get_mut::<Transform>(primary).unwrap().translation = Vec3::new(500.0, 0.0, -500.0);
            let mut ship = world.get_mut::<Ship>(primary).unwrap();
            ship.order = 0;
            ship.waypoints.clear();
            ship.speed = 0.0;
        }
        if frame == 30 {
            world.resource_mut::<campaign::Mission>().script.played.insert("NIS 2 D".into());
        }
        if frame == 50 {
            assert!(world.get::<campaign::MissionShip>(exile).unwrap().visible);
            assert!(world.resource::<campaign::Mission>().script.holds_fire("Pirate Silver"), "reveal must not launch the hunt");
            assert!(world.get::<campaign::MissionShip>(exile).unwrap().target.is_none());
        }
        if frame == 60 {
            let start = game_to_bevy(&world.resource::<campaign::Mission>().data["paths"]["Sloop Patrolling"][0]);
            world.get_mut::<Transform>(exile).unwrap().translation = start;
        }
        if frame == 70 {
            assert!(world.resource::<campaign::Mission>().script.holds_fire("Pirate Silver"), "patrolling must not launch the hunt");
            let pos = world.get::<Transform>(exile).unwrap().translation;
            world.get_mut::<Transform>(primary).unwrap().translation = pos - Vec3::X * 100.0;
            let mut ship = world.get_mut::<Ship>(primary).unwrap();
            ship.order = 2;
            ship.speed = 10.0;
        }
        if frame == 80 {
            assert!(world.resource::<campaign::Mission>().script.nis, "spotting starts NIS");
            assert_eq!(world.get::<Ship>(primary).unwrap().speed, 0.0, "helm motion stops during NIS");
            let pirate = world.get::<campaign::MissionShip>(exile).unwrap();
            assert!(pirate.looping.is_none() && pirate.stop_at_end, "Exile retreats instead of patrolling/attacking");
            assert!(pirate.target.is_none(), "Exile must not engage");
        }
        if frame == 90 {
            world.resource_mut::<campaign::Mission>().script.played.insert("1st officer says NO".into());
        }
        if frame == 100 {
            // Isolate return-volume wiring without simulating the entire retreat through scenery.
            world.get_mut::<Transform>(exile).unwrap().translation = Vec3::new(1490.0, 0.0, -1480.0);
        }
        if frame == 110 {
            assert!(
                !world.resource::<campaign::Mission>().script.holds_fire("Pirate Silver"),
                "returning Exile launches the hunt: position={:?} route={:?}",
                world.get::<Transform>(exile).unwrap().translation,
                world.get::<Ship>(exile).unwrap().waypoints
            );
            assert!(world.query::<&campaign::MissionShip>().iter(world).filter(|m| [247, 362, 42, 44].contains(&m.id)).all(|m| m.visible));
            for (_, ship, mission_ship) in world.query::<(&Transform, &Ship, &campaign::MissionShip)>().iter(world).filter(|(_, _, m)| [41, 89].contains(&m.id)) {
                assert_eq!(mission_ship.stance, rules::Stance::Dummy, "intercept ships wait for the chase trigger");
                assert!(ship.waypoints.is_empty() && mission_ship.attack.is_empty());
            }
        }
        if frame == 120 {
            let c = rules::Region::from_json(&world.resource::<campaign::Mission>().data["volumes"]["Player Triggers Chase Group"]).centroid().unwrap();
            let center = Vec3::new(c.x, 0.0, -c.y);
            world.get_mut::<Transform>(primary).unwrap().translation = center;
        }
        if frame == 130 {
            for m in world.query::<&campaign::MissionShip>().iter(world).filter(|m| [41, 89].contains(&m.id)) {
                assert_eq!(m.stance, rules::Stance::Aggressive);
                assert!(!m.attack.is_empty(), "the chase trigger launches the intercept ships");
            }
            let punishers: Vec<_> = world.query::<(&Ship, &campaign::MissionShip)>().iter(world).filter(|(_, m)| [422, 423, 424].contains(&m.id)).collect();
            assert_eq!(punishers.len(), 3);
            for (ship, m) in punishers {
                assert!(!m.visible, "normal mission flow keeps Punishers hidden");
                assert_eq!(m.stance, rules::Stance::Dummy);
                assert!(ship.waypoints.is_empty() && m.attack.is_empty());
            }
            info!("mission 3 reveal, patrol, spotting NIS halt, retreat, fleet hunt, delayed intercept and inactive Punishers ECS smoke passed");
        }
        return;
    }
    if matches!(cfg.script.as_deref(), Some("storm-m3" | "storm-m3-damage")) {
        if frame == 10 {
            let mut m = world.resource_mut::<campaign::Mission>();
            let mut data = m.data.clone();
            data["rules"].as_array_mut().unwrap().retain(|r| matches!(r["name"].as_str(), Some("Solar Storm Path" | "Start NIS 2 " | "NIS 2b")));
            m.script = rules::Script::new(&data, serde_json::json!({}));
            m.opening = None;
            let e = world.resource::<ship::Fleets>().primary().unwrap();
            world.get_mut::<Transform>(e).unwrap().translation = Vec3::new(600.0, 0.0, 1050.0);
            let mut sh = world.get_mut::<Ship>(e).unwrap();
            sh.order = 0;
            sh.speed = 0.0;
            sh.waypoints.clear();
        }
        if frame >= 10 && world.resource::<Config>().script.as_deref() == Some("storm-m3") {
            let e = world.resource::<ship::Fleets>().primary().unwrap();
            world.get_mut::<campaign::MissionShip>(e).unwrap().invuln = 1.0;
        }
        if frame == 30 {
            assert!(world.resource::<campaign::Mission>().script.nebula_trap, "crossing the trigger must start the storm route");
        }
        if frame == 1000 {
            let e = world.resource::<ship::Fleets>().primary().unwrap();
            let pos = world.get::<Transform>(e).unwrap().translation;
            let sc = &world.resource::<campaign::Mission>().script;
            assert!(sc.nis, "the moving storm must trigger NIS 2");
            assert!(!sc.nebula_trap, "the ride must release the ship at its end volume");
            assert!(pos.z < -650.0, "the storm must carry the ship north: {pos:?}");
            let hp = world.get::<Health>(e).unwrap();
            assert!(!hp.destroyed && hp.core > 0.0, "the sloop must survive the storm ride");
            info!("mission 3 moving-storm entry, carry and release ECS smoke passed: {pos:?}; core={} sections={:?}", hp.core, hp.sections);
        }
        return;
    }
    if cfg.script.as_deref() == Some("fleet-primary") {
        let name = match cfg.map.as_deref() {
            Some("mission03") => "PlayerShips,RLS Endurance",
            Some("mission05") => "Jim,RLS Superior",
            _ => "Players Ships,RLS St Roch",
        };
        if frame == 10 {
            let mut m = world.resource_mut::<campaign::Mission>();
            let mut data = m.data.clone();
            if name == "PlayerShips,RLS Endurance" {
                data["rules"].as_array_mut().unwrap().retain(|r| r["name"] == "Setup New Sloop & Essential Ship");
            } else {
                data["rules"] = serde_json::json!([{"name":"Synthetic primary dispatch", "conditions":[{"Type":"World Initialize"}], "actions":[{"Type":"Set Fleet Primary Ship", "Group/Unit Name":name}], "Run Once":true, "Is Active":true}]);
            }
            m.script = rules::Script::new(&data, serde_json::json!({}));
            m.opening = None;
        }
        if frame == 30 {
            let selected = world.resource::<ship::Fleets>().selected.unwrap();
            assert!(world.get::<PlayerShip>(selected).is_some());
            assert_eq!(world.get::<campaign::MissionShip>(selected).unwrap().name, name.split_once(',').unwrap().1);
            assert_eq!(world.query_filtered::<Entity, With<PlayerShip>>().iter(world).count(), 1);
            assert!(!world.query::<&campaign::MissionShip>().iter(world).any(|m| m.id == -1));
            info!("mission primary ECS dispatch passed: {name}");
        }
        return;
    }
    if cfg.script.as_deref() != Some("fleet") {
        return;
    }
    world.resource_mut::<ButtonInput<KeyCode>>().reset_all();
    use crate::panels::FleetButton as B;
    let button = match frame {
        70 | 71 | 195 | 196 | 252 | 253 => Some(B::Commands),
        80 | 81 => Some(B::Formation(ship::Formation::LineAbreast)),
        90 | 91 => Some(B::Formation(ship::Formation::EchelonLeft)),
        100 | 101 => Some(B::Formation(ship::Formation::EchelonRight)),
        110 | 111 => Some(B::Formation(ship::Formation::Column)),
        120 | 121 => Some(B::Formation(ship::Formation::Diamond)),
        130 | 131 | 140 | 141 | 150 | 151 | 200 | 201 | 254 | 255 => Some(B::Formation(ship::Formation::Convoy)),
        160 | 161 => Some(B::Attack),
        164 | 165 | 168 | 169 => Some(B::CeaseFire),
        _ => None,
    };
    if let Some(action) = button {
        let mut found = false;
        for (b, mut i) in world.query::<(&B, &mut Interaction)>().iter_mut(world) {
            if *b == action {
                *i = Interaction::Hovered;
                found = true;
            } else {
                *i = Interaction::None;
            }
        }
        assert!(found, "both presentations must expose fleet widget {action:?}");
        let mut mouse = world.resource_mut::<ButtonInput<MouseButton>>();
        if (frame.is_multiple_of(2) && frame != 196) || frame == 195 {
            mouse.press(MouseButton::Left);
        } else {
            mouse.release(MouseButton::Left);
        }
    }
    if matches!(frame, 202 | 203 | 206 | 207) {
        world.query_filtered::<&mut Window, With<PrimaryWindow>>().single_mut(world).unwrap().set_cursor_position(Some(Vec2::splat(100.0)));
        for (b, mut i) in world.query::<(&B, &mut Interaction)>().iter_mut(world) {
            *i = if *b == B::Slot(0) { Interaction::Hovered } else { Interaction::None };
        }
        if frame.is_multiple_of(2) {
            world.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
        } else {
            world.resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Right);
        }
    }
    let key = match frame {
        30 | 190 | 250 => Some(KeyCode::KeyS),
        40 | 135 => Some(KeyCode::Digit1),
        60 => Some(KeyCode::KeyP),
        180 => Some(KeyCode::KeyR),
        220 => Some(KeyCode::F10),
        _ => None,
    };
    if let Some(key) = key {
        world.resource_mut::<ButtonInput<KeyCode>>().press(key);
    }
    match frame {
        28 => {
            let local = world.resource::<ship::Fleets>().local.clone();
            let ally = world.query::<(Entity, &Ship, &ship::FleetMember)>().iter(world).find(|(_, s, m)| s.side == Side::Player && m.0 != local).map(|s| s.0).unwrap();
            assert!(!ship::promote(world, ally, false));
        }
        204 => assert!(world.resource::<options_menu::OptionsMenu>().is_open(), "bar right click must open the shared object menu"),
        208 => assert!(!world.resource::<options_menu::OptionsMenu>().is_open(), "repeat bar right click must toggle the same object menu closed"),
        205 | 245 => {
            let mut q = world.query_filtered::<&mut Window, With<bevy::window::PrimaryWindow>>();
            let mut window = q.single_mut(world).unwrap();
            if frame == 205 {
                window.resolution.set(1024.0, 600.0);
            } else {
                window.resolution.set(1280.0, 720.0);
            }
        }
        42 => {
            let f = world.resource::<ship::Fleets>();
            assert!(f.selected != f.primary());
            *previous = f.primary();
            assert!(world.get::<PlayerShip>(previous.unwrap()).is_some());
        }
        43 => {
            let actor = world.resource::<ship::Fleets>().selected.unwrap();
            let at = world.get::<Transform>(actor).unwrap().translation;
            let enemy = world.query::<(Entity, &Ship)>().iter(world).find(|(_, s)| s.side == Side::Enemy).map(|s| s.0).unwrap();
            world.get_mut::<Transform>(enemy).unwrap().translation = at + Vec3::new(60.0, 25.0, -90.0);
        }
        44 | 52 => {
            let actor = world.resource::<ship::Fleets>().selected.unwrap();
            let point = if frame == 44 {
                world
                    .query::<(&Transform, &Ship)>()
                    .iter(world)
                    .find(|(_, s)| s.side == Side::Enemy)
                    .map(|(t, s)| t.transform_point(world.resource::<Library>().classes[s.class].centre))
                    .unwrap()
            } else {
                world.get::<Transform>(actor).unwrap().translation + Vec3::new(-120.0, 0.0, -150.0)
            };
            let cursor = {
                let mut q = world.query::<(&Camera, &GlobalTransform)>();
                let (c, t) = q.single(world).unwrap();
                c.world_to_viewport(t, point).unwrap()
            };
            world.query_filtered::<&mut Window, With<bevy::window::PrimaryWindow>>().single_mut(world).unwrap().set_cursor_position(Some(cursor));
        }
        45 | 53 => {
            world.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        }
        46 | 54 => {
            world.resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        }
        47 => {
            let actor = world.resource::<ship::Fleets>().selected.unwrap();
            let target = world.get::<ship::FleetPilot>(actor).unwrap().target.expect("secondary world attack must reach its captain");
            assert_eq!(world.get::<Ship>(target).unwrap().side, Side::Enemy);
            assert!(world.resource::<Target>().0.is_none(), "secondary attack must not overwrite the primary target");
        }
        50 => {
            let enemy = world.query::<(Entity, &Ship)>().iter(world).find(|(_, s)| s.side == Side::Enemy).map(|s| s.0);
            world.resource_mut::<Orbit>().focus = enemy;
        }
        55 => {
            let f = world.resource::<ship::Fleets>();
            let actor = f.selected.unwrap();
            let p = world.get::<ship::FleetPilot>(actor).unwrap();
            assert!(p.destination.is_some() && !p.hold, "secondary plane click must detach only its captain");
            assert!(world.resource::<Target>().0.is_none());
            assert!(f.roster().iter().skip(1).filter(|e| **e != actor).all(|e| world.get::<ship::FleetPilot>(*e).unwrap().hold));
            assert!(world.get::<PlayerShip>(actor).is_none());
        }
        62 => {
            let f = world.resource::<ship::Fleets>();
            assert!(f.primary() != *previous);
            assert_eq!(f.primary(), f.selected);
            assert!(!world.resource::<Time<Virtual>>().is_paused());
        }
        72 => assert!(world.resource::<ship::Fleets>().commands_open),
        137 => {
            let actor = world.resource::<ship::Fleets>().selected.unwrap();
            world.get_mut::<ship::FleetPilot>(actor).unwrap().move_to(Vec3::X * 100.0);
        }
        139 => assert!(world.resource::<crate::panels::FleetHud>().active.is_none()),
        143 => assert_eq!(world.resource::<crate::panels::FleetHud>().active, Some(ship::Formation::Convoy)),
        158 => {
            let e = world.query::<(Entity, &Ship)>().iter(world).find(|(_, s)| s.side == Side::Enemy).map(|s| s.0);
            world.resource_mut::<Target>().0 = e;
        }
        166 => assert!(world.resource::<crate::panels::FleetHud>().cease_fire),
        170 => assert!(!world.resource::<crate::panels::FleetHud>().cease_fire),
        179 => *previous = world.resource::<ship::Fleets>().primary(),
        185 => {
            let f = world.resource::<ship::Fleets>();
            assert!(f.primary() != *previous);
            assert_eq!(f.selected, f.primary());
            assert!(!f.bar_open);
            assert!(!f.commands_open);
        }
        225 => {
            assert!(world.resource::<ship::Fleets>().groups.is_empty());
            assert!(world.resource::<Orbit>().focus.is_none());
        }
        230 => world.resource_mut::<NextState<menu::Screen>>().set(menu::Screen::Battle),
        240 => {
            let f = world.resource::<ship::Fleets>();
            assert_eq!(f.selected, f.primary());
            assert_eq!(f.roster().len(), 4);
        }
        260 => info!("fleet ECS/UI smoke passed: selection, promotion, six buttons, detach/rejoin, cease/resume, restart and re-entry"),
        _ => {}
    }
}
