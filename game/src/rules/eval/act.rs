//! Rule actions.

use serde_json::Value;

use super::*;

impl Script {
    pub(in crate::rules) fn act(&mut self, a: &Value, w: &[Unit], out: &mut Vec<Fx>, rule: &str) {
        let t = s(a, "Type");
        let g = |k: &str| ids(w, s(a, k));
        match t {
            "Start Timer" => {
                self.stopped.remove(s(a, "Timer Name"));
                self.timers.insert(s(a, "Timer Name").into(), 0.0);
            }
            "Stop Timer" => {
                self.stopped.insert(s(a, "Timer Name").into());
            }
            "Set Flag Action" => {
                self.flags.insert(s(a, "Flag Name").into(), b(a, "Boolean Value"));
            }
            "Play speech event" => self.queue.push_back(s(a, "Speech Event Name").into()),
            "Play Music Track" => self.music.push(MusicCmd {
                file: s(a, "File Name").into(),
                fade_in: f(a, "Fade In Time ( secs )"),
                fade_out: f(a, "Fade Out Time ( secs )"),
                vol: f(a, "New Volume ( 0 to 1 )"),
            }),
            "Set Objective Task Active State" => {
                let n = s(a, "Objective Task");
                if let Some(t) = self.tasks.iter_mut().find(|t| t.name == n) {
                    t.active = b(a, "Active State");
                }
            }
            "Set Objective Task Failed State" => {
                let n = s(a, "Objective Task");
                if let Some(t) = self.tasks.iter_mut().find(|t| t.name == n) {
                    t.failed = b(a, "Failed State");
                }
            }
            "Set Objective Task Complete State" => {
                let n = s(a, "Objective Task");
                if let Some(t) = self.tasks.iter_mut().find(|t| t.name == n) {
                    t.complete = b(a, "Complete State");
                }
            }
            "Set Current Objective Point" => {
                let n = s(a, "Objective Point");
                self.point = self.points.as_array().and_then(|p| p.iter().position(|x| x["Name"] == n));
                self.point_on = None;
            }
            "Set Current Objective Point On Ship" => self.point_on = Some(s(a, "Group Name").into()),
            "Set Current Objective Point Visible On Starmap" => self.point_on_map = b(a, "Visible On Starmap"),
            "Set Map Text Visibility" => {
                self.map_text.insert(s(a, "Map Text Name").into(), b(a, "Visible On Starmap"));
            }
            "Group to follow path" => {
                out.push(Fx::Path { units: g("Group Name"), path: s(a, "Path Name").into(), closest: b(a, "Find Closest Point"), looping: s(a, "Follow Mode") == "loop" });
                out.push(Fx::MeteorPath {
                    group: s(a, "Group Name").into(),
                    path: s(a, "Path Name").into(),
                    closest: b(a, "Find Closest Point"),
                    mode: crate::combat::PathMode::parse(s(a, "Follow Mode")),
                });
            }
            "Setup Space Animal Flock" => {
                out.push(Fx::Path { units: g("Group Name"), path: s(a, "Path Name").into(), closest: b(a, "Find Closest Point"), looping: s(a, "Follow Mode") == "loop" });
                out.push(Fx::Velocity(g("Group Name"), f(a, "Velocity")));
            }
            "Setup Asteroid Belt" => out.push(Fx::Asteroids {
                units: g("Group Name"),
                path: s(a, "Path Name").into(),
                closest: b(a, "Find Closest Point"),
                mode: crate::combat::PathMode::parse(s(a, "Follow Mode")),
                speed: [f(a, "Velocity Lower m/sec"), f(a, "Velocity Upper m/sec")],
                tumble: [f(a, "Tumble Lower Rads/sec"), f(a, "Tumble Upper Rads/sec")],
            }),
            "Set Group Throttle Percent" => out.push(Fx::Speed(g("Group Name"), f(a, "Throttle Percent"))),
            "Destroy Group/Unit" => out.push(Fx::Destroy(g("Group Name"))),
            "Transfer Group/Unit to Group" => out.push(Fx::Owner(g("Group/Unit to transfer"), s(a, "Target Group").into())),
            // An attack order flown bow-on at emergency speed; the contact damage is collision.rs `Ram`.
            "GroupA to Ram GroupB" => out.push(Fx::Ram(g("Group Name"), s(a, "Group Unit Target").into())),
            "Teleport Group/Unit" => {
                if let Some(p) = self.point_sets[s(a, "Point Set Name")].get(0) {
                    out.push(Fx::Teleport(g("Group/Unit Name"), game_to_bevy(&p["pos"])));
                }
            }
            "Dock Ships" => out.push(Fx::Dock(g("Docking Group"), g("Target to Dock to"))),
            "Set Group/Unit Dockable" => out.push(Fx::Dockable(g("Group Name"), b(a, "Boolean"))),
            "Set Group/Unit Boardable" => out.push(Fx::Boardable(g("Group Name"), b(a, "Boolean"))),
            "Toggle Island Repair When Docked" => out.push(Fx::IslandRepair(g("Group Name"), b(a, "Repair when docked"))),
            "Set Dock Time" => out.push(Fx::DockTime(g("Docking GroupUnit"), f(a, "Dock Time"))),
            "Set Group Space Objects velocity" => {
                out.push(Fx::Velocity(g("Group Name"), f(a, "Velocity")));
                out.push(Fx::MeteorSpeed(s(a, "Group Name").into(), f(a, "Velocity")));
            }
            "*State Init* Setup Nebula" => {} // Spawned from the exported setup and point set by environment.rs.
            "Set Max Throttle Percent (Max user settable)" => out.push(Fx::Throttle(g("Group Name"), f(a, "Throttle Percent"))),
            // PROVISIONAL: a top-speed cap is the same speed limit as the max throttle (mission 9 nebula drain).
            "Set Ship Top Speed Percentage" => out.push(Fx::Throttle(g("Group Name"), f(a, "Throttle Speed"))),
            // The retail name string comes from the mission media (tools/export_campaign_media.py); unknown keys are ignored.
            "Set Ship Name" => match self.media["ship_names"][s(a, "New Ship Name")].as_str() {
                Some(name) => out.push(Fx::Label(g("Group/Unit Name"), name.into())),
                None => self.warn(&format!("ship name '{}'", s(a, "New Ship Name"))),
            },
            "Set Group/Unit AI Stance" => {
                let st = Stance::parse(s(a, "AI Stance"));
                out.push(Fx::Stance(g("Group/Unit Name"), st));
            }
            "Set Group/Unit Owner" => out.push(Fx::Owner(g("Group/Unit Name"), s(a, "New Owner").into())),
            "Set Group/Unit Visibility" => out.push(Fx::Visible(g("Group/Unit Name"), b(a, "Boolean"))),
            "Set Group/Unit Vulnerability" => out.push(Fx::Invuln(g("Group Name"), f(a, "Invulnerable Percent"))),
            "Set Ships Flag Texture" => out.push(Fx::Flag(g("Group Name"), s(a, "Flag Texture").into())),
            "Set Ship Banner Type" => out.push(Fx::Banner(g("Group/Unit Name"), s(a, "Banner Type").into())),
            "GroupA to attack GroupB" => out.push(Fx::Attack(g("Attack Group"), s(a, "Target Group").into())),
            "Clear all AI commands" => out.push(Fx::Clear(g("Group/Unit Name"))),
            "Damage Group/Unit by X percent" => out.push(Fx::Damage(g("Group Name"), f(a, "Percent"))),
            // A missing Boolean (older rules) means hold.
            "Set Group/Unit Hold Position" => out.push(Fx::Hold(g("Group/Unit Name"), a["Boolean"].is_null() || b(a, "Boolean"))),
            "Reset Hit Count" => out.push(Fx::ResetHits(g("Group/Unit Name"))),
            "Set FleetFormation Type" => {
                out.push(Fx::Formation(base(s(a, "Player Name")).into(), Some(crate::ship::Formation::parse(s(a, "Formation Type"))), b(a, "Hold Formation")))
            }
            "Set Fleet Hold Formation" => out.push(Fx::Formation(base(s(a, "Player Name")).into(), None, b(a, "Hold Formation"))),
            "Set Fleet Primary Ship" => out.push(Fx::Primary(g("Group/Unit Name"))),
            "Set Fleet Hold Fire" => {
                let o = base(s(a, "PLAYER FLEET")).to_string();
                if b(a, "HOLD FIRE") {
                    self.hold_fire.insert(o);
                } else {
                    self.hold_fire.remove(&o);
                }
            }
            "Set alliance between PlayerA and PlayerB to TRUE/FALSE" => {
                let (x, y) = (base(s(a, "PLAYER A")).to_string(), base(s(a, "PLAYER B")).to_string());
                self.hostile.retain(|p| !((p.0 == x && p.1 == y) || (p.0 == y && p.1 == x)));
                if !b(a, "SET ALLIANCE TO") {
                    self.hostile.insert((x, y));
                }
            }
            "Tow Ship" => self.tow_specs.push((s(a, "Docking Group").into(), s(a, "Target to Tow").into())),
            // Either group may be the tower: mission 4's "pirated tender safe" and mission 11's "Player Saves Civ Galleon" name the towed
            // ship first and the player second.
            "Break Tow" => {
                let (ga, gb) = (s(a, "TowerGroupA"), s(a, "TowerGroupB"));
                let (x, y) = (ids(w, ga), ids(w, gb));
                self.tows.retain(|t| !((x.contains(&t.0) && y.contains(&t.1)) || (y.contains(&t.0) && x.contains(&t.1))));
                self.tow_specs.retain(|p| !((p.0 == ga && p.1 == gb) || (p.0 == gb && p.1 == ga)));
            }
            "NIS Start" => {
                self.nis = true;
                self.cam = Cam::default();
            }
            "NIS End" => {
                self.nis = false;
                self.cam = Cam::default();
                self.zoom = None;
                self.all_visible = true;
            }
            "NIS Set Camera Path" => {
                self.cam.path = Some((s(a, "Path Name").into(), 0.0, 60.0));
            }
            "NIS Set Camera Speed" | "NIS Set Transition Camera Speed" => {
                if let Some(p) = &mut self.cam.path {
                    p.2 = f(a, "Max Velocity");
                }
            }
            "NIS Focus camera on Group/Unit" => {
                self.cam.focus_unit = Some(s(a, "GroupUnit Name").into());
                self.cam.focus_point = None;
                self.cam.attach = None;
            }
            "NIS Focus camera on Point" => {
                self.cam.focus_point = Some(s(a, "Point Set Name").into());
                self.cam.focus_unit = None;
            }
            "NIS Attach Camera" => {
                self.cam.attach = Some((s(a, "Group/Unit Name").into(), f(a, "Distance"), f(a, "Angle YZ"), f(a, "Angle XY")));
                self.cam.path = None;
            }
            // PROVISIONAL: a fixed offset from the unit ('Jump to point'), shown like an attached camera.
            "NIS Position Camera Relative to Object" => {
                self.cam.attach = Some((s(a, "Unit Name").into(), f(a, "Distance"), f(a, "Angle YZ"), f(a, "Angle XY")));
                self.cam.path = None;
            }
            "NIS Focus on Main Ship" => {
                self.cam.focus_unit = Some(format!("{},HUMAN CONTROLLED COMMAND SHIP", self.player));
                self.cam.focus_point = None;
                self.cam.attach = None;
            }
            "Set Group/Unit Cloak State" => out.push(Fx::Cloak(g("Group Name"), b(a, "Cloak State"), b(a, "AI Can Override"))),
            "Focus Camera On Group" => {
                let unit = members(w, s(a, "GroupUnit Name")).first().filter(|u| !u.primary).map(|u| u.e);
                out.push(Fx::Orbit { unit, dist: f(a, "Distance"), angle: f(a, "Relative Angle") });
            }
            "Add Victory Points for SinglePlayer" => self.score += a["Victory points to be added"].as_u64().unwrap_or(0) as u32,
            "Goto Next Level" => {
                if self.ended.is_none() {
                    self.ended = Some((victory_text(s(a, "World")), true));
                }
            }
            "End Game" => {
                if self.ended.is_none() {
                    let id = s(a, "Loser - Custom Message String ID");
                    self.ended = Some((self.media["messages"][id].as_str().unwrap_or("Mission failed").into(), false));
                }
                // A skirmish script that decides its game: the rule's own message ids (game strings) go to the end line.
                let id = |k: &str| Some(s(a, k)).filter(|m| m.starts_with("IDGS_")).map(String::from);
                let (win, lose) = (id("Winner - Custom Message String ID"), id("Loser - Custom Message String ID"));
                self.edit_end(|e| (e.winner_message, e.loser_message) = (win, lose));
            }
            "Create/Release Event Effect" => out.push(Fx::Event(s(a, "Point Set").into(), s(a, "Effect Name").into(), b(a, "Boolean State"))),
            "Open HUD Texture Overlay" => self.overlay = Some(s(a, "Texture Base Name").to_lowercase()),
            "Close HUD Texture Overlay" => self.overlay = None,
            "Open Weapon Bar" => out.push(Fx::WeaponBar(b(a, "OpenState"))),
            "Play Special Effect" => out.push(Fx::Sound(s(a, "File Name").into())),
            "Set Lifeboat Creation State" => out.push(Fx::Lifeboats(g("Group Name"), b(a, "Boolean"))),
            // Placement-time AI stance and path of one ship (the name/owner/primary fields are applied at spawn, campaign.rs).
            "*State Init* Setup Ship" => {
                let id = a["World Object ID"].as_i64().unwrap_or(-2);
                let units: Vec<Entity> = w.iter().filter(|u| u.id == id).map(|u| u.e).collect();
                out.push(Fx::Stance(units.clone(), Stance::parse(s(a, "AI Stance"))));
                if self.paths.get(s(a, "Ship Path")).is_some() {
                    out.push(Fx::Path { units, path: s(a, "Ship Path").into(), closest: false, looping: s(a, "Follow Mode") == "loop" });
                }
            }
            // Island setups are applied at spawn (campaign.rs: owner, combat strength); mission essential is the primary-alive loss rule.
            "*State Init* Setup Island" | "Set Group/Unit Mission Essential" => {}
            "Set Radar Active State" => self.radar = b(a, "Radar Active"),
            "Set Group/Unit Towable" => out.push(Fx::Towable(g("Group Name"), b(a, "Boolean"))),
            "Set Group/Unit Movable" => out.push(Fx::Movable(g("Group Name"), b(a, "Boolean"))),
            "SetCollidable" => out.push(Fx::Collidable(g("Group/Unit Name"), b(a, "Boolean"))),
            "Set Group/Unit Border Zone" => out.push(Fx::Border(g("Group/Unit Name"), Some(s(a, "Polygon Name").into()))),
            "Clear Group/Unit Border Zone" => out.push(Fx::Border(g("Group/Unit Name"), None)),
            "Set Group/Unit Border Zone Return Point" => {
                if let Some(p) = self.point_sets[s(a, "Pointset Name")].get(0) {
                    out.push(Fx::ReturnPoint(g("Group/Unit Name"), game_to_bevy(&p["pos"])));
                }
            }
            "Crew Speech - Toggle On Off" => self.crew_speech = b(a, "Crew Speech State State"),
            "Crew Speech - Helm Off Course" => self.helm_off_course = true,
            "Open Crew and Arms Screens" => self.crew_screen = true,
            "Set Is In Calyan Abyss" => self.abyss = b(a, "Boolean Value"),
            // Only Captain_Human is used by the shipped maps; the other captains keep the stance AI.
            "Set Group/Unit AI Captain" => {
                let units = g("Group/Unit Name");
                let human = s(a, "AI Stance") == "Captain_Human";
                out.push(Fx::Captain(units.clone(), human));
                if human {
                    out.push(Fx::Primary(units));
                }
            }
            // PROVISIONAL: FOV is the vertical angle in radians, speed its change per second.
            "NIS Zoom" => self.zoom = Some((f(a, "FOV").max(0.05), f(a, "Speed").max(0.01))),
            "NIS Toggle NIS mode Gun Accuracy" => self.nis_accuracy = b(a, "Use NIS Gun Accuracy"),
            "NIS Toggle All Objects Visibility" => self.all_visible = b(a, "All Objects Visible"),
            "Set Nebula Fadeout Distance" => self.nebula_fade = Some(f(a, "Fadeout Distance")),
            "Set Nebula to Lock Objects" => self.nebula_trap = b(a, "Trap"),
            "Set Group/Unit Warning Shot Mode" => out.push(Fx::WarningShots(g("GroupUnit Name"), b(a, "Boolean"))),
            "TUTORIAL - Pause when starmap opens?" => self.starmap_pause = b(a, "Pause when starmap opens"),
            "*State Init* Setup Etherium Current" => match self.paths[s(a, "Etherium Path")].as_array() {
                Some(p) => out.push(Fx::Current(p.iter().map(game_to_bevy).collect())),
                // Mission 10's 'bend' current names a path the TWT does not have (retail data); it has no effect there either.
                None => debug!("etherium current without path '{}'", s(a, "Etherium Path")),
            },
            "Mission 9 - Do Dark Matter Explosion" => out.push(Fx::DarkMatter(s(a, "Affected Area").into())),
            "Mission 9 - Teleport Longboat" => out.push(Fx::Longboat(g("Longboat Group Name"))),
            "Dragon - Set AI Stance" => out.push(Fx::DragonStance(g("Group/Unit Name"), Stance::parse(s(a, "AI Stance")))),
            "Dragon - Set Damage Threshold" => out.push(Fx::DragonThreshold(g("Group Name"), f(a, "Damage Threshold"))),
            "Grant Team X Points" => *self.team_points.entry(s(a, "Team Name").into()).or_default() += f(a, "Points") as i32,
            // Skirmish only: the winner is decided by victory.rs from the map's end rules; the objective point and task texts are shown by the star map and the ship ring.
            "Setup Team Objective" => {
                let (point, task) = (s(a, "Objective Point"), s(a, "Objective Task"));
                let known = self.points.as_array().is_some_and(|p| p.iter().any(|x| x["Name"] == point));
                let entry = self.team_objectives.entry(s(a, "Team Name").into()).or_default();
                if known {
                    entry.0 = point.into();
                } else if point == "NO OBJECTIVE POINT" {
                    entry.0.clear();
                }
                if !task.is_empty() && !entry.1.iter().any(|t| t == task) {
                    entry.1.push(task.into());
                }
            }
            // Skirmish only: a map that decides its own game (`"victory": "script"`) ends it here; otherwise victory.rs decides from the
            // map's own end rules.
            "Team X Wins" => {
                let team = s(a, "Team Name").to_string();
                self.edit_end(|e| e.kind = if team.is_empty() { EndKind::Winner } else { EndKind::Team(team) });
            }
            "Remaining Team Wins" => self.edit_end(|e| e.kind = EndKind::Winner),
            "Reset Shots Fired Count" => {
                for e in g("Group/Unit Name") {
                    self.shots.remove(&e);
                }
            }
            t if t.starts_with("OP ") => self.ext_act(a, w, out, rule),
            other => self.warn(&format!("action '{other}' in rule '{rule}'")),
        }
    }
}
