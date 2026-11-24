//! Classic drawing of the developer console (`rules::console`): a dark panel over the top of the screen with the output and the
//! line being typed, and a red "CHEATS" tag once a command that changes the game has run. State and input are the shared module's.
use bevy::prelude::*;

use crate::campaign::Mission;
use crate::rules::console::{Cheats, Console, DevView, HELP_COLUMN, ShapeKind};

#[derive(Component)]
struct Panel;
#[derive(Component)]
struct Tag;

pub fn plugin(app: &mut App) {
    app.add_systems(Update, fps_overlay.run_if(resource_exists::<crate::rules::console::FrameStats>))
        .add_systems(Update, (build, draw).chain().run_if(in_state(crate::menu::Screen::Battle)).run_if(resource_exists::<Console>))
        .add_systems(Update, (build_shapes, place_labels).chain().after(crate::camera).run_if(in_state(crate::menu::Screen::Battle)).run_if(resource_exists::<Mission>));
}

fn build(mut commands: Commands, existing: Query<(), With<Panel>>) {
    if !existing.is_empty() {
        return;
    }
    let font = |size| TextFont { font_size: FontSize::Px(size), ..default() };
    commands.spawn((
        Panel,
        Visibility::Hidden,
        GlobalZIndex(400),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.05, 0.88)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(0.0),
            left: Val::Px(0.0),
            width: Val::Percent(100.0),
            padding: UiRect::all(Val::Px(8.0)),
            flex_direction: FlexDirection::Column,
            ..default()
        },
    ));
    commands.spawn((
        Tag,
        Visibility::Hidden,
        GlobalZIndex(401),
        Text::new("CHEATS USED: this battle's points are not recorded"),
        font(14.0),
        TextColor(Color::srgb(1.0, 0.3, 0.25)),
        Node { position_type: PositionType::Absolute, bottom: Val::Px(6.0), left: Val::Px(8.0), ..default() },
    ));
}

/// Width of the command column of `help` rows, in px at the console's 15 px font (fits the longest usage, "PageUp / PageDown, mouse wheel").
const COMMAND_COLUMN: f32 = 260.0;

fn draw(
    mut commands: Commands,
    console: Res<Console>,
    cheats: Res<Cheats>,
    mut panel: Query<(Entity, &mut Visibility, Option<&Children>), (With<Panel>, Without<Tag>)>,
    mut tag: Query<&mut Visibility, (With<Tag>, Without<Panel>)>,
    mut drawn: Local<String>,
) {
    for mut v in &mut tag {
        v.set_if_neq(if cheats.used { Visibility::Inherited } else { Visibility::Hidden });
    }
    for (e, mut v, children) in &mut panel {
        v.set_if_neq(if console.open { Visibility::Inherited } else { Visibility::Hidden });
        if !console.open {
            continue;
        }
        let shown = console.display();
        if *drawn == shown && children.is_some() {
            continue;
        }
        // One row per line; a `help` row is two pieces: the command in a fixed column, the description wrapping beside it.
        commands.entity(e).despawn_children();
        commands.entity(e).with_children(|p| {
            for line in shown.lines() {
                let piece = |text: &str| (Text::new(text), TextFont { font_size: FontSize::Px(15.0), ..default() }, TextColor(Color::srgb(0.85, 0.95, 0.85)));
                match line.split_once(HELP_COLUMN) {
                    Some((command, about)) => {
                        p.spawn(Node { width: Val::Percent(100.0), ..default() }).with_children(|row| {
                            row.spawn((piece(command), Node { width: Val::Px(COMMAND_COLUMN), flex_shrink: 0.0, ..default() }));
                            row.spawn((piece(about), Node { flex_grow: 1.0, flex_basis: Val::Px(0.0), min_width: Val::Px(0.0), ..default() }));
                        });
                    }
                    None => {
                        p.spawn(piece(line));
                    }
                }
            }
        });
        *drawn = shown;
    }
}

// -------------------------------------------------------------------------------------------------- `show` / `hide`

/// One segment of a drawn shape (a thin unlit box; the build has no gizmos).
#[derive(Component)]
struct DebugLine;
/// A label tied to a world point; `place_labels` moves it over the point each frame.
#[derive(Component)]
struct DebugLabel(Vec3);

fn colour(kind: ShapeKind) -> Color {
    match kind {
        ShapeKind::Volumes => Color::srgb(1.0, 0.6, 0.1),
        ShapeKind::Paths => Color::srgb(0.2, 0.9, 1.0),
        ShapeKind::Points => Color::srgb(0.4, 1.0, 0.4),
    }
}

/// Rebuilds the drawing when what is shown changes (or the battle restarted and swept it away).
fn build_shapes(
    mut commands: Commands,
    dev: Res<DevView>,
    mission: Res<Mission>,
    player: Query<&Transform, With<crate::PlayerShip>>,
    old: Query<Entity, Or<(With<DebugLine>, With<DebugLabel>)>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut shown: Local<Vec<(ShapeKind, Option<String>)>>,
) {
    let now: Vec<_> = dev.shown.iter().cloned().collect();
    if *shown == now && (now.is_empty() || !old.is_empty()) {
        return;
    }
    *shown = now.clone();
    for e in &old {
        commands.entity(e).despawn();
    }
    let h = player.iter().next().map_or(0.0, |t| t.translation.y);
    let mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    for (kind, name) in now {
        let material = materials.add(StandardMaterial { base_color: colour(kind), unlit: true, ..default() });
        let segment = |a: Vec3, b: Vec3, commands: &mut Commands| {
            let len = a.distance(b);
            if len < 0.01 {
                return;
            }
            let tf = Transform::from_translation((a + b) / 2.0).looking_at(b, Vec3::Y).with_scale(Vec3::new(0.8, 0.8, len));
            commands.spawn((crate::Scenario, DebugLine, Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), tf));
        };
        for shape in mission.script.debug_shapes(kind, name.as_deref(), h) {
            let at = shape.points[0];
            match shape.points.len() {
                1 => {
                    for d in [Vec3::X, Vec3::Z] {
                        segment(at - d * 6.0, at + d * 6.0, &mut commands);
                    }
                    segment(at, at + Vec3::Y * 20.0, &mut commands);
                }
                _ => {
                    for w in shape.points.windows(2) {
                        segment(w[0], w[1], &mut commands);
                    }
                    if shape.closed {
                        segment(*shape.points.last().unwrap(), at, &mut commands);
                    }
                }
            }
            commands.spawn((
                crate::Scenario,
                DebugLabel(at + Vec3::Y * 22.0),
                Text::new(shape.label.clone()),
                TextFont { font_size: FontSize::Px(13.0), ..default() },
                TextColor(colour(kind)),
                GlobalZIndex(390),
                Node { position_type: PositionType::Absolute, ..default() },
                Visibility::Hidden,
            ));
        }
    }
}

fn place_labels(camera: Single<(&Camera, &GlobalTransform), (With<Camera3d>, With<crate::MainCamera>)>, mut labels: Query<(&DebugLabel, &mut Node, &mut Visibility)>) {
    let (camera, ctf) = *camera;
    for (label, mut node, mut vis) in &mut labels {
        match camera.world_to_viewport(ctf, label.0) {
            Ok(p) if (ctf.translation() - label.0).dot(*ctf.forward()) < 0.0 => {
                node.left = Val::Px(p.x);
                node.top = Val::Px(p.y);
                vis.set_if_neq(Visibility::Inherited);
            }
            _ => {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
    }
}

// -------------------------------------------------------------------------------------------------------------------- `fps`

/// The frame-rate counter (`fps` command): a small dark box in the top-right corner, above the HUD switch so `hud off` leaves it.
#[derive(Component)]
struct FpsBox;

fn fps_overlay(
    mut commands: Commands,
    stats: Res<crate::rules::console::FrameStats>,
    time: Res<Time<Virtual>>,
    mut boxes: Query<(&mut Visibility, &Children), With<FpsBox>>,
    mut texts: Query<&mut Text>,
) {
    let on = crate::menu::fps_setting();
    if boxes.is_empty() {
        commands.spawn((
            FpsBox,
            Visibility::Hidden,
            GlobalZIndex(402),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.05, 0.8)),
            Node { position_type: PositionType::Absolute, top: Val::Px(4.0), right: Val::Px(4.0), padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)), ..default() },
            children![(Text::new(""), TextFont { font_size: FontSize::Px(13.0), ..default() }, TextColor(Color::srgb(0.7, 1.0, 0.7)))],
        ));
        return;
    }
    for (mut v, children) in &mut boxes {
        v.set_if_neq(if on { Visibility::Inherited } else { Visibility::Hidden });
        if !on {
            continue;
        }
        if let Some(mut text) = children.first().and_then(|c| texts.get_mut(*c).ok()) {
            let shown = stats.text(time.relative_speed());
            if text.0 != shown {
                text.0 = shown;
            }
        }
    }
}
