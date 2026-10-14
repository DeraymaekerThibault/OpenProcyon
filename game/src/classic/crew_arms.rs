//! Retail presentation of the shared campaign outfitting model; layout remains provisional.
#![allow(deprecated)]
use crate::crew_arms::{CrewSet, ScreenModel};
use bevy::prelude::*;

#[derive(Component)]
struct Root;

pub fn plugin(app: &mut App) {
    app.add_systems(Update, build.after(CrewSet));
}

fn build(mut commands: Commands, model: Res<ScreenModel>, old: Query<Entity, With<Root>>) {
    if !model.is_changed() && (model.open == !old.is_empty()) {
        return;
    }
    for root in &old {
        commands.entity(root).despawn();
    }
    if !model.open {
        return;
    }
    commands
        .spawn((
            Root,
            crate::Scenario,
            Interaction::None,
            crate::panels::Solid,
            Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), padding: UiRect::all(Val::Percent(5.0)), ..default() },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.8)),
            GlobalZIndex(180),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    padding: UiRect::all(Val::Px(12.0)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(6.0),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.02, 0.04, 0.1)),
            ))
            .with_children(|card| {
                card.spawn((Text::new(&model.title), TextFont { font_size: FontSize::Px(20.0), ..default() }, TextColor(Color::srgb(1.0, 0.92, 0.75))));
                card.spawn((Text::new(&model.detail), TextFont { font_size: FontSize::Px(14.0), ..default() }, TextColor(Color::WHITE)));
                card.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(6.0), row_gap: Val::Px(4.0), ..default() }).with_children(|nav| {
                    for (label, action) in &model.navigation {
                        nav.spawn((
                            action.clone(),
                            Interaction::None,
                            Node { padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)), ..default() },
                            BackgroundColor(Color::srgb(0.1, 0.2, 0.45)),
                        ))
                        .with_children(|button| {
                            button.spawn((Text::new(label), TextFont { font_size: FontSize::Px(14.0), ..default() }, TextColor(Color::WHITE)));
                        });
                    }
                });
                for (label, action) in &model.rows {
                    card.spawn((
                        action.clone(),
                        Interaction::None,
                        Node { width: Val::Percent(100.0), padding: UiRect::all(Val::Px(6.0)), ..default() },
                        BackgroundColor(Color::srgb(0.07, 0.12, 0.23)),
                    ))
                    .with_children(|button| {
                        button.spawn((Text::new(label), TextFont { font_size: FontSize::Px(14.0), ..default() }, TextColor(Color::WHITE)));
                    });
                }
                card.spawn((Text::new(&model.message), TextFont { font_size: FontSize::Px(14.0), ..default() }, TextColor(Color::srgb(1.0, 0.8, 0.35))));
            });
        });
}
