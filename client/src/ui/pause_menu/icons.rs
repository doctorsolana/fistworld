//! Small native silhouettes for menu navigation. Shared colour and dimensions
//! keep them readable without an icon font or additional texture allocations.

use super::*;

pub(super) fn navigation(
    parent: &mut ChildSpawnerCommands<'_>,
    action: PauseButton,
    color: Color,
    size: f32,
) {
    parent
        .spawn(canvas(size))
        .with_children(|icon| match action {
            PauseButton::Resume | PauseButton::Back => arrow(icon, color, size),
            PauseButton::Graphics => monitor(icon, color, size),
            PauseButton::Controls => mouse(icon, color, size),
            PauseButton::Audio => music(icon, color, size),
            PauseButton::Disconnect => plug(icon, color, size),
            PauseButton::Exit => {
                outline(icon, size, [0.08, 0.10, 0.49, 0.80], color, 0.0);
                arrow(icon, color, size);
            }
        });
}

/// Section icon only; the caller continues to own the heading and divider.
pub(super) fn section(parent: &mut ChildSpawnerCommands<'_>, text: &str) {
    let color = crate::ui::startup::widgets::INK;
    let size = 28.0;
    parent.spawn(canvas(size)).with_children(|icon| match text {
        "Display" => monitor(icon, color, size),
        "Lighting" => sun(icon, color, size),
        "Camera & Selection" => mouse(icon, color, size),
        _ => gear(icon, color, size),
    });
}

fn canvas(size: f32) -> impl Bundle {
    (
        Node {
            width: Val::Px(size),
            height: Val::Px(size),
            flex_shrink: 0.0,
            ..default()
        },
        Pickable::IGNORE,
    )
}

fn rectangle(size: f32, bounds: [f32; 4]) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(bounds[0] * size),
        top: Val::Px(bounds[1] * size),
        width: Val::Px(bounds[2] * size),
        height: Val::Px(bounds[3] * size),
        ..default()
    }
}

fn outline(
    parent: &mut ChildSpawnerCommands<'_>,
    size: f32,
    bounds: [f32; 4],
    color: Color,
    round: f32,
) {
    parent.spawn((
        Node {
            border: UiRect::all(Val::Px((size * 0.07).max(1.3))),
            border_radius: BorderRadius::all(Val::Px(round * size)),
            ..rectangle(size, bounds)
        },
        BorderColor::all(color),
        Pickable::IGNORE,
    ));
}

fn dot(parent: &mut ChildSpawnerCommands<'_>, size: f32, bounds: [f32; 4], color: Color) {
    parent.spawn((
        Node {
            border_radius: BorderRadius::all(Val::Px(size)),
            ..rectangle(size, bounds)
        },
        BackgroundColor(color),
        Pickable::IGNORE,
    ));
}

fn line(
    parent: &mut ChildSpawnerCommands<'_>,
    size: f32,
    from: [f32; 2],
    to: [f32; 2],
    color: Color,
) {
    let from = Vec2::from_array(from) * size;
    let to = Vec2::from_array(to) * size;
    let delta = to - from;
    let middle = (from + to) * 0.5;
    let thickness = (size * 0.075).max(1.4);
    parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(middle.x - delta.length() * 0.5),
            top: Val::Px(middle.y - thickness * 0.5),
            width: Val::Px(delta.length()),
            height: Val::Px(thickness),
            border_radius: BorderRadius::all(Val::Px(thickness * 0.5)),
            ..default()
        },
        UiTransform::from_rotation(Rot2::radians(delta.y.atan2(delta.x))),
        BackgroundColor(color),
        Pickable::IGNORE,
    ));
}

fn arrow(parent: &mut ChildSpawnerCommands<'_>, color: Color, size: f32) {
    line(parent, size, [0.20, 0.50], [0.89, 0.50], color);
    line(parent, size, [0.63, 0.22], [0.90, 0.50], color);
    line(parent, size, [0.63, 0.78], [0.90, 0.50], color);
}

fn monitor(parent: &mut ChildSpawnerCommands<'_>, color: Color, size: f32) {
    outline(parent, size, [0.05, 0.13, 0.90, 0.65], color, 0.03);
    line(parent, size, [0.50, 0.78], [0.50, 0.91], color);
    line(parent, size, [0.29, 0.91], [0.71, 0.91], color);
    for (from, to) in [
        ([0.16, 0.65], [0.36, 0.38]),
        ([0.36, 0.38], [0.55, 0.64]),
        ([0.55, 0.64], [0.70, 0.45]),
        ([0.70, 0.45], [0.84, 0.64]),
    ] {
        line(parent, size, from, to, color);
    }
    dot(parent, size, [0.66, 0.26, 0.09, 0.09], color);
}

fn mouse(parent: &mut ChildSpawnerCommands<'_>, color: Color, size: f32) {
    outline(parent, size, [0.22, 0.05, 0.56, 0.90], color, 0.28);
    line(parent, size, [0.25, 0.40], [0.75, 0.40], color);
    line(parent, size, [0.50, 0.08], [0.50, 0.39], color);
}

fn music(parent: &mut ChildSpawnerCommands<'_>, color: Color, size: f32) {
    line(parent, size, [0.42, 0.19], [0.42, 0.70], color);
    line(parent, size, [0.81, 0.10], [0.81, 0.61], color);
    line(parent, size, [0.42, 0.19], [0.81, 0.10], color);
    line(parent, size, [0.42, 0.32], [0.81, 0.23], color);
    dot(parent, size, [0.10, 0.62, 0.34, 0.22], color);
    dot(parent, size, [0.49, 0.53, 0.34, 0.22], color);
}

fn plug(parent: &mut ChildSpawnerCommands<'_>, color: Color, size: f32) {
    outline(parent, size, [0.24, 0.34, 0.52, 0.36], color, 0.10);
    line(parent, size, [0.35, 0.10], [0.35, 0.34], color);
    line(parent, size, [0.65, 0.10], [0.65, 0.34], color);
    line(parent, size, [0.50, 0.70], [0.50, 0.92], color);
}

fn sun(parent: &mut ChildSpawnerCommands<'_>, color: Color, size: f32) {
    dot(parent, size, [0.29, 0.29, 0.42, 0.42], color);
    for index in 0..8 {
        let angle = index as f32 * std::f32::consts::FRAC_PI_4;
        let direction = Vec2::new(angle.cos(), angle.sin());
        line(
            parent,
            size,
            (Vec2::splat(0.5) + direction * 0.33).to_array(),
            (Vec2::splat(0.5) + direction * 0.46).to_array(),
            color,
        );
    }
}

fn gear(parent: &mut ChildSpawnerCommands<'_>, color: Color, size: f32) {
    outline(parent, size, [0.23, 0.23, 0.54, 0.54], color, 0.27);
    for index in 0..8 {
        let angle = index as f32 * std::f32::consts::FRAC_PI_4;
        let direction = Vec2::new(angle.cos(), angle.sin());
        line(
            parent,
            size,
            (Vec2::splat(0.5) + direction * 0.24).to_array(),
            (Vec2::splat(0.5) + direction * 0.44).to_array(),
            color,
        );
    }
}
