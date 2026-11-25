# content/

Files that ship with the code. Everything here must be our own work or carry a licence that allows redistribution
(AGENTS.md rule 7). The game overlays this folder on the imported assets, so a file here replaces the imported file of the same path.

| Path | What | Origin / licence |
|---|---|---|
| `fonts/Chancery.otf` | heading font (stands in for the retail BlackChancery bitmap font) | URW Z003, URW base35 fonts, AGPL-3.0 with font exception |
| `fonts/Sans.otf`, `fonts/SansBold.otf` | body font (stands in for Univers/Immi) | URW Nimbus Sans, URW base35 fonts, AGPL-3.0 with font exception |
| `data/hud/theme.json` | front-end chrome palette and geometry (panels) | hand-written for this project (PROVISIONAL numbers) |
| `data/hud/bar.json` | in-game command bar part order and backdrops | hand-written for this project (PROVISIONAL order) |
| `voice/events.json` | crew voice event → retail dialogue file names | event grouping derived from the community CL ModPack's voice-line folder layout (names only, no audio); the audio is copied from the player's own install at import. The CL ModPack is free to use (confirmed by the maintainer, 2026-11-23, #543), so the grouping may be redistributed. About 290 entries are not read by the game yet (kept) |
| `templates/dialogue.json` | mission template (dialogue block), `docs/modding.md` "Mission templates" | hand-written for this project |
| `templates/cutscene.json` | mission template (cutscene block), `docs/modding.md` "Mission templates" | hand-written for this project |
| `templates/failsafe.json` | mission template (failsafe block), `docs/modding.md` "Mission templates" | hand-written for this project |
| `templates/reach-point.json` | mission template (reach a point), `docs/modding.md` "Mission templates" | hand-written for this project |
| `templates/ambush-on-arrival.json` | mission template (ambush on arrival), `docs/modding.md` "Mission templates" | hand-written for this project |
| `templates/escort-to-gate.json` | mission template (escort to a gate), `docs/modding.md` "Mission templates" | hand-written for this project |
| `templates/sink-group.json` | mission template (sink a group to win), `docs/modding.md` "Mission templates" | hand-written for this project |
