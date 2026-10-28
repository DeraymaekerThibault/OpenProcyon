//! World-anchored HUD: ship-name banners and the target chevrons (retail hud.hdt art projected over the 3D view), and the objective
//! navigation indicator under the player's ship. `N` (`panels::Toggles::names`) toggles the banners.
use bevy::prelude::*;

use crate::combat::{Health, Target};
use crate::panels::{REF_H, Toggles, cinematic};
use crate::sprites::{Fonts, HudData, image, sliced, sprite_of};
use crate::{Library, PlayerShip, Ship};

pub fn plugin(app: &mut App) {
    app.add_systems(PostStartup, spawn_reticle.run_if(resource_exists::<HudData>))
        .add_systems(Update, navigation.after(crate::camera).before(crate::sky).run_if(in_state(crate::menu::Screen::Battle)).run_if(crate::menu::overlay_closed))
        .add_systems(Update, end_line.run_if(in_state(crate::menu::Screen::Battle)).run_if(resource_exists::<Fonts>))
        .add_systems(
            Update,
            (place_ref_rects, scale_label_text, spawn_labels, place_labels)
                .chain()
                .after(crate::sprites::choose_skin)
                .run_if(in_state(crate::menu::Screen::Battle))
                .run_if(resource_exists::<HudData>)
                .run_if(resource_exists::<Fonts>),
        );
}

#[derive(Component)]
struct EndLineText;

/// The retail game-over line ("Game Lost!  Exiting...") in white, centred on the window while a decided skirmish winds down.
/// PROVISIONAL: size (one reference-footage frame: about 16 px of 1080) and the exact vertical centre.
fn end_line(
    mut commands: Commands,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    fonts: Res<Fonts>,
    line: Res<crate::scenario::EndLine>,
    mut shown: Query<(Entity, &mut Text, &mut TextFont), With<EndLineText>>,
) {
    let size = 17.0 * window.height() / 1080.0;
    let Some(id) = line.0.as_deref() else {
        for (e, ..) in &shown {
            commands.entity(e).despawn();
        }
        return;
    };
    if let Ok((_, _, mut f)) = shown.single_mut() {
        f.font_size = FontSize::Px(size);
    } else if let Some(font) = fonts.0.get("Immi14") {
        // the string table is read once, when the line appears
        let text = crate::menu::text_of(crate::menu::strings(), id);
        let node = Node { position_type: PositionType::Absolute, top: Val::Percent(49.0), left: Val::Px(0.0), width: Val::Percent(100.0), ..default() };
        commands.spawn((crate::Scenario, EndLineText, font.text(&text, size / font.size, Color::WHITE), TextLayout::justify(Justify::Center), node, GlobalZIndex(170)));
    }
}

/// A label child placed in reference pixels relative to its label root (scaled to the window by `place_ref_rects`).
#[derive(Component)]
struct RefRect {
    pos: Vec2,
    size: Vec2,
}
/// Scales label children (ribbon pieces) from reference pixels to the window.
fn place_ref_rects(window: Single<&Window, With<bevy::window::PrimaryWindow>>, mut q: Query<(&RefRect, &mut Node)>) {
    let s = window.height() / REF_H;
    for (r, mut n) in &mut q {
        n.left = Val::Px(r.pos.x * s);
        n.top = Val::Px(r.pos.y * s);
        n.width = Val::Px(r.size.x * s);
        n.height = Val::Px(r.size.y * s);
    }
}

#[derive(Component)]
struct Navigation {
    arrow: bool,
}

fn navigation(
    mut commands: Commands,
    assets: Res<AssetServer>,
    lib: Res<Library>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    ships: Query<(Entity, &Transform, &Ship, Has<PlayerShip>)>,
    fleets: Res<crate::ship::Fleets>,
    mission: Option<Res<crate::campaign::Mission>>,
    victory: Option<Res<crate::victory::Victory>>,
    bases: Query<(&Transform, &crate::campaign::MissionBase, &crate::combat::Health), (With<crate::combat::ArmedBase>, Without<Ship>, Without<Navigation>)>,
    mut indicators: Query<(&Navigation, &mut Transform, &mut Visibility), Without<Ship>>,
) {
    if indicators.is_empty() {
        for (arrow, part) in [(false, "COMPASS"), (true, "ARROW")] {
            let mut root = commands.spawn((
                crate::Scenario,
                Navigation { arrow },
                Visibility::Hidden,
                Transform::default(),
                crate::data::scene(&assets, &format!("SELECTION_INDICATOR_{part}_SMALL.glb")),
            ));
            if !arrow {
                // PROVISIONAL backing fill: the native indicator render state is not decoded.
                let mesh = meshes.add(Circle::new(5.2));
                let material = materials.add(StandardMaterial {
                    base_color: Color::srgba(0.02, 0.22, 0.38, 0.8),
                    unlit: true,
                    alpha_mode: AlphaMode::Blend,
                    double_sided: true,
                    cull_mode: None,
                    ..default()
                });
                root.with_children(|c| {
                    c.spawn((Mesh3d(mesh), MeshMaterial3d(material), Transform::from_xyz(0.0, -4.2, 0.0).with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))));
                });
            }
        }
    }
    let Some((_, tf, ship, _)) = ships.iter().find(|s| s.3) else {
        for (_, _, mut v) in &mut indicators {
            *v = Visibility::Hidden;
        }
        return;
    };
    let primary_tf = tf;
    let goal = match mission.as_deref() {
        Some(m) => crate::campaign::objective_point(m),
        // Skirmish: the arrow points at the base the map's end rule says to destroy; with no such objective it is not shown.
        None => victory.as_deref().and_then(|v| {
            let ids = v.objective_bases();
            bases
                .iter()
                .filter(|(_, b, h)| ids.contains(&b.id) && !h.destroyed)
                .map(|(t, ..)| t.translation)
                .min_by(|a, b| a.distance(primary_tf.translation).total_cmp(&b.distance(primary_tf.translation)))
        }),
    };
    for (indicator, mut transform, mut vis) in &mut indicators {
        let selected = fleets.selected.and_then(|e| ships.get(e).ok());
        let (tf, ship) = if !indicator.arrow { selected.map_or((primary_tf, ship), |(_, t, s, _)| (t, s)) } else { (primary_tf, ship) };
        vis.set_if_neq(if cinematic(mission.as_deref()) || (indicator.arrow && goal.is_none()) { Visibility::Hidden } else { Visibility::Inherited });
        transform.translation = tf.translation;
        // The ring's radius is the ship's radius times its WOT fraction. PROVISIONAL: the small indicator's own radius (the compass mesh
        // measures 5.9) is taken as the ring's.
        let class = &lib.classes[ship.class];
        transform.scale = Vec3::splat(class.radius * class.ring / COMPASS_RADIUS);
        transform.rotation = if indicator.arrow { Quat::from_rotation_y(navigation_heading(tf.translation, goal.unwrap_or(tf.translation))) } else { Quat::IDENTITY };
    }
}

/// Outer radius of `SELECTION_INDICATOR_COMPASS_SMALL` in model units.
const COMPASS_RADIUS: f32 = 5.9;

fn navigation_heading(ship: Vec3, goal: Vec3) -> f32 {
    let d = goal - ship;
    (-d.x).atan2(-d.z)
}

// ---- ship name banners and the target reticle -----------------------------------------------------------------

/// Root of a banner that follows `ship`; its children are positioned relative to it (Anchor::Parent).
#[derive(Component)]
struct Label {
    ship: Entity,
    width: f32,
}
#[derive(Component)]
struct Labeled;
/// One of the four target chevrons (the original's 'ShipTargeted_Icon_Enemy', a corner pointer) set round the target like a 45-degree cross.
#[derive(Component)]
struct Reticle(usize);

fn spawn_reticle(mut commands: Commands, assets: Res<AssetServer>, hud: Res<HudData>) {
    let Some((img, _)) = image(&assets, &hud, "ShipTargeted_Icon_Enemy") else { return };
    // The sprite's tip points up-right, i.e. it belongs at the bottom-left corner; the others are the same sprite turned by 90 degrees.
    for i in 0..4 {
        commands.spawn((
            Reticle(i),
            img.clone(),
            Node { position_type: PositionType::Absolute, ..default() },
            UiTransform::from_rotation(Rot2::degrees(-90.0 * i as f32)),
            Visibility::Hidden,
        ));
    }
}

/// Name-ribbon tint: the mission's banner type if set, else red for our side and blue for the others.
fn banner_tint(side: crate::Side, ms: Option<&crate::campaign::MissionShip>) -> Color {
    match ms.map_or("", |m| m.banner.as_str()) {
        b if b.contains("Black") => Color::srgb(0.12, 0.12, 0.12),
        b if b.contains("Blue") => Color::srgb(0.1, 0.25, 0.95),
        b if b.contains("Red") => Color::srgb(0.9, 0.08, 0.08),
        _ if side == crate::Side::Player => Color::srgb(0.9, 0.08, 0.08),
        _ => Color::srgb(0.1, 0.25, 0.95),
    }
}

/// Ribbon text: a wrapper laid out in reference px (centred flex row) and its text child, scaled to the window like the sprites.
#[derive(Component)]
struct LabelText {
    pos: Vec2,
    width: f32,
}
#[derive(Component)]
struct LabelSize(f32);
fn scale_label_text(window: Single<&Window, With<bevy::window::PrimaryWindow>>, mut w: Query<(&LabelText, &mut Node)>, mut t: Query<(&LabelSize, &mut TextFont)>) {
    let s = window.height() / REF_H;
    for (l, mut n) in &mut w {
        n.left = Val::Px(l.pos.x * s);
        n.top = Val::Px(l.pos.y * s);
        n.width = Val::Px(l.width * s);
    }
    for (z, mut f) in &mut t {
        f.font_size = FontSize::Px(z.0 * s);
    }
}

fn spawn_labels(
    mut commands: Commands,
    assets: Res<AssetServer>,
    hud: Res<HudData>,
    fonts: Res<Fonts>,
    lib: Res<Library>,
    ships: Query<(Entity, &Ship, Option<&crate::campaign::MissionShip>, Option<&crate::ShipName>), Without<Labeled>>,
) {
    let Some(font) = fonts.0.get("Immi14") else { return };
    for (e, ship, mission, named) in &ships {
        commands.entity(e).insert(Labeled);
        // Reference video: thin white ribbon (ShipNameBackdrop_*_WhiteBanner) tinted red for our side, blue for the others.
        let tint = banner_tint(ship.side, mission);
        let part = |side: &str| sprite_of(&hud, &format!("ShipNameBackdrop_{side}_WhiteBanner"));
        let (Some(l), Some(c), Some(r)) = (part("Left"), part("Center"), part("Right")) else { continue };
        let text = mission
            .map(|m| m.name.clone())
            .filter(|n| !n.is_empty())
            .or(named.map(|n| n.0.clone()))
            .unwrap_or_else(|| lib.classes[ship.class].name.trim_start_matches("Ship_").replace('_', " "));
        // PROVISIONAL font scale; the ribbon's opaque strip is only 21 px, not its 33 px atlas height.
        let font_scale = 1.05;
        let tw = font.width(&text) * font_scale;
        let total = l.1.width() + r.1.width() + c.1.width().max(tw + 8.0);
        let text_pos = Vec2::new(((total - tw) / 2.0).round(), ((22.0 - font.height * font_scale) / 2.0 + 3.0).round()); // +3: the line box sits above the ribbon strip
        let root = commands.spawn((Label { ship: e, width: total }, Node { position_type: PositionType::Absolute, ..default() }, Visibility::Hidden)).id();
        let mut kids = vec![];
        let mut img = sliced(&assets, "ribbon", hud.skin, "nocursor", l.1.width(), r.1.width());
        img.color = tint;
        kids.push(commands.spawn((img, Node { position_type: PositionType::Absolute, ..default() }, RefRect { pos: Vec2::ZERO, size: Vec2::new(total, l.1.height()) })).id());
        let wrapper = commands
            .spawn((
                Node { position_type: PositionType::Absolute, justify_content: JustifyContent::Center, ..default() },
                LabelText { pos: Vec2::new(0.0, text_pos.y), width: total },
            ))
            .id();
        commands.spawn((font.text(&text, 1.0, Color::WHITE), TextLayout::no_wrap(), LabelSize(font.size * font_scale), ChildOf(wrapper)));
        kids.push(wrapper);
        commands.entity(root).add_children(&kids);
    }
}

/// Use the highest intact-model section bound, including sails, rather than a fixed hull-origin offset.
fn banner_height(class: &crate::combat::Class) -> f32 {
    class.sections.iter().map(|section| section.max.y).fold(6.0, f32::max) + 2.0
}

fn place_labels(
    mut commands: Commands,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    cam: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    target: Res<Target>,
    selection: (Res<crate::ship::Fleets>, Query<&crate::ship::FleetPilot>),
    ships: Query<(&GlobalTransform, &Health, &Ship, Option<&crate::campaign::MissionShip>)>,
    animals: Query<(&GlobalTransform, &crate::environment::SpaceAnimal)>,
    lib: Res<Library>,
    toggles: Res<Toggles>,
    sensors: Res<crate::environment::Sensors>,
    mut labels: Query<(Entity, &Label, &mut Node, &mut Visibility, Option<&Children>), Without<Reticle>>,
    mut imgs: Query<&mut ImageNode, Without<Reticle>>,
    mut reticle: Query<(&Reticle, &mut Node, &mut Visibility), With<Reticle>>,
    mission: Option<Res<crate::campaign::Mission>>,
) {
    let s = window.height() / REF_H;
    let (camera, ctf) = *cam;
    let project = |g: &GlobalTransform, up: f32| camera.world_to_viewport(ctf, g.translation() + Vec3::Y * up).ok();
    for (e, label, mut node, mut vis, kids) in &mut labels {
        let Ok((g, h, sh, ms)) = ships.get(label.ship) else {
            commands.entity(e).despawn();
            continue;
        };
        match project(g, banner_height(&lib.classes[sh.class])) {
            Some(p) if !h.destroyed && ms.is_none_or(|m| m.visible) && sensors.player_sees(label.ship, mission.as_deref()) && toggles.names && !cinematic(mission.as_deref()) => {
                // Banners fade out as the camera closes in (reference game): opaque beyond ~90 units, faint under ~30.
                let a = ((g.translation().distance(ctf.translation()) - 20.0) / 70.0).clamp(0.12, 1.0);
                // The first three children are the ribbon pieces; the rules' banner type ('Banner BlueTeam' ...) picks their tint.
                let tint = banner_tint(sh.side, ms);
                for (n, k) in kids.into_iter().flatten().enumerate() {
                    if let Ok(mut i) = imgs.get_mut(*k) {
                        if n < 3 {
                            i.color = tint;
                        }
                        i.color.set_alpha(a);
                    }
                }
                // Backdrop sits 50 reference px above the anchor point (Offset_ShipName_Background).
                node.left = Val::Px(p.x - label.width * s / 2.0);
                node.top = Val::Px(p.y - 50.0 * s);
                vis.set_if_neq(Visibility::Inherited);
            }
            _ => {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
    }
    let shown = (|| {
        if cinematic(mission.as_deref()) {
            return None;
        }
        let entity = if selection.0.direct() {
            target.0
        } else {
            selection.0.selected.and_then(|e| selection.1.get(e).ok().and_then(|p| p.target).or_else(|| ships.get(e).ok().and_then(|s| s.3.and_then(|m| m.target))))
        }?;
        let (g, r, visible) = if let Ok((g, h, ship, ms)) = ships.get(entity) {
            (g, lib.classes[ship.class].radius, !h.destroyed && ms.is_none_or(|m| m.visible))
        } else {
            let (g, a) = animals.get(entity).ok()?;
            (g, a.radius, a.visible)
        };
        if !visible || !sensors.player_sees(entity, mission.as_deref()) {
            return None;
        }
        let p = project(g, 0.0)?;
        // Chevrons stand on the diagonals just outside the target's projected radius.
        let edge = camera.world_to_viewport(ctf, g.translation() + ctf.right() * r).ok().map_or(0.0, |q| (q - p).length());
        Some((p, edge))
    })();
    for (Reticle(i), mut node, mut vis) in &mut reticle {
        vis.set_if_neq(Visibility::Hidden);
        let Some((p, edge)) = shown else { continue };
        let size = (edge * 0.3).clamp(16.0 * s, 40.0 * s);
        let corner = [Vec2::new(-1.0, 1.0), Vec2::new(-1.0, -1.0), Vec2::new(1.0, -1.0), Vec2::new(1.0, 1.0)][*i];
        let c = p + corner * (edge.max(20.0 * s) * 0.8 + size * 0.2);
        node.left = Val::Px(c.x - size / 2.0);
        node.top = Val::Px(c.y - size / 2.0);
        // Size comes from layout_and_fill (Sprite); set it here after that system ran.
        node.width = Val::Px(size);
        node.height = Val::Px(size);
        vis.set_if_neq(Visibility::Inherited);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets_dir;
    #[test]
    fn banners_clear_the_sails_of_large_and_small_ships() {
        let lib = Library::load(assets_dir());
        for name in ["Ship_Navy_Cutter", "Ship_Navy_ManOWar", "Ship_Pirate_Barque"] {
            let class = &lib.classes[lib.index(name)];
            assert!(class.sections.iter().all(|s| banner_height(class) >= s.max.y + 2.0));
        }
        assert!(banner_height(&lib.classes[lib.index("Ship_Navy_ManOWar")]) > banner_height(&lib.classes[lib.index("Ship_Navy_Cutter")]));
    }
    #[test]
    fn objective_arrow_points_from_ship_to_goal() {
        let pos = Vec3::new(100.0, 0.0, -30.0);
        for direction in [Vec3::NEG_Z, Vec3::Z, Vec3::X, Vec3::NEG_X] {
            let q = Quat::from_rotation_y(navigation_heading(pos, pos + direction * 50.0));
            assert!((q * Vec3::NEG_Z).distance(direction) < 0.00001);
        }
    }
}
