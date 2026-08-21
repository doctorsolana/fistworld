//! Founding a company: one form, two homes.
//!
//! The form is its own page in the encyclopedia (COMPANIES › NEW COMPANY) and
//! also sits inline on the permit board's ACTING AS strip when you have no
//! company to act for. Name and capital are a local draft edited entirely on
//! the client; only FOUND COMPANY talks to the server, which still validates
//! the name, the capital and that your hero stands at a Hall.
//!
//! The draft's text is written in place every frame ([`sync_founding_texts`]),
//! so a press on +1 shows instantly even while the pointer rests on the
//! button — refresh-gated panels only rebuild once the pointer leaves.

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use lightyear::prelude::{Connected, MessageReceiver, MessageSender};

use shared::components::{
    CharacterName, CompanyId, CompanyLeadership, Hero, PersonId, PlayerPosition, Settlement,
};
use shared::economy::{format_money, Wallet, PENNIES_PER_COIN};
use shared::protocol::{HeroCompanyFoundingOrder, HeroCompanyFoundingResult, ReliableChannel};

use crate::camera_rts::LocalPeerId;
use crate::states::GameState;
use crate::ui::encyclopedia::{EncyclopediaOpen, EncyclopediaPageHost, EncyclopediaTab};
use crate::ui::foundation::{
    button_chrome, subtree_is_interacting, UiButtonLabel, UiButtonStyle, UiButtonVariant,
    UiRefreshExempt, UiRefreshStamp,
};
use crate::ui::player_permits::ActiveCompany;
use crate::ui::styles::{INK, INK_MUTED, LIMEWASH, LIMEWASH_LIT, PLATE_RULE_SOFT, RADIUS};

/// Must match the server's permit-desk range: founding happens at a Hall.
const HALL_RANGE: f32 = 12.0;

const T_TITLE: f32 = 22.0;
const T_VALUE: f32 = 18.0;
const T_BUTTON: f32 = 14.0;
const T_BODY: f32 = 13.5;
const T_LABEL: f32 = 12.0;

pub struct CompanyFoundingPlugin;

impl Plugin for CompanyFoundingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CompanyFoundingDraft>();
        app.init_resource::<CompanyFoundingFeedback>();
        app.init_resource::<FoundingPageOpen>();
        app.add_systems(
            Update,
            (
                receive_company_founding_results,
                handle_founding_buttons,
                handle_founding_name_input,
                ensure_founding_page,
                sync_founding_texts,
            )
                .chain()
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(OnExit(GameState::Playing), cleanup);
    }
}

/// The client-side draft. Nothing here reaches the server until FOUND.
#[derive(Resource, Debug, Clone)]
pub(crate) struct CompanyFoundingDraft {
    pub founder: Option<PersonId>,
    pub name: String,
    pub initial_capital: u64,
    pub editing_name: bool,
    pub pending: bool,
}

impl Default for CompanyFoundingDraft {
    fn default() -> Self {
        Self {
            founder: None,
            name: String::new(),
            initial_capital: 10 * PENNIES_PER_COIN,
            editing_name: false,
            pending: false,
        }
    }
}

#[derive(Resource, Default, Debug, Clone)]
pub(crate) struct CompanyFoundingFeedback {
    pub message: String,
    pub success: bool,
}

/// The encyclopedia's NEW COMPANY page.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub(crate) struct FoundingPageOpen(pub bool);

#[derive(Component)]
pub(crate) struct CompanyNameField;

#[derive(Component)]
pub(crate) struct AdjustFoundingCapital(pub i64);

#[derive(Component)]
pub(crate) struct FoundCompanyButton {
    pub hall: Entity,
}

/// Takes you to the NEW COMPANY page (closing whatever panel held the button).
#[derive(Component)]
pub(crate) struct OpenCompanyFounding;

#[derive(Component)]
struct FoundingNameText;

#[derive(Component)]
struct FoundingCapitalText;

#[derive(Component)]
struct FoundingPageRoot {
    signature: String,
}

pub(crate) type FoundingControl = Or<(
    With<CompanyNameField>,
    With<AdjustFoundingCapital>,
    With<OpenCompanyFounding>,
    With<FoundCompanyButton>,
)>;

pub(crate) fn suggested_company_name(hero: &str, existing: usize) -> String {
    if existing == 0 {
        format!("{hero} & Company")
    } else {
        format!("{hero} Company {}", existing.saturating_add(1))
    }
}

pub(crate) fn suggested_founding_capital(available: u64) -> u64 {
    if available < PENNIES_PER_COIN {
        PENNIES_PER_COIN
    } else {
        available.min(10 * PENNIES_PER_COIN)
    }
}

fn name_text(draft: &CompanyFoundingDraft) -> String {
    // The caret sits flush against the last character; an empty field shows
    // only the caret while editing and a placeholder when not.
    match (draft.name.is_empty(), draft.editing_name) {
        (true, true) => "|".to_string(),
        (true, false) => "Type a company name".to_string(),
        (false, true) => format!("{}|", draft.name),
        (false, false) => draft.name.clone(),
    }
}

fn capital_text(draft: &CompanyFoundingDraft) -> String {
    format!("{} coin", format_money(draft.initial_capital))
}

/// Everything the form needs to draw itself, gathered by whichever panel
/// hosts it.
pub(crate) struct FoundingFormView<'a> {
    pub draft: &'a CompanyFoundingDraft,
    pub feedback: &'a CompanyFoundingFeedback,
    pub wallet: u64,
    /// The Hall the hero stands at, if any. Founding needs one.
    pub hall: Option<(Entity, &'a str)>,
    pub existing: usize,
}

/// The founding form. Big fields, one primary action, no explanatory prose.
pub(crate) fn spawn_founding_form(parent: &mut ChildSpawnerCommands<'_>, view: &FoundingFormView) {
    let draft = view.draft;
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            max_width: Val::Px(720.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(14.0),
            ..default()
        })
        .with_children(|form| {
            form.spawn((
                Text::new(if view.existing == 0 {
                    "NEW COMPANY"
                } else {
                    "ANOTHER COMPANY"
                }),
                TextFont {
                    font_size: FontSize::Px(T_TITLE),
                    ..default()
                },
                TextColor(INK),
            ));

            // NAME
            form.spawn((
                Text::new("NAME"),
                TextFont {
                    font_size: FontSize::Px(T_LABEL),
                    ..default()
                },
                TextColor(INK_MUTED),
            ));
            form.spawn((
                CompanyNameField,
                Button,
                UiRefreshExempt,
                Node {
                    width: Val::Percent(100.0),
                    min_height: Val::Px(46.0),
                    padding: UiRect::axes(Val::Px(14.0), Val::Px(10.0)),
                    align_items: AlignItems::Center,
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(RADIUS)),
                    ..default()
                },
                BackgroundColor(LIMEWASH),
                BorderColor::all(if draft.editing_name {
                    INK
                } else {
                    PLATE_RULE_SOFT
                }),
                UiButtonStyle::new(UiButtonVariant::Secondary).focused(draft.editing_name),
            ))
            .with_child((
                FoundingNameText,
                Text::new(name_text(draft)),
                UiButtonLabel,
                TextFont {
                    font_size: FontSize::Px(T_VALUE),
                    ..default()
                },
                TextColor(INK),
                Pickable::IGNORE,
            ));

            // CAPITAL
            form.spawn((
                Text::new("FOUNDING CAPITAL  /  becomes the company treasury"),
                TextFont {
                    font_size: FontSize::Px(T_LABEL),
                    ..default()
                },
                TextColor(INK_MUTED),
            ));
            form.spawn(Node {
                width: Val::Percent(100.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                ..default()
            })
            .with_children(|row| {
                step_button(row, AdjustFoundingCapital(-500), "-5");
                step_button(row, AdjustFoundingCapital(-100), "-1");
                row.spawn((
                    FoundingCapitalText,
                    Text::new(capital_text(draft)),
                    TextFont {
                        font_size: FontSize::Px(T_VALUE),
                        ..default()
                    },
                    TextColor(INK),
                    TextLayout::justify(Justify::Center),
                    Node {
                        min_width: Val::Px(150.0),
                        ..default()
                    },
                ));
                step_button(row, AdjustFoundingCapital(100), "+1");
                step_button(row, AdjustFoundingCapital(500), "+5");
                row.spawn((
                    Text::new(format!("Wallet {} coin", format_money(view.wallet))),
                    TextFont {
                        font_size: FontSize::Px(T_BODY),
                        ..default()
                    },
                    TextColor(INK_MUTED),
                    Node {
                        margin: UiRect::left(Val::Px(12.0)),
                        ..default()
                    },
                ));
            });

            // ACTION
            form.spawn(Node {
                width: Val::Percent(100.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                margin: UiRect::top(Val::Px(4.0)),
                ..default()
            })
            .with_children(|row| {
                let (label, hall) = match (view.hall, draft.pending) {
                    (_, true) => ("REGISTERING...".to_string(), None),
                    (Some((hall, name)), false) => (
                        format!("FOUND COMPANY AT {}", name.to_uppercase()),
                        Some(hall),
                    ),
                    (None, false) => ("WALK TO A TOWN HALL TO FOUND".to_string(), None),
                };
                let mut button = row.spawn((
                    Button,
                    UiRefreshExempt,
                    Node {
                        flex_grow: 1.0,
                        min_height: Val::Px(46.0),
                        padding: UiRect::axes(Val::Px(14.0), Val::Px(10.0)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(RADIUS)),
                        ..default()
                    },
                    button_chrome(if hall.is_some() {
                        UiButtonVariant::Primary
                    } else {
                        UiButtonVariant::Secondary
                    }),
                ));
                match hall {
                    Some(hall) => {
                        button.insert(FoundCompanyButton { hall });
                    }
                    None => {
                        button.insert(InteractionDisabled);
                    }
                }
                button.with_child((
                    Text::new(label),
                    UiButtonLabel,
                    TextFont {
                        font_size: FontSize::Px(T_BUTTON),
                        ..default()
                    },
                    TextColor(INK),
                    Pickable::IGNORE,
                ));
            });

            if !view.feedback.message.is_empty() {
                form.spawn((
                    Text::new(view.feedback.message.clone()),
                    TextFont {
                        font_size: FontSize::Px(T_BODY),
                        ..default()
                    },
                    TextColor(if view.feedback.success {
                        Color::srgb(0.20, 0.48, 0.27)
                    } else {
                        Color::srgb(0.70, 0.20, 0.16)
                    }),
                ));
            }
        });
}

fn step_button(parent: &mut ChildSpawnerCommands<'_>, marker: impl Component, label: &str) {
    parent
        .spawn((
            marker,
            Button,
            // A press must show at once, even with the pointer still on the
            // button: exempt it from the host panel's hover-deferred refresh.
            UiRefreshExempt,
            Node {
                min_width: Val::Px(52.0),
                min_height: Val::Px(42.0),
                padding: UiRect::axes(Val::Px(12.0), Val::Px(8.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            button_chrome(UiButtonVariant::Secondary),
        ))
        .with_child((
            Text::new(label.to_string()),
            UiButtonLabel,
            TextFont {
                font_size: FontSize::Px(T_BUTTON),
                ..default()
            },
            TextColor(INK),
            Pickable::IGNORE,
        ));
}

/// Reset the draft for a new founder: suggested name, sensible capital.
pub(crate) fn start_draft(
    draft: &mut CompanyFoundingDraft,
    feedback: &mut CompanyFoundingFeedback,
    founder: PersonId,
    hero_name: &str,
    existing: usize,
    wallet: u64,
) {
    draft.founder = Some(founder);
    draft.name = suggested_company_name(hero_name, existing);
    draft.initial_capital = suggested_founding_capital(wallet);
    draft.editing_name = true;
    draft.pending = false;
    feedback.message.clear();
}

fn local_hero<'a>(
    local: Option<&Res<LocalPeerId>>,
    heroes: &'a Query<(
        &Hero,
        &PersonId,
        &CharacterName,
        &PlayerPosition,
        Option<&Wallet>,
    )>,
) -> Option<(
    &'a Hero,
    &'a PersonId,
    &'a CharacterName,
    &'a PlayerPosition,
    Option<&'a Wallet>,
)> {
    let local = local?;
    heroes
        .iter()
        .find(|(hero, ..)| shared::player::peer_id_to_u64(hero.owner) == local.0)
}

/// The Hall the hero stands at, if any.
fn hall_at<'a>(
    position: Vec3,
    settlements: &'a Query<(Entity, &Settlement, &PlayerPosition)>,
) -> Option<(Entity, &'a str)> {
    settlements
        .iter()
        .filter(|(_, _, hall)| {
            Vec2::new(position.x, position.z).distance(Vec2::new(hall.0.x, hall.0.z)) <= HALL_RANGE
        })
        .min_by(|a, b| {
            let da = Vec2::new(position.x, position.z).distance(Vec2::new(a.2 .0.x, a.2 .0.z));
            let db = Vec2::new(position.x, position.z).distance(Vec2::new(b.2 .0.x, b.2 .0.z));
            da.total_cmp(&db)
        })
        .map(|(entity, settlement, _)| (entity, settlement.name.as_str()))
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn handle_founding_buttons(
    mouse: Res<ButtonInput<MouseButton>>,
    property_guard: Res<crate::ui::property_market::PropertyClickGuard>,
    encyclopedia_guard: Res<crate::ui::encyclopedia::ClickGuard>,
    mut buttons: Query<
        (
            &Interaction,
            Option<&CompanyNameField>,
            Option<&AdjustFoundingCapital>,
            Option<&OpenCompanyFounding>,
            Option<&FoundCompanyButton>,
        ),
        (Changed<Interaction>, FoundingControl),
    >,
    local: Option<Res<LocalPeerId>>,
    heroes: Query<(
        &Hero,
        &PersonId,
        &CharacterName,
        &PlayerPosition,
        Option<&Wallet>,
    )>,
    companies: Query<(&CompanyId, &CompanyLeadership)>,
    mut draft: ResMut<CompanyFoundingDraft>,
    mut feedback: ResMut<CompanyFoundingFeedback>,
    mut clients: Query<
        &mut MessageSender<HeroCompanyFoundingOrder>,
        (With<crate::GameClient>, With<Connected>),
    >,
    mut page: ResMut<FoundingPageOpen>,
    mut property_target: ResMut<crate::ui::property_market::PropertyMarketTarget>,
    mut encyclopedia_open: ResMut<EncyclopediaOpen>,
    mut tab: ResMut<EncyclopediaTab>,
) {
    // Whichever window hosts the form armed its own click guard.
    let clicked =
        (property_guard.0 || encyclopedia_guard.0) && mouse.just_pressed(MouseButton::Left);
    let mut clicked_name = false;
    let mut any_pressed = false;
    for (interaction, name, adjust, open, found) in buttons.iter_mut() {
        if !clicked || *interaction != Interaction::Pressed {
            continue;
        }
        any_pressed = true;
        if name.is_some() {
            draft.editing_name = true;
            clicked_name = true;
            continue;
        }
        let Some((_, person, hero_name, _, wallet)) = local_hero(local.as_ref(), &heroes) else {
            continue;
        };
        let balance = wallet.map_or(0, |wallet| wallet.balance());
        let existing = companies
            .iter()
            .filter(|(_, leadership)| leadership.master == *person)
            .count();
        if open.is_some() {
            // One screen for founding: leave the board, open the page.
            start_draft(
                &mut draft,
                &mut feedback,
                *person,
                &hero_name.0,
                existing,
                balance,
            );
            property_target.0 = None;
            page.0 = true;
            encyclopedia_open.0 = true;
            *tab = EncyclopediaTab::Companies;
            continue;
        }
        if let Some(adjust) = adjust {
            let current = i128::from(draft.initial_capital);
            let next = (current + i128::from(adjust.0)).clamp(
                i128::from(PENNIES_PER_COIN),
                i128::from(balance.max(PENNIES_PER_COIN)),
            );
            draft.initial_capital = next as u64;
            continue;
        }
        if let Some(found) = found {
            if draft.pending {
                continue;
            }
            let Ok(mut sender) = clients.single_mut() else {
                feedback.success = false;
                feedback.message = "Company registry is not connected yet.".into();
                continue;
            };
            sender.send::<ReliableChannel>(HeroCompanyFoundingOrder {
                hall: found.hall,
                name: draft.name.clone(),
                initial_capital: draft.initial_capital,
            });
            draft.pending = true;
            draft.editing_name = false;
            feedback.message = "Registering the company with the Hall...".into();
            feedback.success = true;
        }
    }
    // Clicking anywhere else ends name editing. Only consult the guard that
    // is armed, so an unrelated click in the world does not steal focus.
    if clicked && !clicked_name && (any_pressed || draft.editing_name) {
        draft.editing_name = false;
    }
}

fn handle_founding_name_input(
    mut events: MessageReader<KeyboardInput>,
    mut draft: ResMut<CompanyFoundingDraft>,
) {
    if !draft.editing_name || draft.pending {
        // Drain so a stale buffer never types into the next edit.
        events.clear();
        return;
    }
    for event in events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match &event.logical_key {
            Key::Backspace => {
                draft.name.pop();
            }
            Key::Enter | Key::Escape => draft.editing_name = false,
            Key::Character(text) => {
                for character in text.chars() {
                    let allowed =
                        character.is_alphanumeric() || matches!(character, ' ' | '&' | '-' | '\'');
                    if allowed && draft.name.chars().count() < 40 {
                        draft.name.push(character);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Write the draft into the form's text in place. This is what makes +1
/// land instantly: no panel rebuild, no hover deferral.
#[allow(clippy::type_complexity)]
fn sync_founding_texts(
    draft: Res<CompanyFoundingDraft>,
    mut texts: Query<
        (
            &mut Text,
            Option<&FoundingNameText>,
            Option<&FoundingCapitalText>,
        ),
        Or<(With<FoundingNameText>, With<FoundingCapitalText>)>,
    >,
) {
    if !draft.is_changed() {
        return;
    }
    for (mut text, name, capital) in texts.iter_mut() {
        let next = if name.is_some() {
            name_text(&draft)
        } else if capital.is_some() {
            capital_text(&draft)
        } else {
            continue;
        };
        if text.0 != next {
            text.0 = next;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn receive_company_founding_results(
    mut receivers: Query<&mut MessageReceiver<HeroCompanyFoundingResult>, With<crate::GameClient>>,
    mut active: ResMut<ActiveCompany>,
    mut draft: ResMut<CompanyFoundingDraft>,
    mut feedback: ResMut<CompanyFoundingFeedback>,
    mut page: ResMut<FoundingPageOpen>,
    mut selected: ResMut<crate::ui::encyclopedia::companies::SelectedCompany>,
    mut tab: ResMut<EncyclopediaTab>,
) {
    for mut receiver in receivers.iter_mut() {
        for result in receiver.receive() {
            draft.pending = false;
            feedback.message = result.message;
            feedback.success = result.success;
            if result.success {
                active.0 = result.company;
                draft.editing_name = false;
                draft.name.clear();
                if page.0 {
                    // The new company is the thing to look at now.
                    page.0 = false;
                    selected.0 = result.company;
                    *tab = EncyclopediaTab::Companies;
                }
            }
        }
    }
}

/// The encyclopedia's NEW COMPANY page, hosted like the ledgers.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn ensure_founding_page(
    mut commands: Commands,
    time: Res<Time<Real>>,
    page: Res<FoundingPageOpen>,
    mut draft: ResMut<CompanyFoundingDraft>,
    mut feedback: ResMut<CompanyFoundingFeedback>,
    local: Option<Res<LocalPeerId>>,
    heroes: Query<(
        &Hero,
        &PersonId,
        &CharacterName,
        &PlayerPosition,
        Option<&Wallet>,
    )>,
    companies: Query<(&CompanyId, &CompanyLeadership)>,
    settlements: Query<(Entity, &Settlement, &PlayerPosition)>,
    hosts: Query<Entity, With<EncyclopediaPageHost>>,
    roots: Query<(Entity, &FoundingPageRoot, Option<&UiRefreshStamp>)>,
    children: Query<&Children>,
    interactions: Query<(&Interaction, Has<UiRefreshExempt>)>,
    mut encyclopedia_open: ResMut<EncyclopediaOpen>,
    mut tab: ResMut<EncyclopediaTab>,
    mut was_open: Local<bool>,
) {
    if !page.0 {
        *was_open = false;
        for (root, ..) in roots.iter() {
            commands.entity(root).despawn();
        }
        return;
    }
    let Ok(host) = hosts.single() else {
        if !encyclopedia_open.0 {
            encyclopedia_open.0 = true;
            *tab = EncyclopediaTab::Companies;
        }
        return;
    };
    let hero = local_hero(local.as_ref(), &heroes);
    let wallet = hero
        .and_then(|(_, _, _, _, wallet)| wallet)
        .map_or(0, |wallet| wallet.balance());
    let existing = hero.map_or(0, |(_, person, ..)| {
        companies
            .iter()
            .filter(|(_, leadership)| leadership.master == *person)
            .count()
    });
    // Seed the draft when the page opens or the founder changes -- and only
    // then. Backspacing the name to nothing must leave it empty, not reset it.
    let just_opened = !*was_open;
    *was_open = true;
    if let Some((_, person, hero_name, ..)) = hero {
        if just_opened || draft.founder != Some(*person) {
            start_draft(
                &mut draft,
                &mut feedback,
                *person,
                &hero_name.0,
                existing,
                wallet,
            );
        }
    }
    let hall = hero.and_then(|(_, _, _, position, _)| hall_at(position.0, &settlements));
    let signature = format!(
        "{:?}|{:?}|{wallet}|{:?}|{existing}|{}",
        *draft,
        *feedback,
        hall.map(|(entity, name)| (entity, name.to_string())),
        hero.is_some()
    );
    if roots.iter().any(|(_, root, _)| root.signature == signature) {
        return;
    }
    if roots.iter().any(|(entity, _, stamp)| {
        subtree_is_interacting(entity, &children, &interactions)
            || stamp.is_some_and(|stamp| !stamp.is_ready(&time))
    }) {
        return;
    }
    for (root, ..) in roots.iter() {
        commands.entity(root).despawn();
    }
    let panel = commands
        .spawn((
            FoundingPageRoot { signature },
            UiRefreshStamp::now(&time),
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexStart,
                padding: UiRect::all(Val::Px(28.0)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(LIMEWASH_LIT),
        ))
        .id();
    commands.entity(host).add_child(panel);
    commands.entity(panel).with_children(|panel| {
        if hero.is_none() {
            panel.spawn((
                Text::new("Create a Hero first; a company needs a founder."),
                TextFont {
                    font_size: FontSize::Px(T_VALUE),
                    ..default()
                },
                TextColor(INK_MUTED),
            ));
            return;
        }
        spawn_founding_form(
            panel,
            &FoundingFormView {
                draft: &draft,
                feedback: &feedback,
                wallet,
                hall,
                existing,
            },
        );
    });
}

fn cleanup(
    mut commands: Commands,
    roots: Query<Entity, With<FoundingPageRoot>>,
    mut page: ResMut<FoundingPageOpen>,
    mut draft: ResMut<CompanyFoundingDraft>,
    mut feedback: ResMut<CompanyFoundingFeedback>,
) {
    for root in roots.iter() {
        commands.entity(root).despawn();
    }
    page.0 = false;
    *draft = default();
    *feedback = default();
}
