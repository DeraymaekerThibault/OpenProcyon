//! Rule actions.

use serde_json::Value;

use super::*;

/// A follow-path order for `units` on the path named by field `path_key`; 'Follow Mode' "loop" repeats it.
pub(super) fn follow(units: Vec<Entity>, a: &Value, path_key: &str, closest: bool) -> Fx {
    Fx::Path { units, path: s(a, path_key).into(), closest, looping: s(a, "Follow Mode") == "loop" }
}

impl Script {
    /// Where point `idx` of the mission's point set `set` lies (world position), if the set has that point.
    pub(in crate::rules) fn point_of(&self, set: &str, idx: usize) -> Option<Vec3> {
        self.point_sets[set].get(idx).map(|p| game_to_bevy(&p["pos"]))
    }
    /// Sets one state of the objective task the action names (field "Objective Task") to the boolean in `key`.
    fn set_task(&mut self, a: &Value, key: &str, set: impl FnOnce(&mut Task, bool)) {
        let n = s(a, "Objective Task");
        if let Some(t) = self.tasks.iter_mut().find(|t| t.name == n) {
            set(t, b(a, key));
        }
    }
    pub(in crate::rules) fn act(&mut self, a: &Value, w: &[Unit], out: &mut Vec<Fx>, rule: &str) {
        let Some(ty) = RuleType::of(Role::Action, a) else {
            self.warn(&format!("action '{}' in rule '{rule}'", s(a, "Type")));
            return;
        };
        let g = |k: &str| ids(w, s(a, k));
        match ty {
            T::StartTimer => {
                self.stopped.remove(s(a, "Timer Name"));
                self.timers.insert(s(a, "Timer Name").into(), 0.0);
            }
            T::StopTimer => {
                self.stopped.insert(s(a, "Timer Name").into());
            }
            T::SetFlagAction => {
                self.flags.insert(s(a, "Flag Name").into(), b(a, "Boolean Value"));
            }
            T::PlaySpeechEvent => self.queue.push_back(s(a, "Speech Event Name").into()),
            T::PlayMusicTrack => self.music.push(MusicCmd {
                file: s(a, "File Name").into(),
                fade_in: f(a, "Fade In Time ( secs )"),
                fade_out: f(a, "Fade Out Time ( secs )"),
                vol: f(a, "New Volume ( 0 to 1 )"),
            }),
            T::SetObjectiveTaskActiveState => self.set_task(a, "Active State", |t, on| t.active = on),
            T::SetObjectiveTaskFailedState => self.set_task(a, "Failed State", |t, on| t.failed = on),
            T::SetObjectiveTaskCompleteState => self.set_task(a, "Complete State", |t, on| t.complete = on),
            T::SetCurrentObjectivePoint => {
                let n = s(a, "Objective Point");
                self.point = self.points.as_array().and_then(|p| p.iter().position(|x| x["Name"] == n));
                self.point_on = None;
            }
            T::SetCurrentObjectivePointOnShip => self.point_on = Some(s(a, "Group Name").into()),
            T::SetCurrentObjectivePointVisibleOnStarmap => self.point_on_map = b(a, "Visible On Starmap"),
            T::SetMapTextVisibility => {
                self.map_text.insert(s(a, "Map Text Name").into(), b(a, "Visible On Starmap"));
            }
            T::GroupToFollowPath => {
                out.push(follow(g("Group Name"), a, "Path Name", b(a, "Find Closest Point")));
                out.push(Fx::MeteorPath {
                    group: s(a, "Group Name").into(),
                    path: s(a, "Path Name").into(),
                    closest: b(a, "Find Closest Point"),
                    mode: crate::combat::PathMode::parse(s(a, "Follow Mode")),
                });
            }
            T::SetupSpaceAnimalFlock => {
                out.push(follow(g("Group Name"), a, "Path Name", b(a, "Find Closest Point")));
                out.push(Fx::Velocity(g("Group Name"), f(a, "Velocity")));
            }
            T::SetupAsteroidBelt => out.push(Fx::Asteroids {
                units: g("Group Name"),
                path: s(a, "Path Name").into(),
                closest: b(a, "Find Closest Point"),
                mode: crate::combat::PathMode::parse(s(a, "Follow Mode")),
                speed: [f(a, "Velocity Lower m/sec"), f(a, "Velocity Upper m/sec")],
                tumble: [f(a, "Tumble Lower Rads/sec"), f(a, "Tumble Upper Rads/sec")],
            }),
            T::SetGroupThrottlePercent => out.push(Fx::Speed(g("Group Name"), f(a, "Throttle Percent"))),
            T::DestroyGroupUnit => out.push(Fx::Destroy(g("Group Name"))),
            T::TransferGroupUnitToGroup => out.push(Fx::Owner(g("Group/Unit to transfer"), s(a, "Target Group").into())),
            // An attack order flown bow-on at emergency speed; the contact damage is collision.rs `Ram`.
            T::GroupAToRamGroupB => out.push(Fx::Ram(g("Group Name"), s(a, "Group Unit Target").into())),
            T::TeleportGroupUnit => {
                if let Some(dest) = self.point_of(s(a, "Point Set Name"), 0) {
                    out.push(Fx::Teleport(g("Group/Unit Name"), dest));
                }
            }
            T::DockShips => out.push(Fx::Dock(g("Docking Group"), g("Target to Dock to"))),
            T::SetGroupUnitDockable => out.push(Fx::Dockable(g("Group Name"), b(a, "Boolean"))),
            T::SetGroupUnitBoardable => out.push(Fx::Boardable(g("Group Name"), b(a, "Boolean"))),
            T::ToggleIslandRepairWhenDocked => out.push(Fx::IslandRepair(g("Group Name"), b(a, "Repair when docked"))),
            T::SetDockTime => out.push(Fx::DockTime(g("Docking GroupUnit"), f(a, "Dock Time"))),
            T::SetGroupSpaceObjectsVelocity => {
                out.push(Fx::Velocity(g("Group Name"), f(a, "Velocity")));
                out.push(Fx::MeteorSpeed(s(a, "Group Name").into(), f(a, "Velocity")));
            }
            T::StateInitSetupNebula => {} // Spawned from the exported setup and point set by environment.rs.
            T::SetMaxThrottlePercentMaxUserSettable => out.push(Fx::Throttle(g("Group Name"), f(a, "Throttle Percent"))),
            // PROVISIONAL: a top-speed cap is the same speed limit as the max throttle (mission 9 nebula drain).
            T::SetShipTopSpeedPercentage => out.push(Fx::Throttle(g("Group Name"), f(a, "Throttle Speed"))),
            // The retail name string comes from the mission media (tools/export_campaign_media.py); unknown keys are ignored.
            T::SetShipName => match self.media["ship_names"][s(a, "New Ship Name")].as_str() {
                Some(name) => out.push(Fx::Label(g("Group/Unit Name"), name.into())),
                None => self.warn(&format!("ship name '{}'", s(a, "New Ship Name"))),
            },
            T::SetGroupUnitAIStance => {
                let st = Stance::parse(s(a, "AI Stance"));
                out.push(Fx::Stance(g("Group/Unit Name"), st));
            }
            T::SetGroupUnitOwner => out.push(Fx::Owner(g("Group/Unit Name"), s(a, "New Owner").into())),
            T::SetGroupUnitVisibility => out.push(Fx::Visible(g("Group/Unit Name"), b(a, "Boolean"))),
            T::SetGroupUnitVulnerability => out.push(Fx::Invuln(g("Group Name"), f(a, "Invulnerable Percent"))),
            T::SetShipsFlagTexture => out.push(Fx::Flag(g("Group Name"), s(a, "Flag Texture").into())),
            T::SetShipBannerType => out.push(Fx::Banner(g("Group/Unit Name"), s(a, "Banner Type").into())),
            T::GroupAToAttackGroupB => out.push(Fx::Attack(g("Attack Group"), s(a, "Target Group").into())),
            T::ClearAllAICommands => out.push(Fx::Clear(g("Group/Unit Name"))),
            T::DamageGroupUnitByXPercent => out.push(Fx::Damage(g("Group Name"), f(a, "Percent"))),
            // A missing Boolean (older rules) means hold.
            T::SetGroupUnitHoldPosition => out.push(Fx::Hold(g("Group/Unit Name"), a["Boolean"].is_null() || b(a, "Boolean"))),
            T::ResetHitCount => out.push(Fx::ResetHits(g("Group/Unit Name"))),
            T::SetFleetFormationType => {
                out.push(Fx::Formation(base(s(a, "Player Name")).into(), Some(crate::ship::Formation::parse(s(a, "Formation Type"))), b(a, "Hold Formation")))
            }
            T::SetFleetHoldFormation => out.push(Fx::Formation(base(s(a, "Player Name")).into(), None, b(a, "Hold Formation"))),
            T::SetFleetPrimaryShip => out.push(Fx::Primary(g("Group/Unit Name"))),
            T::SetFleetHoldFire => {
                let o = Owner::from(base(s(a, "PLAYER FLEET")));
                if b(a, "HOLD FIRE") {
                    self.hold_fire.insert(o);
                } else {
                    self.hold_fire.remove(&o);
                }
            }
            T::SetAllianceBetweenPlayerAAndPlayerBToTRUEFALSE => {
                let (x, y) = (Owner::from(base(s(a, "PLAYER A"))), Owner::from(base(s(a, "PLAYER B"))));
                self.hostile.retain(|p| !((p.0 == x && p.1 == y) || (p.0 == y && p.1 == x)));
                if !b(a, "SET ALLIANCE TO") {
                    self.hostile.insert((x, y));
                }
            }
            T::TowShip => self.tow_specs.push((s(a, "Docking Group").into(), s(a, "Target to Tow").into())),
            // Either group may be the tower: mission 4's "pirated tender safe" and mission 11's "Player Saves Civ Galleon" name the towed
            // ship first and the player second.
            T::BreakTow => {
                let (ga, gb) = (s(a, "TowerGroupA"), s(a, "TowerGroupB"));
                let (x, y) = (ids(w, ga), ids(w, gb));
                self.tows.retain(|t| !((x.contains(&t.0) && y.contains(&t.1)) || (y.contains(&t.0) && x.contains(&t.1))));
                self.tow_specs.retain(|p| !((p.0 == ga && p.1 == gb) || (p.0 == gb && p.1 == ga)));
            }
            T::NISStart => {
                self.nis = true;
                self.cam = Cam::default();
            }
            T::NISEnd => {
                self.nis = false;
                self.cam = Cam::default();
                self.zoom = None;
                self.all_visible = true;
            }
            T::NISSetCameraPath => {
                self.cam.path = Some((s(a, "Path Name").into(), 0.0, 60.0)); // PROVISIONAL: default camera path speed 60 is a guess
            }
            T::NISSetCameraSpeed | T::NISSetTransitionCameraSpeed => {
                if let Some(p) = &mut self.cam.path {
                    p.2 = f(a, "Max Velocity");
                }
            }
            T::NISFocusCameraOnGroupUnit => {
                self.cam.focus_unit = Some(s(a, "GroupUnit Name").into());
                self.cam.focus_point = None;
                self.cam.attach = None;
            }
            T::NISFocusCameraOnPoint => {
                self.cam.focus_point = Some(s(a, "Point Set Name").into());
                self.cam.focus_unit = None;
            }
            T::NISAttachCamera => {
                self.cam.attach = Some((s(a, "Group/Unit Name").into(), f(a, "Distance"), f(a, "Angle YZ"), f(a, "Angle XY")));
                self.cam.path = None;
            }
            // PROVISIONAL: a fixed offset from the unit ('Jump to point'), shown like an attached camera.
            T::NISPositionCameraRelativeToObject => {
                self.cam.attach = Some((s(a, "Unit Name").into(), f(a, "Distance"), f(a, "Angle YZ"), f(a, "Angle XY")));
                self.cam.path = None;
            }
            T::NISFocusOnMainShip => {
                self.cam.focus_unit = Some(format!("{},HUMAN CONTROLLED COMMAND SHIP", self.player));
                self.cam.focus_point = None;
                self.cam.attach = None;
            }
            T::SetGroupUnitCloakState => out.push(Fx::Cloak(g("Group Name"), b(a, "Cloak State"), b(a, "AI Can Override"))),
            T::FocusCameraOnGroup => {
                let unit = members(w, s(a, "GroupUnit Name")).first().filter(|u| !u.primary).map(|u| u.e);
                out.push(Fx::Orbit { unit, dist: f(a, "Distance"), angle: f(a, "Relative Angle") });
            }
            T::AddVictoryPointsForSinglePlayer => self.score += a["Victory points to be added"].as_u64().unwrap_or(0) as u32,
            T::GotoNextLevel => {
                if self.ended.is_none() {
                    self.ended = Some((victory_text(s(a, "World")), true));
                }
            }
            T::EndGame => {
                if self.ended.is_none() {
                    let id = s(a, "Loser - Custom Message String ID");
                    self.ended = Some((self.media["messages"][id].as_str().unwrap_or("Mission failed").into(), false));
                }
                // A skirmish script that decides its game: the rule's own message ids (game strings) go to the end line.
                let id = |k: &str| Some(s(a, k)).filter(|m| m.starts_with("IDGS_")).map(String::from);
                let (win, lose) = (id("Winner - Custom Message String ID"), id("Loser - Custom Message String ID"));
                self.edit_end(|e| (e.winner_message, e.loser_message) = (win, lose));
            }
            T::CreateReleaseEventEffect => out.push(Fx::Event(s(a, "Point Set").into(), s(a, "Effect Name").into(), b(a, "Boolean State"))),
            T::OpenHUDTextureOverlay => self.overlay = Some(s(a, "Texture Base Name").to_lowercase()),
            T::CloseHUDTextureOverlay => self.overlay = None,
            T::OpenWeaponBar => out.push(Fx::WeaponBar(b(a, "OpenState"))),
            T::PlaySpecialEffect => out.push(Fx::Sound(s(a, "File Name").into())),
            T::SetLifeboatCreationState => out.push(Fx::Lifeboats(g("Group Name"), b(a, "Boolean"))),
            // Placement-time AI stance and path of one ship (the name/owner/primary fields are applied at spawn, campaign.rs).
            T::StateInitSetupShip => {
                let id = a["World Object ID"].as_i64().unwrap_or(-2);
                let units: Vec<Entity> = w.iter().filter(|u| u.id == id).map(|u| u.e).collect();
                out.push(Fx::Stance(units.clone(), Stance::parse(s(a, "AI Stance"))));
                if self.paths.get(s(a, "Ship Path")).is_some() {
                    out.push(follow(units, a, "Ship Path", false));
                }
            }
            // Island setups are applied at spawn (campaign.rs: owner, combat strength); mission essential is the primary-alive loss rule.
            T::StateInitSetupIsland | T::SetGroupUnitMissionEssential => {}
            T::SetRadarActiveState => self.radar = b(a, "Radar Active"),
            T::SetGroupUnitTowable => out.push(Fx::Towable(g("Group Name"), b(a, "Boolean"))),
            T::SetGroupUnitMovable => out.push(Fx::Movable(g("Group Name"), b(a, "Boolean"))),
            T::SetCollidable => out.push(Fx::Collidable(g("Group/Unit Name"), b(a, "Boolean"))),
            T::SetGroupUnitBorderZone => out.push(Fx::Border(g("Group/Unit Name"), Some(s(a, "Polygon Name").into()))),
            T::ClearGroupUnitBorderZone => out.push(Fx::Border(g("Group/Unit Name"), None)),
            T::SetGroupUnitBorderZoneReturnPoint => {
                if let Some(dest) = self.point_of(s(a, "Pointset Name"), 0) {
                    out.push(Fx::ReturnPoint(g("Group/Unit Name"), dest));
                }
            }
            T::CrewSpeechToggleOnOff => self.crew_speech = b(a, "Crew Speech State State"),
            T::CrewSpeechHelmOffCourse => self.helm_off_course = true,
            T::OpenCrewAndArmsScreens => self.crew_screen = true,
            T::SetIsInCalyanAbyss => self.abyss = b(a, "Boolean Value"),
            // Only Captain_Human is used by the shipped maps; the other captains keep the stance AI.
            T::SetGroupUnitAICaptain => {
                let units = g("Group/Unit Name");
                let human = s(a, "AI Stance") == "Captain_Human";
                out.push(Fx::Captain(units.clone(), human));
                if human {
                    out.push(Fx::Primary(units));
                }
            }
            // PROVISIONAL: FOV is the vertical angle in radians, speed its change per second.
            T::NISZoom => self.zoom = Some((f(a, "FOV").max(0.05), f(a, "Speed").max(0.01))),
            T::NISToggleNISModeGunAccuracy => self.nis_accuracy = b(a, "Use NIS Gun Accuracy"),
            T::NISToggleAllObjectsVisibility => self.all_visible = b(a, "All Objects Visible"),
            T::SetNebulaFadeoutDistance => self.nebula_fade = Some(f(a, "Fadeout Distance")),
            T::SetNebulaToLockObjects => self.nebula_trap = b(a, "Trap"),
            T::SetGroupUnitWarningShotMode => out.push(Fx::WarningShots(g("GroupUnit Name"), b(a, "Boolean"))),
            T::TUTORIALPauseWhenStarmapOpens => self.starmap_pause = b(a, "Pause when starmap opens"),
            T::StateInitSetupEtheriumCurrent => match self.paths[s(a, "Etherium Path")].as_array() {
                Some(p) => out.push(Fx::Current(s(a, "Etherium Path").into(), p.iter().map(game_to_bevy).collect())),
                // Mission 10's 'bend' current names a path the TWT does not have (retail data); it has no effect there either.
                None => debug!("etherium current without path '{}'", s(a, "Etherium Path")),
            },
            T::Mission9DoDarkMatterExplosion => out.push(Fx::DarkMatter(s(a, "Affected Area").into())),
            T::Mission9TeleportLongboat => out.push(Fx::Longboat(g("Longboat Group Name"))),
            T::DragonSetAIStance => out.push(Fx::DragonStance(g("Group/Unit Name"), Stance::parse(s(a, "AI Stance")))),
            T::DragonSetDamageThreshold => out.push(Fx::DragonThreshold(g("Group Name"), f(a, "Damage Threshold"))),
            T::GrantTeamXPoints => *self.team_points.entry(s(a, "Team Name").into()).or_default() += f(a, "Points") as i32,
            // Skirmish only: the winner is decided by victory.rs from the map's end rules; the objective point and task texts are shown by the star map and the ship ring.
            T::SetupTeamObjective => {
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
            T::TeamXWins => {
                let team = s(a, "Team Name").to_string();
                self.edit_end(|e| e.kind = if team.is_empty() { EndKind::Winner } else { EndKind::Team(team) });
            }
            T::RemainingTeamWins => self.edit_end(|e| e.kind = EndKind::Winner),
            T::ResetShotsFiredCount => {
                for e in g("Group/Unit Name") {
                    self.shots.remove(&e);
                }
            }
            t if t.is_engine() => self.ext_act(t, a, w, out, rule),
            other => self.warn(&format!("action '{}' in rule '{rule}'", other.name())),
        }
    }
}
