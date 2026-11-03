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
  viewer sees `1 - L`, so it is gone at full cloak. Starting a cloak prunes the list to the ship's own team (mutual allies); the list
  grows only while fully cloaked.
- Enemies learn of it by chance: a ship of another team within reach spots it with the chance (its lookouts' factor - 1), where the
  factor is the average lookout rating factor (1.075 + 0.05 per rating point, at most 1.5). Ships without a lookout never spot it.
  PROVISIONAL: we roll once a second per watcher, within half the cloaked ship's sight range; the original's tick rate and reach are unresolved.
- A hidden (fully cloaked, unknown) ship is skipped by cursor picking, labels and reticles, contact lists and every computer's
  targeting. A fully cloaked ship is never picked as a gun target at all. Shells and torpedoes that reach it still hurt it.
- Guns that cannot fire while cloaked are held from the first moment of cloaking until the decloak is over; torpedoes and the light
  Gattling are never held.
- Speed and thrust fall to 40% at full cloak (`1 - 0.6 L`). Engine flames and the wake fade with the hull (`1 - L`).

## Effects and sounds
- No distortion shader: the hull is drawn with the alpha above.
- `Cloaking` (5 s) and `DeCloaking` (5 s, always the full 5 s) play at the hull's cloaking point (hull-space (0, 0, -0.3)): a blue bubble
  (mesh morph between `Cloak1_Layer1` and `Cloak2_Layer1`, `magma_1b` texture) whose alpha rises to 0.5 at 2 s and falls away by 5 s.
- `FullyCloaked` plays at the stern-top point (a trail of strength set from speed once, plus the white `Periscope_Layer1` mesh fading
  in over 3 s) and ends when the decloak starts. It is not gated per viewer, so enemies probably still see it.
- `Cloaked_Impact` (3 s, a purple-to-blue flash on the bubble mesh) plays at the cloaking point when a fully cloaked ship is hit.
- Sounds: `IC_Sub_Cloak` when cloaking starts, `IC_Sub_CloakingLoop` (loop) from fully cloaked until the next transition,
  `IC_Sub_UnCloak` when decloaking starts; UI `HUD_Options_CloakStart` / `HUD_Options_CloakStop`.

## Computer submarines (PROVISIONAL)
With the AI flag set (default) and not commanded by the player: once a second, decloak when a hostile is closer than 1.5 x the
longest gun range or none is in contact; cloak when a hostile is in contact beyond that and a cloak-firing gun is fitted. The
original's reveal range value and cadence are unresolved; expect a cloak / decloak cycle.

## Not done / unknown
The decloak by projectile impulse; the downward "dive" offset the original applies to crewed ships while the level changes; what the
section counter that forces a stop is; the detection tick rate; whether an enemy sees the periscope and its trail; live checks of
every timing here.
