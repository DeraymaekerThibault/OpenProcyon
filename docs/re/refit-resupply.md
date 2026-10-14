# Tender refit and weapon resupply

Research only; nothing here is implemented. Source: observed behaviour of the original game and its retail data (WOTs, `hud.hdt`, strings, mission-start saves, exported mission JSON). Evidence levels: **[C]** confirmed from the game's behaviour or data, **[I]** inference, **[?]** unresolved.

- Ship flags: bit 0 damageable, bit 1 set on every ship [armed?], bit 2 dockable, bit 3 boardable, bit 8 ship, bit 15 base/island. The dock-point keeper holds "can be docked with" and "repairs when docked with". The world setting is **`IsCampaign`** (serialised next to "Is Alliance Change Allowed").

## 1. Summary (the answers)

1. **There is no finite ammunition.** No gun class field, no gun state field, no save field and no fire-path check carries an ammunition count; guns are limited only by reload timers, arcs, range, crew alert and whether the mount still exists. "Munnitions"/"Ammunition" is only a *critical-system tag* on hull sections (§3.3).
2. **There is no torpedo resupply.** A torpedo launcher is `Gun_Torpedo`: 1 bullet per burst, `TimeBetweenBursts` 35 s, nothing else. Mission 1's "dock with the supply Tender to re-arm / live ammunition" is *narrative*: the "dummy loads" are the script's **invulnerability 1.0** on the training ships, removed by a rule when the group reaches the tender volume. The dock itself repairs and fires rule hooks.
3. **Docking repair is automatic.** Once docked with a keeper that repairs, the repair starts by itself (no extra input beyond the dock order), completes at a fixed *Repair Time* after docking, and the ship is released after the *Repair Effect Time* (or the rule dock time, whichever is longer).
4. **The service is "repair", and in campaign it is a refit of damaged hardware:** it restores hull sections to full hit points; for bases always and for tenders **in campaign (`IsCampaign`)** it also revives destroyed sections and **re-creates every gun mount** (fresh gun objects). It does not change which weapon types are mounted and does not add ammunition. In a skirmish a tender only tops up sections that are damaged but not destroyed.
5. **Weapon changes happen elsewhere:** the Arms screen (`Open Crew and Arms Screens`, `SinglePlayerArmsList`, arms profiles). It is first opened after a story NIS in Mission 3, never in Mission 1.

## 2. Which things provide what

| Provider | Hull repair | Gun/section revival | Weapon change |
|---|---|---|---|
| Tender ships (WOT `Is Tender`: `Ship_Navy_Tender`, `_Feather`, `Ship_Pirate_Tender`, `Ship_Procyon_Tender`) | yes (keeper repairs only when `Is Tender`; rule `Set Group/Unit Dockable` toggles dockability only) | only when the world is a campaign | no |
| Bases/islands (the 23 exported docks, e.g. `Base_Grant`, `Base_Dolinor`) | yes; `Toggle Island Repair When Docked` can disable | **always** | no |
| Any other ship | not dockable/repairing by default | – | no |
| Own crew (engineers) | slow passive repair of damaged sections, see 3.4 | – | no |
| Arms/Crew screens (between NIS and mission segments, missions 3, 5, 7, 9, 11) | – | – | yes, spends campaign points |

[C] for the dock keeper defaults (`docking-and-orders.md`: ctor, ship info ctor, island ctor) and the heal-mode rule in 3.1. The tender's own `Repair Effect Name` (`Repair_Medium`/`Repair_Large`) does not matter for the docker: the **docker's** class picks the times.

## 3. Confirmed behaviour

### 3.1 Docking repair
- Entering the *docked* state, the dock routine (state 3) checks the dockee keeper's "repairs when docked with" flag. If set it runs the repair setup on the docker: stores the start time, creates the scaffold effect(s) (`Repair_*`, chosen from the docker's `Repair Effect Name`), and takes the docker's **Repair Time** and **Repair Effect Time** from `hud.hdt` "Repair Speed Info" by size class: small 4 / 7 s, medium 10 / 15, large 20 / 25, large ironclad 30 / 35 (`Repair_Small` → small; `Repair_Medium`, `Repair_IC_Small` → medium; `Repair_Large`, `Repair_IC_Medium` → large; `Repair_IC_Large` → large ironclad). The routine's dock duration is **the larger of the rule dock time (default 5 s, `Set Dock Time`) and Repair *Effect* Time** (the docked-entry code reads right after the repair setup). [C]
- Every ship info tick: when `now − start ≥ Repair Time` it calls the heal, ends the effect and resets the timer. So the hull is repaired **in one step at Repair Time**, not progressively; the scaffold keeps playing until the Effect Time. [C]
- Heal (all damage sections of the docker) with a mode flag derived from the **dockee**:

  | dockee | world | flag |
  |---|---|---|
  | base/island (flag bit 15) | any | **full** |
  | ship | `IsCampaign` true | **full** |
  | ship | `IsCampaign` false | partial |

  - Per section: if current hp > 0 → set to max. If hp is 0 and the flag is *full* → revive: recreate the section's mesh/effect state and set hp to max. Critical-system hit counters of the section are cleared; its damage effect is stopped.
  - If *full*, and the ship has flag bit 1, every **gun info** entry of the ship is rebuilt: an existing gun object is destroyed and a new gun world object is created from the stored mount name, assigned to the same player, attached to the same mesh node, with accuracy deviation recomputed. [C]
  - Effect: hull sections full; destroyed sections and gun mounts back (new objects, so reload timers, burst state and shots-fired counters start from zero [I from re-creation]); no gun *type* is changed (the name is read from the stored mount entry; whether that entry holds the arms-profile gun is [I]).
- When the docker is the player's selected primary ship the heal also clears HUD warning bytes (session HUD object,, and when *full*) [C that it does; meaning of each byte [?]].
- Release: state 4 waits for the duration, then, frees the dock slot and clears the relation; **a human ship stays where it is**, AI ships move away/turn/return (`docking-and-orders.md`). No player confirmation is needed for repair. [C]

### 3.2 No finite ammunition
- Gun WOTs: the 57 distinct property names of all `Gun_*.wot` contain no capacity/ammo/clip/magazine field. Fire pattern is `BulletsPerBurst`, `DuringBurstReloadTime`, `TimeBetweenBursts`, ranges, arcs, `Victory Point Cost`, `Mount Type`, `Exclusion Race`. `Gun_Torpedo`: 1 per burst, 35 s between bursts, `Victory Point Cost` 7, `Is TorpedoLauncher Gun`. [C]
- Gun runtime state, as serialised (and the real save `Mission 01.twt`): per bullet type `# Bullets Fired`, `Is Firing`, `During Burst Reload Timer`, `Between Burst Timer`; plus `CurrentBulletTypeBeingUsed`, `SwitchToFirstBulletTypeAfterFiringOnce`, `FiringRateDamages`, `AccuracyDeviation`, `Shots Fired Count`, `Mutable Shots Fired Count`, `Gun Exists`. The two counters only ever go **up** (increments both after each muzzle; nothing compares them to a limit). [C]
- The fire routine has no precondition on any count; its only gates are the speech event for the firing crew and the cloak side effects. The "empty bank" case does not exist: a bank is unavailable only when its guns do not exist (destroyed section/mount) or they are still recharging. [C; callers' gating (AI/UI) not exhaustively checked → I for "nothing else gates it"]
- Strings: no ammo text in `hud.hdt`, the HUD sprite list or the English strings except the critical-system label "Ammunition" and Mission 1/9 dialogue; `Strings` "live ammunition" lines are speech only. The one other mention (nova mortar "ammunition left", M09JIM12) is flavour. [C]
- HUD feedback exists only for recharge/destroyed/range: sprite names `WeaponBar_GunBankNotFullyRecharged`, `WeaponBar_GunBankDestroyed`, `WeaponBar_GunBankCanHitTarget` (..`; their consumers were not traced, meaning from names [I]).

### 3.3 "Munnitions" critical system
Hull sections carry a `criticals=` list of system tags (`Cargo Hold`, `Spotter Equitment`, `Power Transformer Engines/Guns`, `Engines`, `MaxVelocity`, `Rudder Linkage`, `Lower Deck Guns`, `Munnitions`, `Cloaking Generator`; names, section parser). A section stores a per-system hit counter (10 ints) and the HUD shows "Critical Systems Hit: … Ammunition". What a Munnitions hit *does* was not found; nothing links it to an ammo count. [?] (Do not infer ammo from the label.)

### 3.4 Passive crew repair (not a tender service)
A crew repair step runs from the ship info update every 12 s, picks damaged sections, scales a chance by the crew engineering skill and adds hit points (clamped to max; cancels the damage effect at full). This is the only non-dock repair found. Amount per tick is unresolved [?]; it is the "damage control" a faithful remake may add later.

### 3.5 Arms and crew
- Rule action `Open Crew and Arms Screens` (`TP::Open_CrewArms_Screens`, execute): if the HUD event queue accepts event `0x14` it calls the big GUI event handler; no state is changed by the rule itself. [C] Used in missions 3 (END NIS1), 5 (Player Undocks @ Grant), 7 (Docking), 9 (End NIS1), 11 (END NIS0); **not in mission 1 or 2**.
- State: `SinglePlayerVictoryPoints`, `SinglePlayerCrewList`, `SinglePlayerArmsList`, `World Arms List` (the guns allowed by the world), arms profile files (`.ap`, `ARMS PROFILE DIRECTORY`, `Number Of Gunbanks`, `GunBank %02i`), `Victory Point Cost` per gun/ship/crew, `Arms Types Available`. Mission-start saves already carry them (`assets/saves/missionNN.json` `points/crew/arms`). The screens (`Arms Selection Screen`, `Arms Information`, `Modify Arms Position`, load/save profile) pick which gun sits in which bank. [C for names and the lists; the exact application of a profile to ships and the points spending [?]]
- This is the "weapon change" path; tenders and bases never open it.

### 3.6 Mission rules and save fields
- Rules that matter: `Set Group/Unit Vulnerability` (`Invulnerable Percent`), `Set Group/Unit Dockable`, `Set Dock Time`, `Toggle Island Repair When Docked`, `Group/Unit Docked`/`Is Docked`/`Has Docked`, `Dock Ships`, `Set Ships Flag Texture`, `Open Crew and Arms Screens`, `Add Victory Points for SinglePlayer`. No rule action or condition mentions ammunition, torpedoes or resupply.
- Save fields: ship info `Repair Time`, `Repair Effect Time`, `Docking Repair Timer`, `Sail Repair Timer`, damage sections' `Curr Hit Points`/`Max Hit Points`/`Hit Count`, dock-point info (`Locked Count`, `Docked Count`, `Can Be Docked With`, `Repair When Docked With`), gun state above, `IsCampaign`, `SinglePlayerArmsList`/`CrewList`. There is **no** ammunition or supply field anywhere.

## 4. Mission 1

- Ships: player `RLS Comet` (`Ship_Navy_Torpedo`, two `Gun_Torpedo` launchers + lancers), small ships Explorer/Crusader/Swift/Lyonesse, tender `RLS Bounty` (group `Weapons Tender`, `Is Tender`, `Repair_Medium`). Mission 1 is a campaign world.
- Training state ("dummy loads"): world-init `Initial Vulnerability & Lifeboats` sets `Invulnerable Percent` 1.0 on Player0 Group, Small Ships and Big Ships. Hits are still counted (`Group/Unit Hit at least x Times`).
- Trigger of "live ammunition": rule `Player Reaches Tender (Speech Event) (Reset Vulnerability)` (line 6079): player *and* small ships in the volume `Player Reaches Tender (Speech Event)`, flags `Battle @ Nebula` and `Congrats Winners Played` → music off, `M01AMELIA11` ("We will be using live ammunition… Dock with the supply Tender to re-arm") and `M01JIM00E`, then **vulnerability 0.0** for Player0 Group and Small Ships. **This happens on volume entry, before docking.** Objective 5 activates *Dock With Tender*.
- Dock: `Jim Has Docked With Tender` (`Group/Unit Docked` Player0 command ship ↔ `Weapons Tender,RLS Bounty`, plus `Congrats Winners Played`) fires on entering the docked state: speech `M01AMELIA12` ("…raw power of your weapons when using live ammunition"), `Set Ships Flag Texture Flag_Navy00.tga` for the small ships and the player, flag `Player Has Docked w/ Weapons Tender`. Objective 6 completes *Dock With Tender*, activates *Destroy Asteroids*; the small ships path to the asteroids after `M01AMELIA12`; the asteroid attack rules require the flag.
- Eligibility: any allied ship can dock with the tender (keeper enabled, allied, dock points); the rules only wait for the player ship.
- Timing: repair completes 4 s after docking (Comet's class is `Repair_Small` [I from class]), release after max(5 s, 7 s) = **7 s**; the player stays where it is when released. Quantity: n/a (no quantity exists). Completion: `Group/Unit Docked` + flag; undocking: free slot, relation removed, ship free to sail.
- Sequence consequence: a player who docks but never entered the volume cannot occur (volume lies at the tender); a player who enters the volume gets live damage even without docking.

## 5. Current runtime vs retail

| Topic | Runtime today | Retail |
|---|---|---|
| Weapon ammo | none (reload timers only) | none: **correct; do not add** |
| Torpedo resupply | `docs/re/campaign.md`/backlog list it as missing | does not exist; remove from the backlog at merge |
| Training invulnerability | `Set Group/Unit Vulnerability` → `MissionShip.invuln`; torpedo hits logged `[training: invulnerable]` | same; verify the tender-volume rule resets it before docking |
| Repair timing | `docking.rs::repair` restores `dt/repair_secs` per frame over 4/10/20/30 s | one-step heal at Repair Time; hull unchanged until then |
| Dock duration | `max(5 s, repair_secs)` (4 → 5 s) | `max(dock time, Repair Effect Time)` (7/15/25/35 s) |
| Destroyed sections/guns | gradual refill revives sections with hp 0; no gun re-creation | campaign tender / any base: revive + recreate guns; skirmish tender: no |
| Core hp | restored with sections | unresolved [?] (not in the traced heal) |
| `IsCampaign` | not represented | selects the heal mode |
| Scaffold effect | one effect at ship centre | per-section jobs unless (campaign ship / base) → one effect |
| Crew passive repair | none | every 12 s, skill-scaled [?] |
| Arms/Crew screens | `Open Crew and Arms Screens` ignored (`rules/eval.rs`) | opens screens in missions 3, 5, 7, 9, 11 |

## 6. Ordered minimal implementation checklist

1. **Do not add ammunition or a Mission 1 resupply step.** Record the finding; adjust the backlog item M1-03 at merge (it is repair/refit semantics + lifeboats, not resupply).
2. **Dock timing** (`docking.rs`): export/keep both hud.hdt times per size class (Repair Time, Repair Effect Time). Duration = `max(rule dock time or 5 s, Effect Time)`; heal at `Repair Time`, single step.
3. **Heal mode**: add `is_campaign` (mission vs skirmish) to the shared state; `full = dockee is base/island || is_campaign`. Partial: set sections with hp > 0 to max. Full: all sections to max (revive destroyed), core hp to max (PROVISIONAL, mark it), then reset every gun state of the docker (cooldown, burst, timers) and make mounts alive again.
4. **Keep the scaffold effect** as is (one effect, PROVISIONAL); extend only if per-section jobs are wanted later.
5. **Tests** (pure): partial vs full, instant heal at Repair Time not before, dock duration per class, base always full, gun state reset.
6. **Mission 1 verification** (no new mission rules): confirm the volume rule drops invulnerability before docking and the dock hooks fire (see §7).
7. Later, separate batches: passive crew repair (12 s tick); `Open Crew and Arms Screens` placeholder in missions 3/5/7/9/11 reading `crew/arms` from the campaign state; arms spending UI.

## 7. Acceptance scenarios

1. **No ammo**: fire any gun for several minutes (including both torpedo launchers); no gun ever refuses to fire for lack of ammunition; torpedo reload stays 35 s; the weapon bar shows only recharge/destroyed/range states.
2. **Mission 1 start**: player and small/big ships take no damage from hits while hits are still counted. After the group reaches the tender volume (before docking) a hit damages them; the log no longer says `[training: invulnerable]`.
3. **Mission 1 dock** (left click and right-click menu): the order is accepted without any extra prompt; docked entry plays `M01AMELIA12` once, applies the flag textures, sets the flag, activates *Destroy Asteroids*; the small ships leave.
4. **Mission 1 repair**: a half-destroyed player (one section at 0, one gun mount lost) docks: hull unchanged during the first 3.9 s, fully repaired at 4 s including the destroyed section and the lost mount; the ship is released at 7 s (not 5 s) and stays in place.
5. **Skirmish tender**: same damage → at Repair Time damaged sections are full, the destroyed one stays destroyed, the lost mount stays lost.
6. **Base dock** (e.g. `Base_Grant`, mission 5): full revival regardless of mode; with `Toggle Island Repair When Docked` off, no repair.
7. **Re-docking** after another fight repeats the repair; a docker that is not repairable (keeper without repair) docks, waits the dock time and nothing is healed.
8. **Save/load round trip**: no ammunition field appears; gun existence and section hp persist.
9. **Arms/Crew** (later batch): missions 3, 5, 7, 9, 11 reach their `Open Crew and Arms Screens` rule and show the placeholder with the saved crew/arms lists; Mission 1 and 2 never do.

## 8. Unresolved questions

1. Whether the core damage-section hit points are restored by the heal.
2. What a `Munnitions` critical hit does, and any non-obvious users of the critical counters.
3. The exact HUD consumers of the weapon-bank state sprites and the meaning of the HUD bytes cleared on heal.
4. The application of an arms profile (`GunBank %02i`) to ships at mission/battle start, the points spending rules, and how the screens pause the game.
5. Whether the stored gun-mount name used by the rebuild is the arms-profile gun (assumed) and whether the rebuilt guns keep AI/target state.
6. Magnitude of the 12 s crew repair tick.
7. Flag bit 1 on islands/bases as *docker* (bases do not dock, so probably irrelevant).

## 9. Evidence index

Retail data read: `hud.hdt` Repair Speed Info; `Gun_*.wot`, `Ship_*.wot` (`Is Tender`, `Repair Effect Name`); `SavedGames/Mission 01/05.twt`, `World/*.twt` (`IsCampaign`); `Strings/English_GameStrings.txt` (label `IDGS_TPCRITICALSYSTEM_MUNITIONS`).
