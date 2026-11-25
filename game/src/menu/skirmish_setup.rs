//! A whole skirmish setup as data (#369 Phase 1): what the lobby holds when Launch is pressed. One type for the command-log header
//! (a menu-built battle replays with `--replay <file>` alone), the JSON form of `--lobby`, and later what a host's lobby sends.
//!
//! Identifiers only, never text from the game files: map id, ship class ids, gun keys and crew keys; a ship's picked name is its
//! position in the hull's name list. Player names are not kept. Computer fleets are not written: the lobby draws them at launch from
//! `draw` and the game data (`Skirmish::plan`), so a setup replays only on the same data (and mods). PROVISIONAL: the layout.
//!
//! ```json
//! {"map":"border_dispute","points":1000,"limit":10,"draw":0,"slots":[
//!   {"kind":"human","race":"navy","team":1,"flag":0,"fleet":[{"class":"Ship_Navy_Frigate","guns":{"3":"<gun key>"},"name":2,"crew":[["<crew key>"],[]]}]},
//!   {"kind":"computer","race":"pirate","team":2,"flag":0}]}
//! ```
use super::*;
use crate::scenario::Race;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Most crew keys one post may hold in a setup: a damaged file cannot ask for unbounded work.
const MAX_CREW_PER_POST: usize = 16;

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkirmishSetup {
    pub map: String,
    /// Victory points (the lobby's slider).
    pub points: u32,
    /// Fleet size limit.
    pub limit: usize,
    /// Varies the fleets the computers draw (`Skirmish::draw`).
    #[serde(default)]
    pub draw: u64,
    /// Every lobby row of the map, in order.
    pub slots: Vec<SlotSetup>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotKind {
    Human,
    Computer,
    Open,
    Closed,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlotSetup {
    pub kind: SlotKind,
    /// Fleet type; left out while none is chosen.
    #[serde(default, skip_serializing_if = "unchosen")]
    pub race: Race,
    /// -1: on its own.
    pub team: i32,
    pub flag: u8,
    /// The ships the player picked (empty for computers: their fleets are drawn at launch).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fleet: Vec<ShipSetup>,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShipSetup {
    /// Ship class id.
    pub class: String,
    /// Swapped guns: weapon bank (`Mount::bank`) to gun key.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub guns: BTreeMap<usize, String>,
    /// The picked name: its position in the hull's name list. Left out: the default name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<usize>,
    /// Named crew keys per post (`crew_arms::ROLES` order), trailing empty posts left out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub crew: Vec<Vec<String>>,
}

fn unchosen(r: &Race) -> bool {
    *r == Race::Unchosen
}

impl SlotKind {
    fn of(k: Kind) -> Self {
        match k {
            Kind::Human => SlotKind::Human,
            Kind::Computer => SlotKind::Computer,
            Kind::Open => SlotKind::Open,
            Kind::Closed => SlotKind::Closed,
        }
    }
    fn kind(self) -> Kind {
        match self {
            SlotKind::Human => Kind::Human,
            SlotKind::Computer => Kind::Computer,
            SlotKind::Open => Kind::Open,
            SlotKind::Closed => Kind::Closed,
        }
    }
}

impl SkirmishSetup {
    /// The JSON form (`--lobby '{...}'`).
    pub fn parse(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| format!("lobby setup: {e}"))
    }

    /// What the lobby holds now (the map it shows, its sliders and every row).
    pub fn capture(sk: &Skirmish, lib: &Library) -> Option<Self> {
        let map = sk.maps.get(sk.selected)?.0.clone();
        let slots = sk
            .slots
            .iter()
            .map(|s| SlotSetup {
                kind: SlotKind::of(s.kind),
                race: s.race,
                team: s.team,
                flag: s.flag,
                fleet: s
                    .fleet
                    .iter()
                    .enumerate()
                    .map(|(i, class)| {
                        let pool = lib.names.get(class);
                        let name = s.names.get(i).cloned().flatten().and_then(|n| pool.and_then(|p| p.iter().position(|x| *x == n)));
                        let mut crew: Vec<Vec<String>> = s.crew.get(i).map(|c| c.to_vec()).unwrap_or_default();
                        while crew.last().is_some_and(Vec::is_empty) {
                            crew.pop();
                        }
                        ShipSetup { class: class.clone(), guns: s.weapons.get(i).cloned().unwrap_or_default(), name, crew }
                    })
                    .collect(),
            })
            .collect();
        Some(SkirmishSetup { map, points: sk.victory_limit, limit: sk.fleet_size, draw: sk.draw, slots })
    }

    /// Sets the lobby to this setup, as the lobby's own controls would have left it, checked against the lobby's rules and this
    /// game's data. The lobby is then ready for Launch.
    pub fn apply(&self, sk: &mut Skirmish, lib: &Library) -> Result<(), String> {
        let at = sk.maps.iter().position(|m| m.0 == self.map).ok_or_else(|| format!("lobby setup: no skirmish map {:?}", self.map))?;
        if !(VICTORY_LIMIT.min..=VICTORY_LIMIT.max).contains(&self.points) || !(self.points - VICTORY_LIMIT.min).is_multiple_of(VICTORY_LIMIT.step) {
            return Err(format!("lobby setup points {}: {}-{} in steps of {}", self.points, VICTORY_LIMIT.min, VICTORY_LIMIT.max, VICTORY_LIMIT.step));
        }
        if !(FLEET_SIZE.min as usize..=FLEET_SIZE.max as usize).contains(&self.limit) {
            return Err(format!("lobby setup limit {}: {}-{}", self.limit, FLEET_SIZE.min, FLEET_SIZE.max));
        }
        sk.historical = !sk.maps[at].2;
        sk.selected = at;
        sk.load_map();
        sk.victory_limit = self.points;
        sk.fleet_size = self.limit;
        sk.draw = self.draw;
        if self.slots.len() != sk.slots.len() {
            return Err(format!("lobby setup: {} rows, but {} has {}", self.slots.len(), self.map, sk.slots.len()));
        }
        let (open, free) = (sk.open(), sk.free_teams());
        for (i, want) in self.slots.iter().enumerate() {
            let row = &sk.slots[i];
            // The human's row is the map's; the others can be anything but human.
            if (want.kind == SlotKind::Human) != (row.kind == Kind::Human) {
                return Err(format!("lobby setup row {i}: the human plays row {}", sk.slots.iter().position(|s| s.kind == Kind::Human).unwrap_or(0)));
            }
            if !(-1..TEAMS).contains(&want.team) {
                return Err(format!("lobby setup row {i}: team {} (-1 for none, 0-{})", want.team, TEAMS - 1));
            }
            if !free && (want.race, want.team) != (row.race, row.team) {
                return Err(format!("lobby setup row {i}: {} sets fleet type and team ({:?}, team {})", self.map, row.race, row.team));
            }
            if want.flag >= 10 {
                return Err(format!("lobby setup row {i}: flag {} (0-9)", want.flag));
            }
            if !want.fleet.is_empty() && (!open || !want.kind.kind().live()) {
                return Err(format!("lobby setup row {i}: a fleet on a row or map that does not assemble one"));
            }
            if want.fleet.len() > self.limit {
                return Err(format!("lobby setup row {i}: {} ships, the limit is {}", want.fleet.len(), self.limit));
            }
            for ship in &want.fleet {
                check_ship(ship, want.race, lib).map_err(|e| format!("lobby setup row {i}: {e}"))?;
            }
        }
        for (s, want) in sk.slots.iter_mut().zip(&self.slots) {
            (s.kind, s.race, s.team, s.flag) = (want.kind.kind(), want.race, want.team, want.flag);
            s.clear_fleet();
            for (n, ship) in want.fleet.iter().enumerate() {
                s.push_ship(&ship.class);
                s.weapons[n] = ship.guns.clone();
                s.names[n] = ship.name.and_then(|k| lib.names.get(&ship.class)?.get(k).cloned());
                for (post, keys) in s.crew[n].iter_mut().zip(&ship.crew) {
                    post.clone_from(keys);
                }
            }
        }
        for (i, s) in sk.slots.iter().enumerate() {
            if s.kind.live() && sk.flag_taken(i, s.race, s.flag) {
                return Err(format!("lobby setup row {i}: another fleet of its type flies flag {}", s.flag));
            }
        }
        sk.map = None;
        match sk.launch_error() {
            Some(error) => Err(format!("lobby setup cannot launch: {error}")),
            None => Ok(()),
        }
    }
}

/// A picked ship names a hull of its fleet type, guns its banks take (the Arms screen's rule), a name of its hull's list and at most
/// eight crew posts.
fn check_ship(ship: &ShipSetup, race: Race, lib: &Library) -> Result<(), String> {
    let class = lib.classes.iter().position(|c| c.name == ship.class).ok_or_else(|| format!("no ship class {:?}", ship.class))?;
    if !offered(lib, race).contains(&class) {
        return Err(format!("{} is not a {race:?} hull", ship.class));
    }
    let c = &lib.classes[class];
    for (&bank, gun) in &ship.guns {
        if !c.mounts.iter().any(|m| m.bank == bank) {
            return Err(format!("{} has no weapon bank {bank}", ship.class));
        }
        if !lib.guns.contains_key(gun) || !crate::crew_arms::bank_compatible(lib, c, bank, gun) {
            return Err(format!("{} bank {bank} cannot take the gun {gun:?}", ship.class));
        }
    }
    if let Some(k) = ship.name
        && lib.names.get(&ship.class).is_none_or(|p| k >= p.len())
    {
        return Err(format!("{} has no name number {k}", ship.class));
    }
    if ship.crew.len() > 8 || ship.crew.iter().any(|p| p.len() > MAX_CREW_PER_POST) {
        return Err(format!("{}: crew for more than eight posts or too many on one", ship.class));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_json_form_reads_and_writes_alike() {
        let text = r#"{"map":"border_dispute","points":1000,"limit":10,"draw":0,"slots":[{"kind":"human","race":"navy","team":1,"flag":0,"fleet":[{"class":"C","guns":{"3":"G"},"name":2,"crew":[["K"],[]]}]},{"kind":"computer","race":"pirate","team":2,"flag":0},{"kind":"open","team":-1,"flag":0}]}"#;
        let s = SkirmishSetup::parse(text).unwrap();
        assert_eq!(s.slots[0].fleet[0].guns.get(&3).map(String::as_str), Some("G"));
        assert_eq!(s.slots[0].fleet[0].name, Some(2));
        assert_eq!(s.slots[2].race, Race::Unchosen);
        assert_eq!(serde_json::to_string(&s).unwrap(), text);
    }

    #[test]
    fn unknown_fields_and_kinds_are_refused() {
        for bad in [
            r#"{"map":"a","points":300,"limit":5,"slots":[],"speed":3}"#,
            r#"{"map":"a","points":300,"limit":5,"slots":[{"kind":"robot","team":0,"flag":0}]}"#,
            r#"{"map":"a","points":300,"limit":5,"slots":[{"kind":"human","team":0,"flag":0,"name":"Jim"}]}"#,
        ] {
            assert!(SkirmishSetup::parse(bad).is_err(), "{bad}");
        }
    }

    /// A lobby set up by hand (fleet type, a gun swap, a picked name, named crew, a second flag) captures to a setup that sets a
    /// fresh lobby up so that it plans the same battle.
    #[test]
    #[ignore = "needs imported assets"]
    fn a_captured_lobby_plans_the_same_battle() {
        let lib = Library::load(crate::assets_dir());
        let mut sk = crate::menu::tests::skirmish("border_dispute");
        LobbyScenario::parse("map=border_dispute;points=1000;limit=10;slots=human,3*computer,open,closed").unwrap().apply(&mut sk, &lib).unwrap();
        let h = sk.slots.iter().position(|s| s.kind == Kind::Human).unwrap();
        let class = sk.slots[h].fleet[0].clone();
        let pool = lib.names.get(&class).cloned().unwrap_or_default();
        if let Some(name) = pool.get(1) {
            sk.slots[h].set_name(0, name);
        }
        let hull = lib.classes.iter().find(|c| c.name == class).unwrap();
        let swap = hull.mounts.iter().find_map(|m| {
            let mut guns: Vec<&String> = lib.guns.keys().filter(|g| **g != m.gun && crate::crew_arms::bank_compatible(&lib, hull, m.bank, g)).collect();
            guns.sort();
            guns.first().map(|g| (m.bank, (*g).clone()))
        });
        let (bank, gun) = swap.expect("some bank of the hull takes another gun");
        sk.slots[h].weapons[0].insert(bank, gun);
        sk.slots[h].crew_mut(0)[2] = vec!["crew_key".into()];
        sk.slots[h].flag = 3;
        let setup = SkirmishSetup::capture(&sk, &lib).unwrap();
        let text = serde_json::to_string(&setup).unwrap();
        assert!(!text.contains(pool.get(1).map_or("\u{0}", String::as_str)), "names go as numbers: {text}");
        let mut again = crate::menu::tests::skirmish("border_dispute");
        SkirmishSetup::parse(&text).unwrap().apply(&mut again, &lib).unwrap();
        sk.map = None;
        assert_eq!(again.slots, sk.slots);
        assert_eq!(again.plan(&lib), sk.plan(&lib));
        assert_eq!(SkirmishSetup::capture(&again, &lib), Some(setup));
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn setups_this_lobby_would_not_allow_are_refused() {
        let lib = Library::load(crate::assets_dir());
        let mut sk = crate::menu::tests::skirmish("border_dispute");
        LobbyScenario::parse("map=border_dispute;slots=human,computer").unwrap().apply(&mut sk, &lib).unwrap();
        let good = SkirmishSetup::capture(&sk, &lib).unwrap();
        let mut cases: Vec<(SkirmishSetup, &str)> = vec![];
        let mut s = good.clone();
        s.map = "nowhere".into();
        cases.push((s, "no skirmish map"));
        let mut s = good.clone();
        s.slots.pop();
        cases.push((s, "rows"));
        let mut s = good.clone();
        s.slots[0].fleet[0].class = "Nope".into();
        cases.push((s, "no ship class"));
        let mut s = good.clone();
        s.slots[0].fleet[0].guns.insert(99, "x".into());
        cases.push((s, "no weapon bank"));
        let mut s = good.clone();
        s.slots[0].fleet[0].name = Some(9999);
        cases.push((s, "no name number"));
        let mut s = good.clone();
        s.slots[0].kind = SlotKind::Computer;
        cases.push((s, "the human plays"));
        let mut s = good.clone();
        s.points = 55;
        cases.push((s, "steps of 10"));
        for (setup, needle) in cases {
            let err = setup.apply(&mut crate::menu::tests::skirmish("border_dispute"), &lib).unwrap_err();
            assert!(err.contains(needle), "{needle}: {err}");
        }
    }
}
