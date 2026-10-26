# content/

Files that ship with the code. Everything here must be our own work or carry a licence that allows redistribution
(AGENTS.md rule 7). The game overlays this folder on the imported assets, so a file here replaces the imported file of the same path.

| Path | What | Origin / licence |
|---|---|---|
| `fonts/Chancery.otf` | heading font (stands in for the retail BlackChancery bitmap font) | URW Z003, URW base35 fonts, AGPL-3.0 with font exception |
| `fonts/Sans.otf`, `fonts/SansBold.otf` | body font (stands in for Univers/Immi) | URW Nimbus Sans, URW base35 fonts, AGPL-3.0 with font exception |
| `data/hud/bar.json` | in-game command bar part order and backdrops | hand-written for this project (PROVISIONAL order) |
| `voice/events.json` | crew voice event → retail dialogue file names | event grouping derived from the community CL ModPack's voice-line folder layout (names only, no audio); the audio is copied from the player's own install at import |
