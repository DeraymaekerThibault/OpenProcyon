# WOT extraction scope (schema version 1)

This is a clean-room text reader, not recovered engine code. The observed format
comes from a community snapshot, not a verified official release. Nothing here
establishes executable behavior or units.

## Accepted syntax

- Strict UTF-8 by default; `--encoding` explicitly selects another codec. No lossy
  decoding or automatic fallback. All 559 files in the inspected snapshot decode
  as UTF-8.
- One property per physical line: a name (possibly containing spaces/type words),
  a type token, and a literal. Supported primitives: single-quoted `String`, signed
  decimal `Int`, finite decimal/exponent `Float`, `Bool` (`True`/`False`), and
  `Vector3(x, y, z)`, `Coord(x, y)`, and nine-component `Matrix33(...)`.
  Strings may be empty and run from the first to the last quote on the line;
  retail `World/*.twt` ship-name fields contain literal apostrophes.
  Backslashes are literal; quote escapes/multiline strings have not been established
  and are unsupported.
- Eight decimal digits followed by a block name, then a standalone `{`, nested
  contents, and a standalone `}`. The digit token is retained **without assigning
  or validating its meaning**. Comments begin with `#` on their own lines.
- Ordered nodes preserve blocks, repeated names, comments, raw property text and
  unknown field names. Every property has a root-relative source path and one-based
  physical line number, original literal, decoded value and declared type.
  Blocks retain opening/closing locations. Blank lines are omitted but still count
  toward source line numbers. Raw line endings are normalized by the text reader;
  each document also records the original file's SHA-256.
- Unknown types, malformed literals, unsupported lines and unmatched braces are
  retained/reported with source locations. A file read failure is reported against
  its path with a null line number. Partial results are not silently declared clean.

## Derived interpretation (limited rules, not engine behavior)

`Type String` at top level is the object identifier. Top-level `FactoryType` values
provide classification evidence:

| Factory | Category |
|---|---|
| `ShipCustomInfoFactory` | ship |
| `GunCustomInfoFactory` | gun |
| `ProjectilePhysics`, `TorpedoPhysics`, `MinePhysics` | projectile |
| No recognized signature | other |

Conflicting categories or duplicate identities produce errors. Filename prefixes
are not classification rules: `Ship_*` includes wrecks and debris in this snapshot.
These signatures cover this snapshot, not every possible game/mod version.

- In a block with `AttributeName = GunPlacement`, `AssociationName`'s first
  comma-separated component is recorded as a mounted-gun target. Remaining text is
  kept as `arguments_raw`; its numeric/bank semantics are deliberately unknown.
  Attribute, association and descriptor locations are retained. Links are extracted
  from non-ship objects too (for example armed scenery), not just ships.
- For gun-classified documents, `BulletTypeName_Primary` and
  `BulletTypeName_Secondary` are projectile references. `NoBulletType` is recorded
  as a **sentinel convention** (not executable evidence).
- Exact, case-sensitive matching against object `Type` resolves links. Missing,
  duplicate or wrong-category targets generate warnings and retain the original
  reference. No guessed case folding, inheritance, defaults or missing objects.
- Other association mini-languages (damage sections, effects, engines, etc.) stay
  opaque strings. No property whitelist discards unrecognized names.

## Output and boundaries

The catalog contains `documents` (syntax trees, identities, classification evidence,
relationships), `diagnostics`, `summary`, `encoding`, and `schema_version`.
The parser reads only `WorldObjects/**/*.wot` (case-insensitive extension).
Relative paths/symlinks resolving outside the input root are rejected; input must be
a trusted, stable local snapshot (not a hostile, concurrently changing filesystem).

The CLI exclusively creates a new output **outside both the project and input
root** and refuses overwrite. Exit codes: 0 = no parse errors (inspect warnings),
1 = catalog written with parse errors, 2 = invocation/input/output failure. An empty
scan fails. Catalogs contain original text and must not be committed or published.

Tests use authored synthetic text. Separate opt-in reference tests independently
read the selected files, check typed values and source lines, account for every
nonblank line, and compare file hashes before/after. Passing syntax checks does
not validate movement, combat, AI, count-token semantics, or the game's use of data.

The map/campaign, asteroid, collision and crew exporters reuse this syntax tree.
The catalog itself does not interpret TWT gameplay, meshes, or simulation.
