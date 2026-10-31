//! Lobby scenarios (`--lobby <spec>`, tools/battle_check.py): a skirmish setup written as text and played through the same
//! model the lobby's pull-downs and Launch button use. The `--ship/--allies/--enemies` shortcut skips the lobby, so allied
//! computer slots, full lobbies and big budgets never reached a test through it.
//!
//! Spec: `map=border_dispute;points=1000;limit=10;slots=human,7*computer`
//! - `map`: skirmish map id. `points`: victory points (50-1000, steps of 10). `limit`: fleet size (1-10).
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
}

impl LobbyScenario {
    pub fn parse(spec: &str) -> Result<Self, String> {
        let (mut map, mut points, mut limit, mut slots) = (None, VICTORY_LIMIT.default, FLEET_SIZE.default as usize, vec![]);
        for part in spec.split(';').map(str::trim).filter(|p| !p.is_empty()) {
            let (key, value) = part.split_once('=').ok_or_else(|| format!("lobby spec part {part:?} is not key=value"))?;
            let number = |what: &str| value.trim().parse::<u32>().map_err(|_| format!("lobby spec {what} {value:?} is not a number"));
            match key.trim() {
                "map" => map = Some(value.trim().to_string()),
                "points" => points = number("points")?,
                "limit" => limit = number("limit")? as usize,
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
                other => return Err(format!("lobby spec key {other:?}: expected map, points, limit or slots")),
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
        Ok(Self { map, points, limit, slots })
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
            crate::menu::auto_assign(sk, lib);
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
