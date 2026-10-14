# Pre-rendered movies

Sources: read-only `Movies/` in the install (`PROCYON_GAME_ROOT`), `ffprobe`/`ffmpeg` on this machine, `AppSettings.ini`, `Screens/Movie_Player.tst`,
World TWT headers, and observed behaviour of the original game, described in our own words. Uncertain parts are PROVISIONAL.

## 1. Files and format

Eight files, hi-res/low-res pairs of four movies. Container and codec: **RAD Bink 1** (`BIKi` magic, `.bik`), video codec `binkvideo`, audio
`binkaudio_rdft` (the two logos) or `binkaudio_dct` (the long movies), 44.1 kHz stereo. The game itself uses the RAD runtime `binkw32.dll`
shipped in the install (API names in the exe: `BinkOpen`, `BinkOpenDirectSound`, `BinkSetSoundSystem`, `BinkDoFrame`, `BinkCopyToBuffer`,
`BinkNextFrame`, `BinkWait`, `BinkGoto`, `BinkSetVolume`, `BinkGetSummary`, `BinkDX8SurfaceType`). The remake must not run it.

| file | size | video | fps | length | audio |
|---|---|---|---|---|---|
| `Disney_Logo.bik` | 0.95 MB | 640x480 | 29.97 | 7.11 s | rdft |
| `BD_Logo.bik` (Barking Dog) | 0.99 MB | 640x480 | 29.97 | 7.87 s | rdft |
| `SinglePlayer_NIS.bik` | 39.2 MB | 640x480 | 29.97 (727350/24269 = 29.97) | 161.79 s | dct |
| `TP_endNIS.bik` | 10.0 MB | 640x480 | 29.97 (30240/1009) | 42.04 s | dct |
| `*_LoRez.bik` of each | 0.56 / 0.60 / 23.5 / 6.0 MB | 320x240 | same | same | same |

Content (frame sampling): `SinglePlayer_NIS` is the campaign intro cinematic ("Disney Interactive and Barking Dog Studios present", the
Procyon ships, the "Treasure Planet: Battle at Procyon" title card, then Jim's cutter in the orange cloud sky that Mission 1 starts in; its
2:26 frame matches the settlement frame in `docs/reference-video.md`). `TP_endNIS` is the campaign finale (a flagship firing, pirates
in asteroid fields, a destroyed ship, the golden planet). All content is **letterboxed**: the active picture is 640x352 inside the 640x480 frame
(`cropdetect` gives `crop=640:352:0:64`, identical for both long movies); the 64-pixel bars top and bottom are black.

**Local tooling**: system `ffmpeg`/`ffprobe` decode Bink natively (decoders `binkvideo`, `binkaudio_dct`, `binkaudio_rdft`, demuxer `bink`) and
encode Theora (`libtheora`), VP8/VP9, H.264, MJPEG, Vorbis, Opus. Python tools already shell out to `ffprobe`
(`tools/export_campaign_media.py`). `ffmpeg` is **not** yet a documented dependency of `tools/remote_setup.sh` / cloud runs.

## 2. How the exe plays them

- A front-end screen `MoviePlayer Screen` (`Movie_Player.tst`: a full-screen `Movie Player Screen Backdrop` and a full-screen
  `Movie Player Skip Button`, both 800x600 reference area) owns playback (`TP::MovieScreen`, `Render::MoviePlayer`: log strings
  "MoviePlayer - Create/Start/Pause/Stop/SetState"). A click or key on the skip button ends it; which input exactly is PROVISIONAL.
- Display: render option `Movie Fullscreen DisplayInfo` = 640x480x32 and `Scale Movies Bool False`: movies are shown at native size (the
  display switches to 640x480) rather than stretched. `AppSettings.ini` has `Skip Movies: 0` (developer switch to skip them all).
- File selection: the exe holds all eight names as constants. The front-end state machine (~70 states) chooses the hi-res or the
  LoRez name from a flag in the app-settings object (offset 0x64; set when a hardware probe at the first logo state fails: PROVISIONAL meaning).
  Mapping state -> movie, from the code that references each name: state 0x1a = `Disney_Logo`, state 4 = `BD_Logo` (startup order Disney then
  Barking Dog, because only the Disney state computes the LoRez flag), state 0x42 = `SinglePlayer_NIS`, state 0x43 = `TP_endNIS`.
  The state 0x43 handler also resets the campaign mode fields and the world UI state (it ends the game session before the movie).
- Triggers (PROVISIONAL, not traced to the caller): logos at application start; `SinglePlayer_NIS` when a **new campaign is started** from the
  SinglePlayer menu, before Mission 1 loads (consistent with the reference video: title card at 2:43, then the mission's own NIS);
  `TP_endNIS` when the campaign ends. The World header flag `PlayEndMovie Bool` is `True` only in `Mission12.twt` (all other 28 worlds, including
  Mission 1 and the demo-ending Mission 13, are `False`), so the end movie plays after Mission 12's victory.
- The exe can also record gameplay to AVI/TGA (`MovieCapture`); irrelevant to the remake.

## 3. Conversion and playback options

Constraints: movies would be converted at import from the player's own install, never shipped; Bevy 0.20 here has no video support; the dependency set is
small (`bevy` with `vorbis` audio, `serde_json`). Bink has no pure-Rust decoder in common use.

Measured trials (this machine): Theora quality 6 + Vorbis quality 4 with the bars cropped (`-vf crop=640:352:0:64`):
`SinglePlayer_NIS` 21 MB (encode took ~4 min), `TP_endNIS` 5.8 MB (uncropped), logos 0.43-0.48 MB each. Total about 28 MB for all four movies at the
original resolution (skip the LoRez files). Separated Vorbis audio for the end movie was 0.5 MB, so audio is a minor part. Decoding the Theora
file to raw RGBA through an `ffmpeg` pipe ran at roughly 280 fps (2000 frames in 7 s), far faster than real time.

Options:
1. **Pre-convert at asset-build time, decode with `ffmpeg` at run time** (recommended): `tools/export_movies.py` writes `assets/movies/<name>.ogv`
   (Theora/Vorbis) plus `<name>.ogg` (audio only, for Bevy's Vorbis decoder) and a tiny `movies.json` (name, width, height, fps, frame count,
   crop). The game spawns `ffmpeg -i <ogv> -f rawvideo -pix_fmt rgba -` as a child process and streams frames into one `Image` (640x352).
   Drawback: requires `ffmpeg` at run time (add to `tools/remote_setup.sh` and `docs/development.md`; fall back to "skip movie" if missing, like
   retail's `Skip Movies`).
2. Frame sequence: pre-extract JPEG/WebP frames and play them from disk. No run-time dependency, but 3880+1260+2x230 frames at about 25 KB is ~130 MB of
   tracked assets. Rejected unless the frame rate is halved.
3. Link `ffmpeg-next`/`libav*` or an AV1/VP9 crate: no external process, but heavy native build dependencies that complicate the headless/remote builds.

Timing: use the audio clock as master (Bevy's `AudioSink::position()` if available in 0.20, else a wall clock started with the audio) and drop or hold
frames to match; frame period is 1001/30000 s. Draw centred and aspect-preserved 4:3 -> window (retail draws unscaled 640x480; with a larger window,
letterbox with black; keep the picture at the original 16:11 active area).

## Implementation plan

1. `tools/export_movies.py <game_root> <assets_dir>` (new, wired into `tools/build_assets.sh`): for `Disney_Logo`, `BD_Logo`, `SinglePlayer_NIS`,
   `TP_endNIS` run `ffmpeg -i X.bik -vf crop=640:352:0:64 -c:v libtheora -q:v 6 -c:a libvorbis -q:a 4 assets/movies/x.ogv` and
   `-vn -c:a libvorbis assets/movies/x.ogg`; write `assets/movies/movies.json` using `ffprobe` (fps, duration). Add a Python unit test that parses
   `movies.json` and skips cleanly when the install is absent (pattern used by existing tests).
2. `game/src/movie.rs` (new module, add to the table in `docs/development.md`): `Movie` resource (name, child process, current frame `Image`, start time, audio
   entity), `play(name)` API, skip on Esc/Space/click (reuse the "skip" behaviour of `briefing.rs`), a full-screen black `Node` with the image,
   `MovieFinished` message. A unit test for the raw-frame reader (feed a synthetic byte stream), plus a runtime smoke test (`--script movie` screenshot at frame N).
3. Hook points: front end start (`menu.rs`): Disney logo, Barking Dog logo (skippable), once per app start; "Start New Campaign" plays
   `SinglePlayer_NIS` before `campaign::load_id("mission01")` (the Mission 1 opening NIS stays in `briefing.rs`); `rules.rs`/`campaign.rs`: after the last
   campaign mission win (the `PlayEndMovie` flag; export it into `assets/maps/missionNN/mission.json` in `tools/export_campaign.py`) play `TP_endNIS`, then return to the menu.
   Honour a `--no-movies` flag and the retail `Skip Movies` setting.
4. Docs: after the code lands, update `docs/reference-video.md` ("Next implementation batch" item 1) and `docs/re/campaign.md` ("Remaining fidelity gaps").
5. Open RE questions: exact skip inputs, whether the end movie plays before or after the stats screen, what the logo-state probe tests (LoRez choice).
