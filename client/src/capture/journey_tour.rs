//! Opt-in rehearsal of the actual notice controls. Only world records and
//! incoming action messages are fixtures; expansion/clear use real UI input.
use super::{CaptureConfig, CaptureState};
use bevy::{input::InputSystems, prelude::*, ui::UiSystems};
use shared::components::{
    CharacterName, Hero, PlayerPosition, Settlement, SettlementId, SettlementSummary,
};

mod medieval;
pub(super) use medieval::ready;

pub(super) fn install(app: &mut App) {
    if std::env::var("FISTFORCE_CAPTURE_NOTICES").as_deref() != Ok("1") {
        return;
    }
    if std::env::var("FISTFORCE_CAPTURE_MEDIEVAL_HUD").is_ok() {
        medieval::install(app);
        return;
    }
    app.add_systems(Update, stage);
    app.add_systems(PreUpdate, input.after(InputSystems).after(UiSystems::Focus));
    app.add_systems(Last, inspect);
}

fn stage(
    mut commands: Commands,
    mut staged: Local<bool>,
    halls: Query<(&SettlementId, &Settlement, &PlayerPosition)>,
    heroes: Query<Entity, (With<Hero>, Without<shared::economy::GoodsInventory>)>,
) {
    for hero in &heroes {
        commands.entity(hero).insert((
            CharacterName("Aldric".into()),
            shared::economy::GoodsInventory::new(shared::economy::capacity::VILLAGER),
        ));
    }
    if *staged {
        return;
    }
    let Some((id, town, position)) = halls.iter().next() else {
        return;
    };
    let summary: SettlementSummary = serde_json::from_value(serde_json::json!({
        "id":id, "name":town.name, "tier":town.tier, "residents":town.residents.max(1),
        "treasury":town.treasury, "prosperity":0.5, "reserve_days":5.0,
        "houses":3, "farmsteads":1, "fishing_huts":0, "lumber_huts":1
    }))
    .expect("notice capture settlement directory fixture");
    commands.spawn((summary, position.clone()));
    *staged = true;
}

fn input(
    config: Res<CaptureConfig>,
    state: Res<CaptureState>,
    mut previous: Local<Option<usize>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut buttons: Query<(&Name, &mut Interaction), With<Button>>,
    mut notice: ResMut<crate::ui::hud::GodNotice>,
) {
    mouse.release(MouseButton::Left);
    for (_, mut interaction) in &mut buttons {
        interaction.set_if_neq(Interaction::None);
    }
    let CaptureState::Settling { shot, .. } = *state else {
        return;
    };
    if *previous == Some(shot) {
        return;
    }
    *previous = Some(shot);
    let action = match config.shots[shot].name.as_str() {
        "02-unread" => {
            notice.show("Your hero cannot afford the cheapest offer.");
            None
        }
        "03-expanded" | "05-collapsed" | "06-reopened" => Some("journey-NOTICES"),
        "04-cleared" => Some("journey-CLEAR"),
        _ => None,
    };
    if let Some(action) = action {
        let (_, mut interaction) = buttons
            .iter_mut()
            .find(|(name, _)| name.as_str() == action)
            .expect("notice control exists");
        *interaction = Interaction::Pressed;
        mouse.press(MouseButton::Left);
    }
}

fn inspect(
    config: Res<CaptureConfig>,
    state: Res<CaptureState>,
    mut inspected: Local<Option<usize>>,
    nodes: Query<(&Name, &Node, &ComputedNode)>,
    text: Query<&Text>,
) {
    let CaptureState::AwaitingCapture { shot, .. } = *state else {
        return;
    };
    if *inspected == Some(shot) {
        return;
    }
    *inspected = Some(shot);
    let name = &config.shots[shot].name;
    let (_, node, computed) = nodes
        .iter()
        .find(|(name, ..)| name.as_str() == "Notice details")
        .expect("retained notice panel exists");
    let expanded = matches!(name.as_str(), "03-expanded" | "04-cleared" | "06-reopened");
    assert_eq!(
        node.display != Display::None,
        expanded,
        "real toggle action must control panel"
    );
    let texts: Vec<_> = text.iter().map(|text| text.0.as_str()).collect();
    if name == "03-expanded" {
        assert!(texts.contains(&"Your hero cannot afford the cheapest offer."));
    }
    if matches!(name.as_str(), "04-cleared" | "06-reopened") {
        assert!(
            texts.contains(&"No recent messages."),
            "clearing survives close and reopen"
        );
    }
    if expanded {
        assert!(computed.size().x > 100.0 && computed.size().y > 100.0);
        assert!(computed.size().x < config.resolution[0] as f32 * 0.5);
        assert!(computed.size().y < config.resolution[1] as f32 * 0.8);
    }
    let evidence = serde_json::json!({"shot":name, "expanded":expanded,
        "panel_pixels":computed.size().to_array(), "input":"production button handlers",
        "fixture":"offline hero, town directory and one incoming action result", "passed":true});
    std::fs::write(
        config.out_dir.join(format!("{name}.notices.json")),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .expect("notice capture evidence");
}
