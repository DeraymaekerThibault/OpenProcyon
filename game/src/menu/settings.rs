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
    Detail,
    Background,
    Flags,
    RestoreDisplay,
    /// Single Player menu toggle: Normal -> Hard -> Easy.
    Difficulty,
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
    pub detail: u8,
    pub background: bool,
    pub flags: bool,
    /// 0 Easy, 1 Normal (retail default), 2 Hard: scales shell damage between the player's and computer ships.
    pub difficulty: u8,
}
impl Default for Settings {
    fn default() -> Self {
        Settings { master: 10, music: 10, effects: 10, dialogue: 10, fullscreen: false, detail: 2, background: true, flags: true, difficulty: 1 }
    }
}
pub(super) static CHANNELS: [std::sync::atomic::AtomicU8; 4] =
    [std::sync::atomic::AtomicU8::new(10), std::sync::atomic::AtomicU8::new(10), std::sync::atomic::AtomicU8::new(10), std::sync::atomic::AtomicU8::new(10)];
pub(super) static DETAIL: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(2);
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
        let v = Self::path().and_then(|p| crate::data::try_json(&p)).unwrap_or_default();
        let d = Self::default();
        let n = |k: &str, def: u8, max: u8| v[k].as_u64().map_or(def, |x| (x as u8).min(max));
        let b = |k: &str, def: bool| v[k].as_bool().unwrap_or(def);
        Settings {
            master: n("master", d.master, 10),
            music: n("music", d.music, 10),
            effects: n("effects", d.effects, 10),
            dialogue: n("dialogue", d.dialogue, 10),
            fullscreen: b("fullscreen", d.fullscreen),
            detail: n("detail", d.detail, 2),
            background: b("background", d.background),
            flags: b("flags", d.flags),
            difficulty: n("difficulty", d.difficulty, 2),
        }
    }
    pub(super) fn save(&self) {
        let Some(p) = Self::path() else { return };
        let _ = p.parent().map(std::fs::create_dir_all);
        let v = serde_json::json!({"master": self.master, "music": self.music, "effects": self.effects, "dialogue": self.dialogue,
            "fullscreen": self.fullscreen, "detail": self.detail, "background": self.background, "flags": self.flags, "difficulty": self.difficulty});
        if let Err(e) = std::fs::write(p, serde_json::to_string_pretty(&v).unwrap_or_default()) {
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
            Setting::Detail => self.detail = (self.detail + 1) % 3,
            Setting::Background => self.background = !self.background,
            Setting::Flags => self.flags = !self.flags,
            Setting::Difficulty => self.difficulty = (self.difficulty + 1) % 3,
            Setting::RestoreDisplay => {
                let d = Self::default();
                (self.fullscreen, self.detail, self.background, self.flags) = (d.fullscreen, d.detail, d.background, d.flags);
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
                (Setting::Detail, "Effect Detail", ["Low", "Medium", "High"][self.detail.min(2) as usize].into()),
                (Setting::Background, "Render Background", on(self.background)),
                (Setting::Flags, "Flag Waving", on(self.flags)),
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
pub(super) fn apply_settings(settings: Res<Settings>, mut window: Query<&mut Window, With<bevy::window::PrimaryWindow>>) {
    if !settings.is_changed() {
        return;
    }
    for (i, v) in [settings.master, settings.music, settings.effects, settings.dialogue].into_iter().enumerate() {
        CHANNELS[i].store(v, std::sync::atomic::Ordering::Relaxed);
    }
    DETAIL.store(settings.detail, std::sync::atomic::Ordering::Relaxed);
    DIFFICULTY.store(settings.difficulty, std::sync::atomic::Ordering::Relaxed);
    let mode = if settings.fullscreen { bevy::window::WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Current) } else { bevy::window::WindowMode::Windowed };
    for mut w in &mut window {
        if w.mode != mode {
            w.mode = mode;
        }
    }
}

#[cfg(test)]
#[test]
fn master_volume_scales_every_channel_and_zero_mutes() {
    assert_eq!(mix(10, Some(10)), 1.0);
    assert_eq!(mix(5, Some(4)), 0.2);
    assert_eq!(mix(0, Some(10)), 0.0);
    assert_eq!(mix(7, None), 0.7);
}
