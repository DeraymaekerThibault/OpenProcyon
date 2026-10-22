//! First-run "Import your game" screen (state and input in `setup.rs`). There is no imported art yet, so it is drawn from the
//! `content/` fonts and plain boxes; nothing here is retail art or branding.
#![allow(deprecated)] // shortcut: bevy 0.20 Interaction (deprecated for picking Hovered/Pressed); port when it is removed
use bevy::prelude::*;

use crate::setup::{Phase, Setup, SetupAction, SetupInput};

const INK: Color = Color::srgb(0.92, 0.9, 0.84);
const DIM: Color = Color::srgb(0.62, 0.64, 0.7);
const GOLD: Color = Color::srgb(1.0, 0.8, 0.4);
const GOOD: Color = Color::srgb(0.55, 0.85, 0.55);
const BAD: Color = Color::srgb(1.0, 0.5, 0.4);
const PANEL: Color = Color::srgb(0.07, 0.09, 0.16);
const FIELD: Color = Color::srgb(0.02, 0.03, 0.06);
const EDGE: Color = Color::srgb(0.3, 0.34, 0.45);
const BUTTON: Color = Color::srgb(0.16, 0.2, 0.32);
const BUTTON_HOVER: Color = Color::srgb(0.26, 0.32, 0.5);
const OFF: Color = Color::srgb(0.11, 0.12, 0.16);

pub fn plugin(app: &mut App) {
    app.add_systems(Update, (build.after(SetupInput), (hover, blink).after(build)));
}

#[derive(Component)]
struct Root;
#[derive(Component)]
struct Caret;
/// The resting fill of a button (hover brightens it).
#[derive(Component)]
struct Rest(Color);

struct Fonts {
    body: Handle<Font>,
    bold: Handle<Font>,
}

fn label(fonts: &Fonts, s: impl Into<String>, size: f32, colour: Color, bold: bool) -> (Text, TextFont, TextColor) {
    let font = if bold { fonts.bold.clone() } else { fonts.body.clone() };
    (Text::new(s), TextFont { font: font.into(), font_size: FontSize::Px(size), ..default() }, TextColor(colour))
}

fn button(c: &mut ChildSpawnerCommands, fonts: &Fonts, text: &str, action: SetupAction, enabled: bool) {
    let fill = if enabled { BUTTON } else { OFF };
    let node = Node {
        padding: UiRect::axes(Val::Px(18.0), Val::Px(8.0)),
        border: UiRect::all(Val::Px(1.0)),
        border_radius: BorderRadius::all(Val::Px(4.0)),
        justify_content: JustifyContent::Center,
        ..default()
    };
    let mut b = c.spawn((node, BackgroundColor(fill), BorderColor::all(EDGE)));
    if enabled {
        b.insert((Interaction::None, action, Rest(fill)));
    }
    b.with_children(|b| {
        b.spawn(label(fonts, text, 17.0, if enabled { INK } else { DIM }, true));
    });
}

fn row(gap: f32) -> Node {
    Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(gap), align_items: AlignItems::Center, ..default() }
}

fn build(mut commands: Commands, setup: Res<Setup>, assets: Res<AssetServer>, roots: Query<Entity, With<Root>>) {
    if !setup.is_changed() {
        return;
    }
    for e in &roots {
        commands.entity(e).despawn();
    }
    let fonts = Fonts { body: assets.load("fonts/Sans.otf"), bold: assets.load("fonts/SansBold.otf") };
    let screen = Node { width: Val::Percent(100.0), height: Val::Percent(100.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() };
    let panel = Node {
        width: Val::Px(760.0),
        max_width: Val::Percent(94.0),
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(14.0),
        padding: UiRect::all(Val::Px(28.0)),
        border: UiRect::all(Val::Px(1.0)),
        border_radius: BorderRadius::all(Val::Px(8.0)),
        ..default()
    };
    commands.spawn((Root, screen)).with_children(|s| {
        s.spawn((panel, BackgroundColor(PANEL), BorderColor::all(EDGE))).with_children(|p| {
            p.spawn(label(&fonts, "Import your game", 34.0, GOLD, true));
            p.spawn(label(
                &fonts,
                format!(
                    "OpenProcyon plays with your own copy of Treasure Planet: Battle at Procyon. It converts the game's files once \
                     into {} (a minute or two). Your install is only read, never changed.",
                    setup.out.display()
                ),
                16.0,
                INK,
                false,
            ));
            match &setup.phase {
                Phase::Choose => choose(p, &fonts, &setup),
                Phase::Importing { step, total, name } => importing(p, &fonts, &setup, *step, *total, name),
                Phase::Failed { step, reason } => failed(p, &fonts, &setup, step.as_deref(), reason),
                Phase::Starting => {
                    p.spawn(label(&fonts, "Import complete. Starting the game...", 20.0, GOOD, true));
                }
            }
        });
    });
}

fn choose(p: &mut ChildSpawnerCommands, fonts: &Fonts, setup: &Setup) {
    if setup.found.is_empty() {
        p.spawn(label(fonts, "No Steam install found on this computer. Type or paste the game folder below, or drop it on this window.", 15.0, DIM, false));
    } else {
        p.spawn(label(fonts, "Found on this computer:", 15.0, DIM, false));
        for (n, dir) in setup.found.iter().enumerate() {
            p.spawn(row(10.0)).with_children(|r| button(r, fonts, &dir.display().to_string(), SetupAction::Pick(n), true));
        }
    }
    p.spawn(label(fonts, "Game folder", 15.0, DIM, true));
    p.spawn(row(10.0)).with_children(|r| {
        let field = Node {
            flex_grow: 1.0,
            min_height: Val::Px(36.0),
            padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(4.0)),
            align_items: AlignItems::Center,
            overflow: Overflow::clip(),
            ..default()
        };
        r.spawn((field, BackgroundColor(FIELD), BorderColor::all(if setup.check.is_ok() { GOOD } else { EDGE }))).with_children(|f| {
            f.spawn(label(fonts, &setup.path, 16.0, INK, false));
            f.spawn((Caret, label(fonts, "|", 16.0, GOLD, false)));
        });
        button(r, fonts, "Paste", SetupAction::Paste, true);
    });
    match &setup.check {
        Ok(dir) => p.spawn(label(fonts, format!("Game found: {}", dir.display()), 15.0, GOOD, false)),
        Err(why) => p.spawn(label(fonts, why, 15.0, BAD, false)),
    };
    p.spawn(Node { justify_content: JustifyContent::SpaceBetween, margin: UiRect::top(Val::Px(8.0)), ..row(10.0) }).with_children(|r| {
        button(r, fonts, "Quit", SetupAction::Quit, true);
        button(r, fonts, "Import", SetupAction::Import, setup.check.is_ok());
    });
}

fn bar(p: &mut ChildSpawnerCommands, fraction: f32, colour: Color) {
    let outer = Node { height: Val::Px(18.0), border: UiRect::all(Val::Px(1.0)), border_radius: BorderRadius::all(Val::Px(3.0)), ..default() };
    p.spawn((outer, BackgroundColor(FIELD), BorderColor::all(EDGE))).with_children(|o| {
        o.spawn((Node { width: Val::Percent(100.0 * fraction.clamp(0.0, 1.0)), height: Val::Percent(100.0), ..default() }, BackgroundColor(colour)));
    });
}

fn log(p: &mut ChildSpawnerCommands, fonts: &Fonts, lines: &[String], colour: Color) {
    let node =
        Node { flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(8.0)), border_radius: BorderRadius::all(Val::Px(4.0)), overflow: Overflow::clip(), ..default() };
    p.spawn((node, BackgroundColor(FIELD))).with_children(|l| {
        for line in lines {
            l.spawn(label(fonts, line, 13.0, colour, false));
        }
    });
}

fn tail(lines: &[String], n: usize) -> &[String] {
    &lines[lines.len().saturating_sub(n)..]
}

fn importing(p: &mut ChildSpawnerCommands, fonts: &Fonts, setup: &Setup, step: u32, total: u32, name: &str) {
    let heading = if total > 0 { format!("Importing: step {step} of {total}, {name}") } else { format!("Importing: {name}") };
    p.spawn(label(fonts, heading, 20.0, INK, true));
    // A step's line comes before it runs, so step n of t means n - 1 are done.
    bar(p, if total > 0 { (step.saturating_sub(1)) as f32 / total as f32 } else { 0.0 }, GOLD);
    p.spawn(label(fonts, format!("from {}", setup.check.as_ref().map_or(String::new(), |d| d.display().to_string())), 13.0, DIM, false));
    log(p, fonts, tail(&setup.log, 6), DIM);
    p.spawn(Node { justify_content: JustifyContent::FlexStart, ..row(10.0) }).with_children(|r| button(r, fonts, "Cancel and quit", SetupAction::Quit, true));
}

fn failed(p: &mut ChildSpawnerCommands, fonts: &Fonts, setup: &Setup, step: Option<&str>, reason: &str) {
    let heading = match step {
        Some(step) => format!("Import failed at step \"{step}\""),
        None => "Import failed".to_owned(),
    };
    p.spawn(label(fonts, heading, 22.0, BAD, true));
    p.spawn(label(fonts, reason, 15.0, INK, false));
    if !setup.log.is_empty() {
        p.spawn(label(fonts, "Last lines from the importer:", 14.0, DIM, false));
        log(p, fonts, tail(&setup.log, 10), INK);
    }
    p.spawn(Node { justify_content: JustifyContent::SpaceBetween, margin: UiRect::top(Val::Px(8.0)), ..row(10.0) }).with_children(|r| {
        button(r, fonts, "Quit", SetupAction::Quit, true);
        r.spawn(row(10.0)).with_children(|r| {
            button(r, fonts, "Change folder", SetupAction::Change, true);
            button(r, fonts, "Retry", SetupAction::Import, setup.check.is_ok());
        });
    });
}

fn hover(mut q: Query<(&Interaction, &Rest, &mut BackgroundColor), Changed<Interaction>>) {
    for (i, rest, mut bg) in &mut q {
        bg.0 = if *i == Interaction::None { rest.0 } else { BUTTON_HOVER };
    }
}

fn blink(time: Res<Time>, mut q: Query<&mut Visibility, With<Caret>>) {
    for mut v in &mut q {
        *v = if time.elapsed_secs().fract() < 0.5 { Visibility::Inherited } else { Visibility::Hidden };
    }
}
