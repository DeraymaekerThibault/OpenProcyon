//! Crew data and posting rules: abilities, posts, recruitment and placement.
use super::*;

/// The roster as the JSON object the crew screens (`view.rs`, `fitting.rs`) still index: the typed entries, serialised once.
/// PROVISIONAL shim until those files read `data::roster()` directly.
pub(super) fn crew_data() -> &'static Value {
    static DATA: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    DATA.get_or_init(|| serde_json::to_value(crate::data::roster()).unwrap_or_default())
}
fn member(id: &str) -> Option<&'static crate::data::CrewMember> {
    crate::data::roster().get(id)
}
pub fn ability(id: &str, skill: &str) -> Option<f32> {
    member(id)?.rating(skill).map(|v| (v as f32 / 10.0).clamp(0.0, 1.0))
}
pub fn species(id: &str) -> Option<&str> {
    member(id)?.species.as_deref()
}
/// Talking-head sprite of a named crew member (`TalkingHead_Character_*`).
pub fn portrait(id: &str) -> Option<&str> {
    member(id)?.portrait.as_deref().filter(|p| !p.is_empty())
}
pub(super) fn name(id: &str) -> &str {
    member(id).and_then(|m| m.name.as_deref()).unwrap_or(id)
}

/// The best `skill` among the campaign roster's crew.
pub fn best(points: &CampaignPoints, skill: &str) -> Option<f32> {
    points.crew.iter().filter_map(|c| ability(c, skill)).reduce(f32::max)
}
/// Legacy mission-start saves have availability lists but no post assignments: the roster's best gunner crews every gun.
pub fn accuracy(points: Option<&CampaignPoints>) -> f32 {
    points.and_then(|p| best(p, "gunnery")).map_or(0.5, |g| 1.0 - 0.8 * g)
}

#[derive(Component, Clone, PartialEq)]
pub struct CrewPosts(pub [Vec<String>; 8]);
impl CrewPosts {
    #[cfg(test)]
    pub fn skill(&self, role: usize, skill: &str) -> Option<f32> {
        let values: Vec<_> = self.0[role].iter().filter_map(|id| ability(id, skill)).collect();
        (!values.is_empty()).then(|| values.iter().sum::<f32>() / values.len() as f32)
    }
    /// Gunnery of the gunner in slot `slot` (the order of the ship's selectable banks); a generic hand (0.1) when the slot is empty or hidden.
    pub fn gunner_skill(&self, slot: Option<usize>) -> f32 {
        slot.and_then(|k| self.0[0].get(k)).and_then(|id| ability(id, "gunnery")).unwrap_or(DEFAULT_HAND)
    }
    /// (top-speed, rudder-torque) factors: engineers' engineering and the helmsman's navigation (docs/re/crew.md).
    pub fn handling(&self, caps: &[u32; 8]) -> (f32, f32) {
        (self.post_factor(4, "engineering", caps[4]), self.post_factor(3, "navigation", caps[3]))
    }
    /// Mean of every slot's `skill_factor` (capacity `slots`; empty slots hold a rating-1 hand), as in retail saves: three
    /// engineers rated 9, 9, 8 give (1.5 + 1.5 + 1.475) / 3 / 1.125. A class with no such post at all (`slots` 0, e.g. a Sloop
    /// without engineers) is rated on its captain instead, without the 0.075 a crewman earns: 1 + 0.05 x the captain's rating
    /// (a rating-1 hand when the captain's post is empty: 1.05; 32 of 34 saved ships, docs/re/crew.md).
    pub fn post_factor(&self, role: usize, skill: &str, slots: u32) -> f32 {
        if slots == 0 && self.0[role].is_empty() {
            // The stand-in: the first mate (then the captain) for the helm, fighters and lookout, else the captain.
            let standin = if matches!(role, 3 | 6 | 7) { self.0[2].first().or(self.0[1].first()) } else { self.0[1].first() };
            let rating = standin.and_then(|id| ability(id, skill)).map_or(1.0, |a| a * 10.0);
            return (1.0 + 0.05 * rating) / 1.125;
        }
        let named: Vec<f32> = self.0[role].iter().map(|id| skill_factor(ability(id, skill).unwrap_or(0.1))).collect();
        let slots = slots.max(named.len() as u32).max(1);
        (named.iter().sum::<f32>() + (slots as usize - named.len()) as f32) / slots as f32
    }
    /// The saved crew factor on sight range (absolute, like `ScanFactor`): the lookouts' spotting by the same rule as handling.
    pub fn scan(&self, caps: &[u32; 8]) -> f32 {
        self.post_factor(7, "spotting", caps[7]) * 1.125
    }
    pub fn combat(&self, role: usize, generic: f32) -> Vec<f32> {
        self.0[role].iter().map(|c| ability(c, "combat").unwrap_or(generic)).collect()
    }
}

/// The generic crew of a placed ship's `Crew Skill Level` (docs/re/crew.md): one crewman, rated 3 / 6 / 8 in every skill, sits in
/// every slot of every post (so no stand-in is needed, and the captain's and first mate's Leadership draw the bonuses like any
/// crew's). `Players` (the campaign roster) only marks the player's own ships, which take their posts from the outfit;
/// PROVISIONAL: it reads as Average here, as everywhere else.
pub fn generic_posts(skill: crate::boarding::Skill, caps: &[u32; 8]) -> CrewPosts {
    use crate::boarding::Skill;
    let id = match skill {
        Skill::Green => "Crew_General_Navy_Green",
        Skill::Elite => "Crew_General_Navy_Elite",
        Skill::Average | Skill::Players => "Crew_General_Navy_Average",
    };
    CrewPosts(std::array::from_fn(|r| vec![id.to_string(); caps[r] as usize]))
}

/// The crew a ship fights with when it has no posts of its own: a ship off the player's side carries its level's generic crew.
/// `None` for the player's side (default hands, or the campaign roster, which the caller knows).
pub fn fallback_posts(ship: &crate::Ship, boarding: Option<&crate::boarding::Boardable>, class_name: &str) -> Option<CrewPosts> {
    if ship.side == crate::Side::Player {
        return None;
    }
    Some(generic_posts(boarding?.skill, &crate::boarding::counts(class_name)))
}

/// Leadership (docs/re/crew.md): each ship draws two chances in [0, 1) once; every crewman but the captain and first mate gains 0.1
/// skill when the captain's Leadership (0..1) beats the first, and 0.05 when the first mate's beats the second. The saved speed and
/// rudder factors always carry both (`skill_factor`); this is for gunnery, repairs, fire-fighting and boarding.
pub fn leadership_bonus(key: crate::scenario::SpawnIndex, posts: &CrewPosts) -> f32 {
    let lead = |role: usize| posts.0[role].first().and_then(|id| ability(id, "leadership")).unwrap_or(0.0);
    // PROVISIONAL: fixed per-ship draws from its spawn number, not the original's random generator.
    (if lead(1) > key.draw(1) { 0.1 } else { 0.0 }) + if lead(2) > key.draw(2) { 0.05 } else { 0.0 }
}

/// One crewman's factor on top speed (engineering) or rudder torque (navigation), relative to a rating-1 hand: retail saves
/// store min(1.075 + 0.05 x rating, 1.5) per post (rating 1 -> 1.125, 9 -> 1.5), and speed and turn follow it exactly
/// (docs/re/crew.md "Saved crew factors"). `skill` is rating / 10.
pub fn skill_factor(skill: f32) -> f32 {
    (1.075 + 0.5 * skill).min(1.5) / 1.125
}

/// The skill a post is rated on (`ROLES` order).
pub fn post_skill_name(role: usize) -> &'static str {
    SKILLS[role.min(SKILLS.len() - 1)]
}

/// What a crew member costs in victory points (0 when unknown).
pub fn crew_points(id: &str) -> u32 {
    member(id).and_then(|m| m.points).and_then(|v| u32::try_from(v).ok()).unwrap_or(0)
}

/// The Sort By entry (`SORTS` index) of a post's skill: Captain and First Mate to Leadership, Helmsman to Navigation, Lookout to
/// Spotting, Engineers to Engineering, Riggers to Rigging, Fighters to Combat, Gunners to Gunnery (docs/re/crew.md). PROVISIONAL: selecting
/// a post re-sorting the roster is not in retail.
pub fn sort_for_post(role: usize) -> usize {
    let skill = post_skill_name(role);
    SORTS.iter().position(|s| s.eq_ignore_ascii_case(skill)).unwrap_or(0)
}

/// Seats `id` on `post` of a ship of `class` (the drop rules): a full post gives up its last member, the points must cover the price
/// (a crew member lifted off the post `moved` comes back first). Returns the message to show, empty on success; on a refusal a moved
/// member goes back where they were.
pub(super) fn place_crew(crew: &mut [Vec<String>; 8], class: &str, id: String, post: usize, moved: Option<(usize, usize)>, points: i64) -> String {
    let post = post.min(7);
    let cap = crate::boarding::counts(class)[post] as usize;
    if let Some((r, _)) = moved {
        crew[r.min(7)].retain(|c| c != &id);
    }
    let back = if moved.is_some() { i64::from(crew_points(&id)) } else { 0 };
    let refund = if crew[post].len() >= cap { crew[post].last().map_or(0, |l| i64::from(crew_points(l))) } else { 0 };
    let message = if cap == 0 {
        "This ship has no such post.".to_string()
    } else if i64::from(crew_points(&id)) > points + refund + back {
        "You do not have enough victory points.".to_string()
    } else {
        if crew[post].len() >= cap {
            crew[post].pop();
        }
        crew[post].push(id.clone());
        return String::new();
    };
    if let Some((r, k)) = moved {
        let r = r.min(7);
        let at = k.min(crew[r].len());
        crew[r].insert(at, id);
    }
    message
}

/// Takes the crew member in seat `k` of post `r` off the ship (their points come back); `None` when the seat is empty.
pub(super) fn unpost(crew: &mut [Vec<String>; 8], r: usize, k: usize) -> Option<String> {
    let post = &mut crew[r.min(7)];
    (k < post.len()).then(|| post.remove(k))
}

/// The first post (in `ROLES` order) with a free seat that counts the crew member's best skill: the target of a double-click when no
/// post is selected. PROVISIONAL.
pub fn auto_post(crew: &[Vec<String>; 8], class: &str, id: &str) -> Option<usize> {
    let caps = crate::boarding::counts(class);
    let best = SKILLS.iter().filter_map(|s| ability(id, s).map(|v| (*s, v))).fold(None::<(&str, f32)>, |a, b| a.filter(|a| a.1 >= b.1).or(Some(b)))?.0;
    (0..8).find(|&r| SKILLS[r] == best && (crew[r].len() as u32) < caps[r])
}

pub(super) fn spend(points: &mut CampaignPoints, cost: u32) -> Result<(), String> {
    points.total = points.total.checked_sub(cost).ok_or("Not enough victory points.")?;
    Ok(())
}
pub(super) fn recruit(points: &mut CampaignPoints, id: &str, race: &str) -> Result<(), String> {
    let d = member(id);
    if d.and_then(|m| m.race.as_deref()) != Some(race) || (!points.crew.is_empty() && !points.crew.iter().any(|c| c == id)) {
        return Err("This crew member is not available to this fleet.".into());
    }
    if points.outfit.hired.iter().any(|c| c == id) {
        return Err("Already recruited.".into());
    }
    let price = d.and_then(|m| m.points).and_then(|v| u32::try_from(v).ok()).ok_or("Missing crew price.")?;
    spend(points, price)?;
    points.outfit.hired.push(id.into());
    Ok(())
}
