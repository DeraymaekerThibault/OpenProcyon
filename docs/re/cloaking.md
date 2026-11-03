# Cloaking (the Procyon Submersible)

Sources: retail ship/gun data (`Is Cloakable` is true only on `Ship_Procyon_Submersible`; `Can Be Fired While Cloaked` only on
`Gun_Torpedo` and `Gun_Gattling_Light`), the effects `Cloaking`, `DeCloaking`, `FullyCloaked`, `Cloaked_Impact`, the rule action
`Set Group/Unit Cloak State` (mission 12, group `ProcSubs`) and a static read of the cloak's keeper. Implemented in `game/src/cloak.rs`.

## State machine
- Four states: off, cloaking, fully cloaked, decloaking. Cloak level L: cloaking `elapsed / 5 s`, fully cloaked 1, decloaking falls
  linearly to 0. Cloaking and decloaking take 5 s each. A stop during cloaking decloaks over exactly the time already spent (no jump).
- A start needs a cloakable hull, the off state and at least 5 s since the ship was created or last finished decloaking. That idle gate
  is the only cooldown: no energy, no charge, no maximum duration. A stop is accepted while cloaking or fully cloaked.
- Menu: Start is shown while off or decloaking (greyed until a start is allowed), Stop while cloaking or fully cloaked.
- A decloak is forced by blast damage that hits a section (mortars, mines, star burst) and when the ship is destroyed. A direct hit
  does not decloak it. Firing does not touch the cloak.
- Rule action: TRUE starts a cloak when allowed (nothing happens inside the first 5 s: fire it again later), FALSE stops one that is
  closing or closed; `AI Can Override` is stored whatever the rule says. Every computer submarine may cloak by itself unless a rule
  clears that flag.

## Who sees it
- Each cloaked ship keeps the teams that know it. A viewer whose team knows it sees the hull at `0.33 + 0.67 (1 - L)`; every other
  viewer sees `1 - L`, so it is gone at full cloak. Starting a cloak prunes the list to mutual allies; it is not cleared at decloak
  and never shrinks while fully cloaked.
- There is no periodic detection. An enemy learns of a fully cloaked ship only when it launches a torpedo (the light Gattling never
  counts): for each team not yet in the list, every ship of that team within half its own scan range (class sight x lookout factor) of
  the sub gets one draw with the chance (lookout factor - 1) (0.125 to 0.5 for a crewed ship), and the team is added on the first
  success. Ships without lookouts never spot it.
- Retail seems to add the ship's own and allied teams by the same launch event only, so a fully cloaked sub would be hidden even
  from its owner (alpha 0, no banner, not pickable) until its first torpedo. Medium confidence, so PROVISIONAL: we know the own team
  from the start of the cloak.
- A hidden (fully cloaked, unknown) ship is skipped by cursor picking, labels and reticles, contact lists and every computer's
  targeting. A fully cloaked ship is never picked as a gun target at all. Shells and torpedoes that reach it still hurt it.
- Guns that cannot fire while cloaked are held from the first moment of cloaking until the decloak is over; torpedoes and the light
  Gattling are never held.
- Speed and thrust fall to 40% at full cloak (`1 - 0.6 L`). Engine flames and the wake fade with the hull (`1 - L`).
- The hull sinks: while cloaking the body's height is `-3.65 L`, while decloaking `-3.65 (1 - t / 5 s)` (always over the constant 5 s,
  even after a shortened decloak, so a stop at 2 s jumps to the full dive and leaves the ship part sunk until the next cloak). Hull,
  guns, crew and effects all sit lower; nothing else reads it.

## Effects and sounds
- No distortion shader: the hull is drawn with the alpha above.
- `Cloaking` (5 s) and `DeCloaking` (5 s, always the full 5 s) play at the hull's cloaking point (hull-space (0, 0, -0.3)): a blue bubble
  (mesh morph between `Cloak1_Layer1` and `Cloak2_Layer1`, `magma_1b` texture) whose alpha rises to 0.5 at 2 s and falls away by 5 s.
- `FullyCloaked` plays at the stern-top point (a trail of strength set from speed once, plus the white `Periscope_Layer1` mesh fading
  in over 3 s) and ends when the decloak starts. Effects and sounds are created with no team test, so enemies see the bubble, the
  periscope and the trail and hear the sounds.
- `Cloaked_Impact` (3 s, a purple-to-blue flash on the bubble mesh) plays at the cloaking point when a fully cloaked ship is hit.
- Sounds: `IC_Sub_Cloak` when cloaking starts, `IC_Sub_CloakingLoop` (loop) from fully cloaked until the next transition,
  `IC_Sub_UnCloak` when decloaking starts; UI `HUD_Options_CloakStart` / `HUD_Options_CloakStop`.
- Starmap, minimap and radar do not consult the cloak, so their icons are probably not hidden (inferred, not changed here).

## Forced decloak
Blast damage decloaks a ship as soon as one of its sections lies inside the blast radius (no damage threshold; direct hits never do).
A gravity-charge well decloaks every cloakable ship inside it, every tick; torpedoes and shells never do. A destroyed ship decloaks.
A flagged-section counter also forces a stop and blocks a start; its meaning (probably a destroyed or destroying section) is unresolved.

## Computer submarines
Twice a second per ship (staggered), for computer ships whose rules have not cleared `AI Can Override` (false skips the routine, so the
rule's state sticks): no hostile contact within its scan range (any other team, lifeboats excluded) decloaks it; fully cloaked with a
hostile targeting it decloaks it; otherwise a ship with guns starts a cloak when the gate allows. The original's "reveal when a hostile
is within 1.5 x range" branch reads an unrelated field (about zero), so it never fires and is left out. A sub with no cloak-firing gun
flickers (cloaks, is ordered back out half a second later, waits the 5 s gate). The hunt routine for lobbing guns is not decoded.

## Mission 12 (group `ProcSubs`: Wraith, Shadow, Shade and two unnamed)
World init: Cloak TRUE with AI override FALSE (does nothing: inside the 5 s gate), hidden, holding position. Player enters the parley
volume: visible, Cloak TRUE (the fade starts), aggressive, attack the player. First shot from the subs (a torpedo, so also the first
detection roll): NIS, throttle cap 25%, player invulnerable 10%. After the story speech: +2 s Cloak FALSE (override FALSE), +7 s NIS ends,
Cloak FALSE with override TRUE (the AI may re-cloak them). The end parley volume: Cloak FALSE, override FALSE.

## Not done / unknown
What the flagged-section counter is; whether a shell hits the sunken hull; the hunt routine; live checks of every timing here (own-team
visibility first: launch a Procyon sub in skirmish and read the hull alpha before and after one torpedo).
