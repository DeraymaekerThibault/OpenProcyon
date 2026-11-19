//! Lobby scenarios (`--lobby <spec>`, tools/battle_check.py): a skirmish setup written as text and played through the same
//! model the lobby's pull-downs and Launch button use. The `--ship/--allies/--enemies` shortcut skips the lobby, so allied
//! computer slots, full lobbies and big budgets never reached a test through it.
//!
//! Spec: `map=border_dispute;points=1000;limit=10;slots=human,7*computer` (optionally `;seed=3`, `;fleet=Ship_Navy_ManOWar`)
//! - `map`: skirmish map id. `points`: victory points (50-1000, steps of 10). `limit`: fleet size (1-10).
//! - `seed`: varies the fleets the computers draw (default 0). The game is deterministic, so runs of one spec are identical;
//!   several seeds give several samples.
//! - `fleet`: the human's fleet, ship classes separated by `+` (`Ship_Navy_ManOWar` or `Ship_Navy_Cutter+Ship_Navy_Cutter`), instead
//!   of Auto Assign's. Not checked against the budget here: the lobby's own launch check still applies.
//! - `slots`: the lobby rows in order, `human`, `computer`, `open` or `closed`; `N*kind` repeats. The first row is always the
//!   human's. Rows left out stay as the lobby opens them. Computers take the team and fleet type the lobby gives them.
use super::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotSpec {
    Human,
    Computer,
    Open,
    Closed,
}

#[derive(Resource, Clone, PartialEq, Debug)]
pub struct LobbyScenario {
    pub map: String,
    pub points: u32,
    pub limit: usize,
    pub slots: Vec<SlotSpec>,
    pub seed: u64,
    pub fleet: Vec<String>,
}

impl LobbyScenario {
    pub fn parse(spec: &str) -> Result<Self, String> {
        let (mut map, mut points, mut limit, mut slots, mut seed, mut fleet) = (None, VICTORY_LIMIT.default, FLEET_SIZE.default as usize, vec![], 0, vec![]);
        for part in spec.split(';').map(str::trim).filter(|p| !p.is_empty()) {
            let (key, value) = part.split_once('=').ok_or_else(|| format!("lobby spec part {part:?} is not key=value"))?;
            let number = |what: &str| value.trim().parse::<u32>().map_err(|_| format!("lobby spec {what} {value:?} is not a number"));
            match key.trim() {
                "map" => map = Some(value.trim().to_string()),
                "points" => points = number("points")?,
                "limit" => limit = number("limit")? as usize,
                "seed" => seed = u64::from(number("seed")?),
                "fleet" => fleet = value.split('+').map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect(),
                "slots" => {
                    for item in value.split(',').map(str::trim).filter(|i| !i.is_empty()) {
                        let (count, kind) = item
                            .split_once('*')
                            .map_or(Ok((1, item)), |(n, k)| n.trim().parse::<usize>().map(|n| (n, k.trim())).map_err(|_| format!("lobby spec slot count in {item:?}")))?;
                        let kind = match kind {
                            "human" => SlotSpec::Human,
                            "computer" => SlotSpec::Computer,
                            "open" => SlotSpec::Open,
                            "closed" => SlotSpec::Closed,
                            other => return Err(format!("lobby spec slot {other:?}: expected human, computer, open or closed")),
                        };
                        slots.extend(std::iter::repeat_n(kind, count));
                    }
                }
                other => return Err(format!("lobby spec key {other:?}: expected map, points, limit, seed, fleet or slots")),
            }
        }
        let map = map.ok_or("lobby spec needs map=<id>")?;
        if slots.first() != Some(&SlotSpec::Human) || slots.iter().skip(1).any(|s| *s == SlotSpec::Human) {
            return Err("lobby spec slots: the first row is the human and only that one".into());
        }
        if !(VICTORY_LIMIT.min..=VICTORY_LIMIT.max).contains(&points) || !(points - VICTORY_LIMIT.min).is_multiple_of(VICTORY_LIMIT.step) {
            return Err(format!("lobby spec points {points}: {}-{} in steps of {}", VICTORY_LIMIT.min, VICTORY_LIMIT.max, VICTORY_LIMIT.step));
        }
        if !(FLEET_SIZE.min as usize..=FLEET_SIZE.max as usize).contains(&limit) {
            return Err(format!("lobby spec limit {limit}: {}-{}", FLEET_SIZE.min, FLEET_SIZE.max));
        }
        Ok(Self { map, points, limit, slots, seed, fleet })
    }

    /// Sets the lobby up like a player would: pick the map, move the sliders, turn rows into computers, Auto Assign the human's
    /// fleet. Computer fleets are built when the battle starts (`Skirmish::plan`), exactly as after the Launch button.
    pub fn apply(&self, sk: &mut Skirmish, lib: &Library) -> Result<(), String> {
        let at = sk.maps.iter().position(|m| m.0 == self.map).ok_or_else(|| format!("lobby scenario: no skirmish map {:?}", self.map))?;
        sk.historical = !sk.maps[at].2;
        sk.selected = at;
        sk.load_map();
        sk.victory_limit = self.points;
        sk.fleet_size = self.limit;
        sk.draw = self.seed;
        if self.slots.len() > sk.slots.len() {
            return Err(format!("lobby scenario: {} rows asked, but {} has {}", self.slots.len(), self.map, sk.slots.len()));
        }
        for (i, want) in self.slots.iter().enumerate() {
            let choice = match want {
                SlotSpec::Human => continue,
                SlotSpec::Computer => Choice::Computer,
                SlotSpec::Open => Choice::Open,
                SlotSpec::Closed => Choice::Close,
            };
            sk.choose(Cell::Player(i), choice);
        }
        if sk.open() {
            // The human picks a fleet type when the map leaves it open, then presses Auto Assign.
            if sk.human().is_some_and(|h| h.race == 4) {
                let i = sk.slots.iter().position(|s| s.kind == Kind::Human).unwrap_or(0);
                sk.choose(Cell::Fleet(i), Choice::Fleet(0));
            }
            if self.fleet.is_empty() {
                crate::menu::auto_assign(sk, lib);
            } else if let Some(i) = sk.slots.iter().position(|s| s.kind == Kind::Human) {
                for class in &self.fleet {
                    if !lib.classes.iter().any(|c| &c.name == class) {
                        return Err(format!("lobby scenario: no ship class {class:?}"));
                    }
                    sk.slots[i].push_ship(class);
                }
            }
        }
        sk.map = None;
        match sk.launch_error() {
            Some(error) => Err(format!("lobby scenario cannot launch: {error}")),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_owners_game_parses() {
        let s = LobbyScenario::parse("map=border_dispute; points=1000; limit=10; slots=human,7*computer").unwrap();
        assert_eq!((s.map.as_str(), s.points, s.limit), ("border_dispute", 1000, 10));
        assert_eq!(s.slots.len(), 8);
        assert_eq!(s.slots[0], SlotSpec::Human);
        assert!(s.slots[1..].iter().all(|k| *k == SlotSpec::Computer));
        assert_eq!(s.seed, 0);
        assert_eq!(LobbyScenario::parse("map=border_dispute;seed=3;slots=human,computer").unwrap().seed, 3);
        assert!(s.fleet.is_empty());
        let f = LobbyScenario::parse("map=a;fleet=Ship_Navy_Cutter + Ship_Navy_ManOWar;slots=human,computer").unwrap();
        assert_eq!(f.fleet, ["Ship_Navy_Cutter", "Ship_Navy_ManOWar"]);
    }

    #[test]
    fn bad_specs_say_what_is_wrong() {
        for (spec, needle) in [
            ("points=300;slots=human", "needs map"),
            ("map=a;slots=computer", "first row is the human"),
            ("map=a;slots=human,human", "first row is the human"),
            ("map=a;slots=human,2*pirate", "expected human, computer"),
            ("map=a;points=1005;slots=human", "steps of 10"),
            ("map=a;points=5000;slots=human", "points 5000"),
            ("map=a;limit=11;slots=human", "limit 11"),
            ("map=a;speed=3;slots=human", "key \"speed\""),
            ("map=a;slots=human,x*computer", "slot count"),
            ("map", "not key=value"),
        ] {
            let err = LobbyScenario::parse(spec).unwrap_err();
            assert!(err.contains(needle), "{spec:?}: {err}");
        }
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn a_seed_changes_the_computers_fleet_draw() {
        let lib = Library::load(crate::assets_dir());
        let fleet = |spec: &str| {
            let mut sk = crate::menu::tests::skirmish("border_dispute");
            LobbyScenario::parse(spec).unwrap().apply(&mut sk, &lib).unwrap();
            sk.plan(&lib).into_iter().filter(|g| !g.human).map(|g| g.ships.iter().map(|s| s.0.clone()).collect::<Vec<_>>()).collect::<Vec<_>>()
        };
        let base = "map=border_dispute;points=1000;limit=10;slots=human,7*computer";
        assert_eq!(fleet(base), fleet(&format!("{base};seed=0")), "seed 0 is the usual draw");
        assert_ne!(fleet(base), fleet(&format!("{base};seed=1")));
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn a_full_lobby_plans_allied_and_hostile_computer_fleets() {
        let lib = Library::load(crate::assets_dir());
        let s = LobbyScenario::parse("map=border_dispute;points=1000;limit=10;slots=human,7*computer").unwrap();
        let mut sk = crate::menu::tests::skirmish("border_dispute");
        s.apply(&mut sk, &lib).unwrap();
        let plan = sk.plan(&lib);
        assert_eq!(plan.len(), 8, "every row plays");
        assert_eq!(plan.iter().filter(|g| g.human).count(), 1);
        let allies = plan.iter().filter(|g| !g.human && g.side == crate::Side::Player).count();
        let foes = plan.iter().filter(|g| g.side != crate::Side::Player).count();
        assert!(allies >= 1 && foes >= 1, "Border Dispute is teams: {allies} allied computers, {foes} hostile");
        for g in plan.iter().filter(|g| !g.human) {
            assert!(!g.ships.is_empty(), "computer row {} has a fleet", g.player);
        }
    }
}
