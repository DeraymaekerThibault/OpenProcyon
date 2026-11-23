//! Rule conditions.

use serde_json::Value;

use super::state::{Entered, EventKey};
use super::*;

impl Script {
    pub(in crate::rules) fn cond(&mut self, c: &Value, w: &[Unit], repeating: bool) -> bool {
        let t = s(c, "Type");
        match t {
            "World Initialize" => !self.init_done,
            "Flag Condition" => self.flags.get(s(c, "Flag Name")).copied().unwrap_or(false) == b(c, "Boolean Value"),
            "Timer Condition" => self.timer(s(c, "Timer Name")).is_some_and(|v| cmp(s(c, "Equivalence"), v, f(c, "Time in seconds"))),
            "Speech Event Played Once" => self.played.contains(s(c, "Speech Event Name")),
            "Speech Event Not Played Yet" => {
                let n = s(c, "Speech Event Name");
                !self.played.contains(n) && !self.queue.iter().any(|q| q == n) && self.speaking.as_ref().is_none_or(|x| x.0 != n)
            }
            "Is Group In Volume" | "Enter Volume" => self.group_in_volume(c, w, t == "Enter Volume", repeating),
            "Exit Volume" => self.group_exited_volume(c, w),
            // A live, visible ship of one of the team's players inside the volume (skirmish teams; anyone in the campaign).
            "Team Member Enters Volume" => {
                let (team, vol) = (s(c, "Team Name"), self.volume(s(c, "Volume Name")));
                w.iter().any(|u| u.alive && u.visible && self.in_team(team, &u.owner) && in_volume(vol, u.pos))
            }
            "Group Destroyed" => {
                let m = members(w, s(c, "Group Name"));
                !m.is_empty() && m.iter().all(|u| !u.alive)
            }
            // PROVISIONAL: 'under attack' = any member has been hit since its hit count was last reset.
            "Group Under Attack" => members(w, s(c, "Group Name")).iter().any(|u| self.hits.get(&u.e).is_some_and(|n| *n > 0)),
            "Group/Unit Fired X Shots" => {
                let n: u32 = members(w, s(c, "Group/Unit Name")).iter().map(|u| self.shots.get(&u.e).copied().unwrap_or(0)).sum();
                cmp(s(c, "Equivalence"), n as f32, f(c, "Number Of Rounds Fired"))
            }
            // PROVISIONAL: vital sections are not modelled; the unit's overall damage stands in.
            "Group/Unit Vital Section has >,<,= X damage" => {
                let m = members(w, s(c, "Group/Unit Name"));
                !m.is_empty() && cmp(s(c, "Equivalence"), m.iter().map(|u| u.damage).fold(0.0, f32::max), f(c, "Damage Percent"))
            }
            "Does Group Contain Unit Name" => {
                let (g, unit) = s(c, "Unit Name").split_once(',').unwrap_or((s(c, "Unit Name"), ""));
                w.iter().any(|u| u.alive && u.owner == g && u.name == unit) == b(c, "Exists?")
            }
            "Is Ship In Tow" => {
                let (x, y) = (ids(w, s(c, "Towing Group")), ids(w, s(c, "Target to Tow")));
                self.tows.iter().any(|t| x.contains(&t.0) && y.contains(&t.1)) == b(c, "Boolean")
            }
            "Is Group A attacking Group B" => self.attacking(w, s(c, "GroupA Name"), s(c, "GroupB Name")),
            "Group/Unit Hit at least x Times" => {
                let n: u32 = members(w, s(c, "Group/Unit Name")).iter().map(|u| self.hits.get(&u.e).copied().unwrap_or(0)).sum();
                n as f32 >= f(c, "Number Of Times Hit")
            }
            "Group/Unit Hit at least x Times by Player ( with eqivalence )" => {
                let by = base(s(c, "Player Name"));
                let n: u32 = members(w, s(c, "Group/Unit Name")).iter().map(|u| self.hits_by.get(&(by.into(), u.e)).copied().unwrap_or(0)).sum();
                cmp(s(c, "Equivalence"), n as f32, f(c, "Number Of Times Hit"))
            }
            "Player has hit Group/Unit at least x Times" => {
                let by = base(s(c, "Player Name"));
                let n: u32 = members(w, s(c, "Group/Unit Name")).iter().map(|u| self.hits_by.get(&(by.into(), u.e)).copied().unwrap_or(0)).sum();
                n as f32 >= f(c, "Number Of Times Hit")
            }
            "Group has X members" => cmp(s(c, "Equivalence"), members(w, s(c, "Group Name")).iter().filter(|u| u.alive).count() as f32, f(c, "number")),
            "Group/Unit has >,<,= X damage" => {
                let m = members(w, s(c, "Group/Unit Name"));
                !m.is_empty() && cmp(s(c, "Equivalence"), m.iter().map(|u| u.damage).fold(0.0, f32::max), f(c, "Damage Percent"))
            }
            "Group/Unit contains no mission essential ships" => !w.iter().any(|u| u.owner == base(s(c, "Group Name")) && u.primary && u.alive),
            "Unit Flag Texture" => {
                // TWT group membership: ships that left the group (owner 'Dead Ships') still count.
                let m = members(w, base(s(c, "Unit Name")));
                !m.is_empty() && m.iter().all(|u| u.flag == s(c, "Flag Type")) == b(c, "Boolean")
            }
            "Group to Group Distance" => {
                let (a, bb) = (members(w, s(c, "GroupA Name")), members(w, s(c, "GroupB Name")));
                let d = a
                    .iter()
                    .filter(|u| u.alive && u.visible)
                    .flat_map(|x| bb.iter().filter(|u| u.alive && u.visible).map(move |y| x.pos.distance(y.pos)))
                    .fold(f32::INFINITY, f32::min);
                d.is_finite() && cmp(s(c, "Equivalence"), d, f(c, "Distance"))
            }
            "Group/Unit Docked" | "Group/Unit Is Docked" => {
                let (a, bb) = (members(w, s(c, "Dockers Group/Unit Name")), members(w, s(c, "Targets Group/Unit Name")));
                a.iter().any(|x| bb.iter().any(|y| self.docked.contains(&(x.e, y.e))))
            }
            "Unit is Within any Nebula" => members(w, s(c, "Unit Name")).iter().any(|u| u.alive && self.nebulae.iter().any(|p| in_volume(p, u.pos))),
            "All Units from Group have entered trigger volume Once" => {
                let key = Entered::AllOnce { group: s(c, "Group Name").into(), volume: s(c, "Volume Name").into() };
                let vol = self.volume(s(c, "Volume Name")).clone();
                let m: Vec<_> = members(w, s(c, "Group Name")).into_iter().filter(|u| u.alive).collect();
                let seen = self.entered.entry(key).or_default();
                seen.extend(m.iter().filter(|u| in_volume(&vol, u.pos)).map(|u| u.e));
                !m.is_empty() && m.iter().all(|u| seen.contains(&u.e))
            }
            // The original's count is the lifeboat table; an unknown player makes the condition false.
            "Player Vs Player capture count" => {
                let (a, bb) = (base(s(c, "Player Name A")), base(s(c, "Player Name B")));
                let known = |p: &str| self.players.iter().any(|x| x == p);
                let n = self.lifeboats_saved_by(a, bb) + self.base_captures.get(&(a.into(), bb.into())).copied().unwrap_or(0);
                known(a) && known(bb) && cmp(s(c, "Equivalence"), n as f32, f(c, "Number Of Captures"))
            }
            // Campaign maps have no teams: a capture by anyone of a unit of the group counts there.
            "Team has captured a ship from Group/Unit" => {
                let (team, group) = (s(c, "Team Name"), s(c, "Group/Unit"));
                let ids = ids(w, group);
                let n = self.captured.iter().filter(|(by, e)| ids.contains(e) && self.in_team(team, by)).count() as u32;
                self.event(EventKey::Captured { team: team.into(), group: group.into() }, n, f(c, "Number Of Captures"))
            }
            "Team has destroyed a ship from Group/Unit" => {
                let (team, group) = (s(c, "Team Name"), s(c, "Group/Unit"));
                let ids = ids(w, group);
                let n = self.destroyed.iter().filter(|(by, e)| ids.contains(e) && self.in_team(team, by)).count() as u32;
                self.event(EventKey::Destroyed { team: team.into(), group: group.into() }, n, f(c, "Number Of Ships Destroyed"))
            }
            // Each unit of the group counts once, the first time it is inside. PROVISIONAL: 'Team Name' is read as the team the rule
            // credits, not a filter on the units (Convoy Raid: the convoys entering the pirates' theft area score for the pirates).
            "Unit from Group enters trigger volume ( Once per Unit )" => {
                let (group, volume) = (s(c, "Group Name").to_string(), s(c, "Volume Name").to_string());
                let vol = self.volume(s(c, "Volume Name")).clone();
                let inside: Vec<Entity> = members(w, s(c, "Group Name")).iter().filter(|u| u.alive && in_volume(&vol, u.pos)).map(|u| u.e).collect();
                let seen = self.entered.entry(Entered::OncePerUnit { group: group.clone(), volume: volume.clone() }).or_default();
                seen.extend(inside);
                let n = seen.len() as u32;
                self.event(EventKey::Once { group, volume }, n, 0.0)
            }
            "Team has X points" => self.team_points.get(s(c, "Team Name")).copied().unwrap_or(0) as f32 >= f(c, "Points"),
            // True for an unknown player; otherwise no lifeboat is owned by the player (a boat counts until it is removed).
            "Player has no lifeboats" => {
                let p = base(s(c, "Player Name"));
                !self.players.iter().any(|x| x == p) || !self.lifeboat_owners.iter().any(|o| o == p)
            }
            "Is Starmap Open" => self.starmap_open,
            // A mortar burst inside the volume since the rule last looked (bursts accumulate until the frame's step ends).
            "Mission 9 - If Mortar explodes within Area" => {
                let vol = self.volume(s(c, "Volume Name"));
                let hit = self.mortar_bursts.iter().any(|p| in_volume(vol, *p));
                if hit {
                    self.mortar_seen.insert(s(c, "Volume Name").into());
                }
                hit || self.mortar_seen.contains(s(c, "Volume Name"))
            }
            // One event per object destroyed by the player, by kind (rocks or ships); a repeating rule scores once per kill.
            "Player Killed A Object" => {
                let (player, kind) = (base(s(c, "Player Name")), s(c, "World Object Type"));
                let n = self.kills.get(&(player.to_string(), kind == "Asteroid")).copied().unwrap_or(0);
                self.event(EventKey::Killed { player: player.into(), kind: kind.into() }, n, 0.0)
            }
            t if t.starts_with("OP ") => self.ext_cond(c, w, repeating),
            other => {
                self.warn(&format!("condition '{other}'"));
                false
            }
        }
    }

    pub(in crate::rules) fn attacking(&self, w: &[Unit], a: &str, bb: &str) -> bool {
        let target_group = base(bb);
        let targets = members(w, bb);
        // Rocks: a shot rock may already be gone from the unit list, so `belt` remembers which group names it answered to.
        let hit = |tg: &Entity| targets.iter().any(|x| x.e == *tg) || self.belt.get(tg).is_some_and(|g| g.iter().any(|g| g == target_group));
        members(w, a).iter().any(|u| u.attack.iter().any(|g| base(g) == target_group) || self.fired_at.get(&u.e).is_some_and(|(t, tg)| self.time - t < 6.0 && hit(tg))) // PROVISIONAL: the 6 s "attacking" window is a guess
    }
}
