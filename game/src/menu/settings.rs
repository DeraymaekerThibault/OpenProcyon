//! Sound and display options (`Settings`, saved as `settings.json` in the user folder) and the values code reads from them.
use bevy::prelude::*;

/// One adjustable option of the Sound / Display screens.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Setting {
    /// Volume channels (retail Volume Overall / Music / Effect / Crew Dialogue), stepped by 10% and wrapping.
    Master,
    Music,
    Effects,
    Dialogue,
    /// Video mode (windowed / fullscreen), effect detail (low / medium / high), render background, flag waving.
    Fullscreen,
    /// Not retail: vsync on / off, anti-aliasing (off / SMAA / MSAA 4x).
    Vsync,
    AntiAlias,
    Detail,
    Background,
    Flags,
    RestoreDisplay,
    /// Single Player menu toggle: Normal -> Hard -> Easy.
    Difficulty,
    /// Not retail: Display screen, whether the console key opens the developer console (rules/console.rs).
    Console,
}

/// Sound and display options, saved as `settings.json` in the user folder. PROVISIONAL: the retail sound channel/detail options
/// and texture/model detail have no remake equivalent; effect detail scales particle emission (Low 50%, Medium 75%, High 100%).
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Settings {
    pub master: u8,
    pub music: u8,
    pub effects: u8,
    pub dialogue: u8,
    pub fullscreen: bool,
    pub vsync: bool,
    /// 0 off, 1 SMAA, 2 MSAA 4x.
    pub aa: u8,
    pub detail: u8,
    pub background: bool,
    pub flags: bool,
    /// 0 Easy, 1 Normal (retail default), 2 Hard: scales shell damage between the player's and computer ships.
    pub difficulty: u8,
    /// The developer console may be opened with its key (backquote by default) / F12. Off by default.
    pub console: bool,
    /// The console's `fps` command: the frame-rate counter is shown. Off by default.
    pub fps: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Settings {
            master: 10,
            music: 10,
            effects: 10,
            dialogue: 10,
            fullscreen: false,
            vsync: true,
            aa: 1,
            detail: 2,
            background: true,
            flags: true,
            difficulty: 1,
            console: false,
            fps: false,
        }
    }
}
pub(super) static CHANNELS: [std::sync::atomic::AtomicU8; 4] =
    [std::sync::atomic::AtomicU8::new(10), std::sync::atomic::AtomicU8::new(10), std::sync::atomic::AtomicU8::new(10), std::sync::atomic::AtomicU8::new(10)];
pub(super) static DETAIL: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(2);
pub(super) static AA: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(1);
pub(super) static VSYNC: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
/// The anti-aliasing option: 0 off, 1 SMAA, 2 MSAA 4x.
pub fn anti_alias() -> u8 {
    AA.load(std::sync::atomic::Ordering::Relaxed)
}
/// The vsync option.
pub fn vsync() -> bool {
    VSYNC.load(std::sync::atomic::Ordering::Relaxed)
}
pub(super) static CONSOLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The "Developer Console" option (`--console`, `PROCYON_CONSOLE` and debug builds turn the console on without it).
pub fn console_setting() -> bool {
    CONSOLE.load(std::sync::atomic::Ordering::Relaxed)
}
pub(super) static FPS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Whether the frame-rate counter is shown (the console's `fps`; saved in `settings.json`).
pub fn fps_setting() -> bool {
    FPS.load(std::sync::atomic::Ordering::Relaxed)
}
pub(super) static DIFFICULTY: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(1);
/// The difficulty option: 0 Easy, 1 Normal, 2 Hard.
pub fn difficulty() -> u8 {
    DIFFICULTY.load(std::sync::atomic::Ordering::Relaxed)
}
/// Volume factor 0..1 of a channel (`Setting::Music`, `Effects` or `Dialogue`) times the master volume, for code that spawns
/// sounds or sets a playing sound's volume. Not Bevy's global volume: that only reaches sounds when they start.
pub fn volume(c: Setting) -> f32 {
    let i = match c {
        Setting::Music => 1,
        Setting::Effects => 2,
        Setting::Dialogue => 3,
        _ => 0,
    };
    let level = |i: usize| CHANNELS[i].load(std::sync::atomic::Ordering::Relaxed);
    mix(level(0), (i > 0).then(|| level(i)))
}
/// Master level times a channel level (0..10 each; no channel: the master alone), as a 0..1 factor.
fn mix(master: u8, channel: Option<u8>) -> f32 {
    master as f32 / 10.0 * channel.map_or(1.0, |c| c as f32 / 10.0)
}
/// Particle emission factor of the effect detail option.
pub fn detail_scale() -> f32 {
    [0.5, 0.75, 1.0][DETAIL.load(std::sync::atomic::Ordering::Relaxed).min(2) as usize]
}
impl Settings {
    pub(super) fn path() -> Option<std::path::PathBuf> {
        Some(crate::campaign::user_dir()?.join("settings.json"))
    }
    pub fn load() -> Self {
        Self::from_json(&Self::path().and_then(|p| crate::data::try_json(&p)).unwrap_or_default())
    }
    /// Out-of-range numbers clamp to the option's maximum (a plain `as u8` would wrap 256 to 0); wrong types fall back to the default.
    fn from_json(v: &serde_json::Value) -> Self {
        let d = Self::default();
        let n = |k: &str, def: u8, max: u8| v[k].as_u64().map_or(def, |x| x.min(max as u64) as u8);
        let b = |k: &str, def: bool| v[k].as_bool().unwrap_or(def);
        Settings {
            master: n("master", d.master, 10),
            music: n("music", d.music, 10),
            effects: n("effects", d.effects, 10),
            dialogue: n("dialogue", d.dialogue, 10),
            fullscreen: b("fullscreen", d.fullscreen),
            vsync: b("vsync", d.vsync),
            aa: n("aa", d.aa, 2),
            detail: n("detail", d.detail, 2),
            background: b("background", d.background),
            flags: b("flags", d.flags),
            difficulty: n("difficulty", d.difficulty, 2),
            console: b("console", d.console),
            fps: b("fps", d.fps),
        }
    }
    pub(crate) fn save(&self) {
        let Some(p) = Self::path() else { return };
        let v = serde_json::json!({"master": self.master, "music": self.music, "effects": self.effects, "dialogue": self.dialogue,
            "fullscreen": self.fullscreen, "vsync": self.vsync, "aa": self.aa, "detail": self.detail, "background": self.background, "flags": self.flags, "difficulty": self.difficulty, "console": self.console, "fps": self.fps});
        if let Err(e) = crate::data::write_atomic(&p, &serde_json::to_string_pretty(&v).unwrap_or_default()) {
            warn!("could not save settings: {e}");
        }
    }
    /// One click on an option: volumes step +10% (wrapping to 0), detail cycles, toggles flip.
    pub fn step(&mut self, s: Setting) {
        let up = |v: &mut u8| *v = if *v >= 10 { 0 } else { *v + 1 };
        match s {
            Setting::Master => up(&mut self.master),
            Setting::Music => up(&mut self.music),
            Setting::Effects => up(&mut self.effects),
            Setting::Dialogue => up(&mut self.dialogue),
            Setting::Fullscreen => self.fullscreen = !self.fullscreen,
            Setting::Vsync => self.vsync = !self.vsync,
            Setting::AntiAlias => self.aa = (self.aa + 1) % 3,
            Setting::Detail => self.detail = (self.detail + 1) % 3,
            Setting::Background => self.background = !self.background,
            Setting::Flags => self.flags = !self.flags,
            Setting::Difficulty => self.difficulty = (self.difficulty + 1) % 3,
            Setting::Console => self.console = !self.console,
            Setting::RestoreDisplay => {
                let d = Self::default();
                (self.fullscreen, self.vsync, self.aa, self.detail, self.background, self.flags) = (d.fullscreen, d.vsync, d.aa, d.detail, d.background, d.flags);
            }
        }
    }
    /// (label, value) rows a view shows for the Sound or Display screen.
    pub fn rows(&self, display: bool) -> Vec<(Setting, &'static str, String)> {
        let pct = |v: u8| format!("{}%", v as u32 * 10);
        let on = |b: bool| if b { "On" } else { "Off" }.to_string();
        if display {
            vec![
                (Setting::Fullscreen, "Video Mode", if self.fullscreen { "Fullscreen" } else { "Windowed" }.into()),
                (Setting::Vsync, "VSync", on(self.vsync)),
                (Setting::AntiAlias, "Anti-Aliasing", ["Off", "SMAA", "MSAA 4x"][self.aa.min(2) as usize].into()),
                (Setting::Detail, "Effect Detail", ["Low", "Medium", "High"][self.detail.min(2) as usize].into()),
                (Setting::Background, "Render Background", on(self.background)),
                (Setting::Flags, "Flag Waving", on(self.flags)),
                (Setting::Console, "Developer Console", on(self.console)),
            ]
        } else {
            vec![
                (Setting::Master, "Volume Overall", pct(self.master)),
                (Setting::Music, "Volume Music", pct(self.music)),
                (Setting::Effects, "Volume Effect", pct(self.effects)),
                (Setting::Dialogue, "Volume Crew Dialogue", pct(self.dialogue)),
            ]
        }
    }
}

/// Pushes the options into the running app: volume levels, window mode, detail.
pub(super) fn apply_settings(settings: Res<Settings>, vtime: Res<Time<Virtual>>, mut window: Query<&mut Window, With<bevy::window::PrimaryWindow>>) {
    if !settings.is_changed() {
        return;
    }
    for (i, v) in [settings.master, settings.music, settings.effects, settings.dialogue].into_iter().enumerate() {
        CHANNELS[i].store(v, std::sync::atomic::Ordering::Relaxed);
    }
    DETAIL.store(settings.detail, std::sync::atomic::Ordering::Relaxed);
    DIFFICULTY.store(settings.difficulty, std::sync::atomic::Ordering::Relaxed);
    CONSOLE.store(settings.console, std::sync::atomic::Ordering::Relaxed);
    FPS.store(settings.fps, std::sync::atomic::Ordering::Relaxed);
    AA.store(settings.aa, std::sync::atomic::Ordering::Relaxed);
    VSYNC.store(settings.vsync, std::sync::atomic::Ordering::Relaxed);
    let present = crate::present_mode(crate::profiling(), vtime.relative_speed());
    for mut w in &mut window {
        let mode = window_mode(settings.fullscreen, w.visible);
        if w.mode != mode {
            w.mode = mode;
        }
        if w.present_mode != present {
            w.present_mode = present;
        }
    }
}

/// The video mode option, except that a hidden tool run (`--hidden`) stays windowed: a saved fullscreen option must not show or
/// resize a window that is meant to stay off the desktop.
fn window_mode(fullscreen: bool, visible: bool) -> bevy::window::WindowMode {
    if fullscreen && visible { bevy::window::WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Current) } else { bevy::window::WindowMode::Windowed }
}

#[cfg(test)]
#[test]
fn a_hidden_window_ignores_the_fullscreen_option() {
    use bevy::window::WindowMode;
    assert_eq!(window_mode(true, true), WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Current));
    assert_eq!(window_mode(false, true), WindowMode::Windowed);
    assert_eq!(window_mode(true, false), WindowMode::Windowed);
}

#[cfg(test)]
#[test]
fn master_volume_scales_every_channel_and_zero_mutes() {
    assert_eq!(mix(10, Some(10)), 1.0);
    assert_eq!(mix(5, Some(4)), 0.2);
    assert_eq!(mix(0, Some(10)), 0.0);
    assert_eq!(mix(7, None), 0.7);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_of_range_numbers_clamp_instead_of_wrapping() {
        // 256 and 300 would wrap to 0 and 44 through `as u8`; a negative or fractional value is not an unsigned integer: default.
        let s = Settings::from_json(&serde_json::json!({"master": 256, "music": 300, "effects": 11, "dialogue": -1, "aa": 4294967296u64, "detail": 1.5}));
        let d = Settings::default();
        assert_eq!((s.master, s.music, s.effects, s.dialogue, s.aa, s.detail), (10, 10, 10, d.dialogue, 2, d.detail));
        assert_eq!(Settings::from_json(&serde_json::json!({"master": 7})).master, 7);
    }
}
