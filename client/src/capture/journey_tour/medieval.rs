//! Semantic input rehearsal of the production medieval HUD. World records are
//! deliberately offline fixtures; no transaction or network success is faked.

use super::super::{CaptureConfig, CaptureState};
use bevy::{
    ecs::system::SystemParam,
    input::InputSystems,
    prelude::*,
    ui::{InteractionDisabled, UiGlobalTransform, UiSystems},
};
use shared::{
    components::{
        BattalionId, Catapult, CatapultStatus, CharacterName, CommandedBy, Health, Hero,
        MemberOfBattalion, PersonId, PlayerPosition, PlayerRotation, Settlement, SettlementId,
        SettlementSummary,
    },
    economy::Wallet,
};

use crate::{
    input::InputState,
    selection::Selection,
    ui::{
        encyclopedia::{
            DetailName, EncyclopediaOpen, EncyclopediaPanel, EncyclopediaRoot, EncyclopediaTab,
            KnownPeople, SelectedPerson,
        },
        hud::{
            GodNotice,
            chrome::{HudArtwork, HudIcon},
            portrait::PortraitReadiness,
        },
        world_map::MapOpen,
    },
};

#[derive(Resource)]
pub(crate) struct Rehearsal {
    combat: bool,
    staged: bool,
    issued: Option<usize>,
    inspected: Option<usize>,
    catapult: Option<Entity>,
}

pub(super) fn install(app: &mut App) {
    let combat = std::env::var("FISTFORCE_CAPTURE_MEDIEVAL_HUD").as_deref() == Ok("combat");
    app.insert_resource(Rehearsal {
        combat,
        staged: false,
        issued: None,
        inspected: None,
        catapult: None,
    });
    app.add_systems(Update, stage);
    app.add_systems(PreUpdate, input.after(InputSystems).after(UiSystems::Focus));
    app.add_systems(Last, inspect);
}

fn stage(
    mut commands: Commands,
    mut rehearsal: ResMut<Rehearsal>,
    halls: Query<(
        &SettlementId,
        &Settlement,
        &PlayerPosition,
        Option<&PlayerRotation>,
    )>,
    heroes: Query<(Entity, &PersonId), With<Hero>>,
    terrain: Option<Res<shared::terrain::WorldTerrain>>,
) {
    if rehearsal.staged {
        return;
    }
    if rehearsal.combat {
        // The existing WARBAR fixture owns soldiers and initial selection.
        // One offline siege actor exercises the production GLB and mixed HUD.
        let Some(terrain) = terrain else {
            return;
        };
        rehearsal.catapult = Some(
            commands
                .spawn((
                    Catapult { ammunition: 20 },
                    CatapultStatus::default(),
                    Health::new(300.0),
                    CommandedBy("wanderer".into()),
                    PlayerPosition(Vec3::new(-8.0, terrain.get_height(-8.0, -58.0), -58.0)),
                    PlayerRotation(std::f32::consts::PI),
                ))
                .id(),
        );
        rehearsal.staged = true;
        return;
    }
    let Some((hero, person)) = heroes.iter().next() else {
        return;
    };
    let Some((id, town, position, _)) = halls.iter().min_by_key(|(id, ..)| id.0) else {
        return;
    };
    commands.entity(hero).insert((
        CharacterName("Aldric".into()),
        Health::new(100.0),
        Wallet::new(12_000),
    ));
    // Name discovery may have cached the actor before this offline restaging.
    // Queue after the component write so either ordering of production discovery
    // observes the same synthetic identity; never change live roster behavior.
    let person = *person;
    commands.queue(move |world: &mut World| {
        let mut people = world.resource_mut::<KnownPeople>();
        if let Some(record) = people.records.iter_mut().find(|record| record.id == person) {
            record.name = "Aldric".into();
        }
    });
    let summary: SettlementSummary = serde_json::from_value(serde_json::json!({
        "id":id, "name":town.name, "tier":town.tier, "residents":town.residents.max(1),
        "treasury":town.treasury, "prosperity":0.5, "reserve_days":5.0,
        "houses":3, "farmsteads":1, "fishing_huts":0, "lumber_huts":1
    }))
    .expect("medieval HUD directory fixture");
    commands.spawn((summary, position.clone()));
    rehearsal.staged = true;
}

fn action(shot: &str) -> Option<&'static str> {
    match shot {
        "02-character" => Some("hud-EXPAND"),
        "04-map" => Some("hud-MAP"),
        "06-ledger" => Some("hud-LEDGER"),
        "09-notices-open" | "11-final" => Some("journey-NOTICES"),
        "10-notices-clear" => Some("journey-CLEAR"),
        "02-combat-next" => Some("Next battalions"),
        "03-combat-select-last" => Some("Select battalion XVI"),
        "04-combat-orders" | "05-mixed-siege" => Some("Toggle combat orders help"),
        _ => None,
    }
}

fn input(
    config: Res<CaptureConfig>,
    state: Res<CaptureState>,
    mut rehearsal: ResMut<Rehearsal>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut buttons: Query<(&Name, &mut Interaction, Has<InteractionDisabled>), With<Button>>,
    mut notice: ResMut<GodNotice>,
    mut selection: ResMut<Selection>,
    members: Query<(Entity, &MemberOfBattalion)>,
    ready_catapults: Query<(), With<crate::siege::CatapultSceneReady>>,
) {
    mouse.release(MouseButton::Left);
    keys.release(KeyCode::Escape);
    for (_, mut interaction, _) in &mut buttons {
        interaction.set_if_neq(Interaction::None);
    }
    let CaptureState::Settling { shot, .. } = *state else {
        return;
    };
    if rehearsal.issued == Some(shot) {
        return;
    }
    let name = config.shots[shot].name.as_str();
    if matches!(name, "05-mixed-siege" | "06-siege-only") {
        let Some(machine) = rehearsal
            .catapult
            .filter(|entity| ready_catapults.contains(*entity))
        else {
            return;
        };
        // Selection itself is an explicit offline input fixture. It sends no
        // order or simulated result; production controls consume this state.
        let mut selected = if name == "05-mixed-siege" {
            members
                .iter()
                .filter(|(_, member)| member.0 == BattalionId(16))
                .map(|(entity, _)| entity)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        selected.push(machine);
        selection.set(selected);
    }
    if let Some(action) = action(name) {
        let Some((_, mut interaction, disabled)) = buttons
            .iter_mut()
            .find(|(name, ..)| name.as_str() == action)
        else {
            return;
        };
        if disabled {
            return;
        }
        *interaction = Interaction::Pressed;
        mouse.press(MouseButton::Left);
    } else if name.ends_with("-return") {
        keys.press(KeyCode::Escape);
    } else if name == "08-notice" {
        notice.show("Your hero cannot afford the cheapest offer.");
    }
    rehearsal.issued = Some(shot);
}

#[derive(SystemParam)]
pub(crate) struct TourWorld<'w, 's> {
    config: Res<'w, CaptureConfig>,
    geometry: Query<
        'w,
        's,
        (
            &'static ComputedNode,
            &'static UiGlobalTransform,
            &'static InheritedVisibility,
            Option<&'static Visibility>,
        ),
    >,
    modal_roots: Query<'w, 's, Entity, With<crate::ui::modal::ModalRoot>>,
    encyclopedia_roots: Query<'w, 's, Entity, With<EncyclopediaRoot>>,
    encyclopedia_panels: Query<'w, 's, Entity, With<EncyclopediaPanel>>,
    actors: Query<
        'w,
        's,
        (
            &'static GlobalTransform,
            &'static InheritedVisibility,
            Has<crate::hero::HeroDressed>,
        ),
        With<MemberOfBattalion>,
    >,
    cameras: Query<
        'w,
        's,
        (&'static Camera, &'static GlobalTransform),
        With<crate::camera_rts::CommanderCamera>,
    >,
    artwork: Res<'w, HudArtwork>,
    assets: Res<'w, AssetServer>,
    icons: Query<'w, 's, (&'static HudIcon, &'static ImageNode)>,
    nodes: Query<
        'w,
        's,
        (
            Entity,
            &'static Node,
            Option<&'static Name>,
            Option<&'static ChildOf>,
            Option<&'static ComputedNode>,
        ),
    >,
    buttons: Query<'w, 's, (&'static Name, Has<InteractionDisabled>), With<Button>>,
    texts: Query<'w, 's, &'static Text>,
    detail_names: Query<'w, 's, (Entity, &'static Text), With<DetailName>>,
    known_people: Res<'w, KnownPeople>,
    viewports: Query<
        'w,
        's,
        (
            &'static Name,
            &'static ScrollPosition,
            &'static ComputedNode,
        ),
    >,
    portrait: Res<'w, PortraitReadiness>,
    people: Query<'w, 's, &'static PersonId, With<Hero>>,
    selection: Res<'w, Selection>,
    members: Query<'w, 's, &'static MemberOfBattalion>,
    catapults: Query<'w, 's, Entity, (With<Catapult>, With<crate::siege::CatapultSceneReady>)>,
    selected_person: Res<'w, SelectedPerson>,
    encyclopedia: Res<'w, EncyclopediaOpen>,
    tab: Res<'w, EncyclopediaTab>,
    map: Res<'w, MapOpen>,
    input: Res<'w, InputState>,
}

impl TourWorld<'_, '_> {
    fn named(&self, wanted: &str) -> Option<Entity> {
        self.nodes
            .iter()
            .find(|(_, _, name, ..)| name.is_some_and(|name| name.as_str() == wanted))
            .map(|(e, ..)| e)
    }

    fn entity_visible(&self, mut entity: Entity) -> bool {
        let Ok((computed, _, inherited, visibility)) = self.geometry.get(entity) else {
            return false;
        };
        if computed.size().min_element() <= 0.0
            || !inherited.get()
            || visibility.is_some_and(|visibility| *visibility == Visibility::Hidden)
        {
            return false;
        }
        loop {
            let Ok((_, node, _, parent, _)) = self.nodes.get(entity) else {
                return false;
            };
            if node.display == Display::None {
                return false;
            }
            let Some(parent) = parent else {
                return true;
            };
            entity = parent.parent();
        }
    }

    fn bounds(&self, entity: Entity) -> Option<Rect> {
        let (computed, transform, _, _) = self.geometry.get(entity).ok()?;
        let half = computed.size() * 0.5;
        let mut min = Vec2::splat(f32::INFINITY);
        let mut max = Vec2::splat(f32::NEG_INFINITY);
        for corner in [
            Vec2::new(-half.x, -half.y),
            Vec2::new(half.x, -half.y),
            half,
            Vec2::new(-half.x, half.y),
        ] {
            let point = transform.transform_point2(corner);
            min = min.min(point);
            max = max.max(point);
        }
        Some(Rect { min, max })
    }

    fn entity_onscreen(&self, entity: Entity) -> bool {
        self.entity_visible(entity)
            && self.bounds(entity).is_some_and(|rect| {
                intersects_viewport(
                    rect,
                    Vec2::new(
                        self.config.resolution[0] as f32,
                        self.config.resolution[1] as f32,
                    ),
                )
            })
    }

    fn visible(&self, name: &str, check_bounds: bool) -> bool {
        self.named(name).is_some_and(|entity| {
            if check_bounds {
                self.entity_onscreen(entity)
            } else {
                self.entity_visible(entity)
            }
        })
    }

    fn visible_actor_count(&self) -> usize {
        let Ok((camera, transform)) = self.cameras.single() else {
            return 0;
        };
        let Some(size) = camera.logical_viewport_size() else {
            return 0;
        };
        self.actors
            .iter()
            .filter(|(pose, inherited, dressed)| {
                *dressed
                    && inherited.get()
                    && camera
                        .world_to_viewport(transform, pose.translation() + Vec3::Y)
                        .is_ok_and(|point| {
                            point.x > 0.0 && point.y > 0.0 && point.x < size.x && point.y < size.y
                        })
            })
            .count()
    }

    fn check(&self, combat: bool, name: &str, check_bounds: bool) -> Result<(), String> {
        let require = |yes: bool, reason: &str| if yes { Ok(()) } else { Err(reason.to_owned()) };
        require(
            self.artwork.ready(&self.assets),
            "HUD frame and icon assets are not ready",
        )?;
        if combat {
            if check_bounds {
                require(
                    self.visible_actor_count() >= 8,
                    "combat fixture needs at least eight dressed actors in the actual camera view",
                )?;
            }
            let siege = matches!(name, "05-mixed-siege" | "06-siege-only");
            require(
                self.visible("Combat battalion dock", check_bounds) == (name != "06-siege-only"),
                "combat dock must remain for mixed units and hide for catapult-only selection",
            )?;
            require(
                self.visible("Siege controls", check_bounds) == siege,
                "siege panel visibility must follow selected siege units",
            )?;
            if siege {
                require(
                    self.catapults
                        .iter()
                        .any(|entity| self.selection.is_selected(entity)),
                    "selected catapult must have its actual scene ready",
                )?;
                require(
                    self.selection.len() == if name == "05-mixed-siege" { 9 } else { 1 },
                    "siege fixture must select eight soldiers plus a catapult, then only the catapult",
                )?;
                if check_bounds && name == "05-mixed-siege" {
                    require(
                        self.texts.iter().any(|text| text.0 == "9 WILL MOVE"),
                        "mixed selection must count the commanded catapult alongside its soldiers",
                    )?;
                    let dock = self
                        .named("Combat battalion dock")
                        .and_then(|entity| self.bounds(entity))
                        .ok_or("dock bounds missing")?;
                    let siege = self
                        .named("Siege controls")
                        .and_then(|entity| self.bounds(entity))
                        .ok_or("siege bounds missing")?;
                    require(
                        siege.max.y + 8.0 <= dock.min.y,
                        "mixed siege controls must clear the battalion dock by a visible gap",
                    )?;
                }
            }
            let count = self
                .buttons
                .iter()
                .filter(|(name, _)| name.as_str().starts_with("Select battalion "))
                .count();
            require(count == 16, "expected sixteen real battalion cards")?;
            let (_, scroll, computed) = self
                .viewports
                .iter()
                .find(|(name, ..)| name.as_str() == "Battalion card viewport")
                .ok_or("battalion viewport missing")?;
            require(
                computed.content_size().x > computed.size().x,
                "fixture must overflow the battalion viewport",
            )?;
            if name == "01-combat" {
                require(scroll.x <= 0.5, "initial page should start at first card")?;
            }
            if matches!(
                name,
                "02-combat-next" | "03-combat-select-last" | "04-combat-orders"
            ) {
                require(
                    scroll.x > 0.5,
                    "Next battalions must scroll the actual viewport",
                )?;
            }
            if matches!(name, "03-combat-select-last" | "04-combat-orders") {
                require(
                    self.selection.len() == 8,
                    "last battalion selection should contain eight soldiers",
                )?;
                require(
                    self.selection
                        .entities
                        .iter()
                        .all(|e| self.members.get(*e).is_ok_and(|m| m.0 == BattalionId(16))),
                    "last-card action selected another battalion",
                )?;
            }
            require(
                self.visible("Combat orders help", check_bounds) == (name == "04-combat-orders"),
                "orders help toggle did not match requested state",
            )?;
            return Ok(());
        }
        require(
            self.portrait.0,
            "canonical selected-character portrait is not ready",
        )?;
        require(
            !self.buttons.iter().any(|(name, _)| {
                matches!(
                    name.as_str(),
                    "hud-CHARACTER" | "hud-INVENTORY" | "hud-TRADE"
                )
            }),
            "minimal card must retain only its expand action",
        )?;
        let modal = matches!(name, "02-character" | "04-map" | "06-ledger");
        for shell in [
            "Selected character card",
            "World clock",
            "Exploration location",
            "Exploration navigation",
        ] {
            require(
                self.named(shell).is_some(),
                &format!("missing retained HUD element {shell}"),
            )?;
            require(
                self.visible(shell, check_bounds) != modal,
                &format!(
                    "{shell} must {} with modal state",
                    if modal { "hide" } else { "return" }
                ),
            )?;
        }
        require(
            self.input.ui_blocking() == modal,
            "modal input guard did not follow the real action",
        )?;
        require(
            self.map.0 == (name == "04-map"),
            "map action did not open/close the world map",
        )?;
        require(
            self.encyclopedia.0 == matches!(name, "02-character" | "06-ledger"),
            "record/ledger modal state did not follow input",
        )?;
        let expects_encyclopedia = matches!(name, "02-character" | "06-ledger");
        let actual_encyclopedia = self.encyclopedia_roots.iter().any(|root| {
            self.modal_roots.contains(root)
                && if check_bounds {
                    self.entity_onscreen(root)
                } else {
                    self.entity_visible(root)
                }
        }) && self.encyclopedia_panels.iter().any(|panel| {
            if check_bounds {
                self.entity_onscreen(panel)
            } else {
                self.entity_visible(panel)
            }
        });
        require(
            actual_encyclopedia == expects_encyclopedia,
            "the requested encyclopedia must have a real visible modal root and laid-out panel",
        )?;
        if name == "02-character" {
            require(
                *self.tab == EncyclopediaTab::People,
                "Character must open People",
            )?;
            require(
                self.selected_person
                    .0
                    .is_some_and(|id| self.people.iter().any(|person| *person == id)),
                "Character must open the selected hero's stable record",
            )?;
            require(
                self.selected_person
                    .0
                    .and_then(|id| self.known_people.find_by_id(id))
                    .is_some_and(|record| record.name == "Aldric")
                    && self.detail_names.iter().any(|(entity, text)| {
                        text.0 == "Aldric"
                            && if check_bounds {
                                self.entity_onscreen(entity)
                            } else {
                                self.entity_visible(entity)
                            }
                    }),
                "Character header must show the same Aldric identity as the HUD",
            )?;
        }
        require(
            self.visible("Notice details", check_bounds)
                == matches!(name, "09-notices-open" | "10-notices-clear"),
            "notice tray changed its expand/collapse contract",
        )?;
        if name == "09-notices-open" {
            require(
                self.texts
                    .iter()
                    .any(|t| t.0 == "Your hero cannot afford the cheapest offer."),
                "notice must retain the incoming result",
            )?;
        }
        if name == "10-notices-clear" {
            require(
                self.texts.iter().any(|t| t.0 == "No recent messages."),
                "Clear must remove the incoming notice",
            )?;
        }
        Ok(())
    }
}

/// Additional semantic gate for drive_capture. Existing notice-only scenarios
/// have no Rehearsal and continue under their original readiness contract.
pub(crate) fn ready(
    rehearsal: Option<Res<Rehearsal>>,
    mut waiting: Local<u32>,
    state: Res<CaptureState>,
    config: Res<CaptureConfig>,
    world: TourWorld,
) -> bool {
    let Some(rehearsal) = rehearsal else {
        return true;
    };
    let (name, issued, limit) = match *state {
        CaptureState::Warmup { .. } => (
            if rehearsal.combat {
                "01-combat"
            } else {
                "01-hud"
            },
            true,
            config.shots[0]
                .readiness
                .maximum_frames
                .max(config.warmup_frames),
        ),
        CaptureState::Settling { shot, .. } => (
            config.shots[shot].name.as_str(),
            rehearsal.issued == Some(shot),
            config.shots[shot].readiness.maximum_frames,
        ),
        _ => return true,
    };
    let result = if !rehearsal.staged {
        Err("world fixture not staged".into())
    } else if !issued {
        Err(format!(
            "production control for {name} not available/enabled"
        ))
    } else {
        world.check(
            rehearsal.combat,
            name,
            matches!(*state, CaptureState::Settling { .. }),
        )
    };
    if let Err(reason) = result {
        *waiting += 1;
        assert!(
            *waiting < limit,
            "medieval HUD readiness timed out for {name}: {reason}"
        );
        false
    } else {
        *waiting = 0;
        true
    }
}

fn inspect(
    config: Res<CaptureConfig>,
    state: Res<CaptureState>,
    mut rehearsal: ResMut<Rehearsal>,
    world: TourWorld,
) {
    let CaptureState::AwaitingCapture { shot, .. } = *state else {
        return;
    };
    if rehearsal.inspected == Some(shot) {
        return;
    }
    let name = &config.shots[shot].name;
    world
        .check(rehearsal.combat, name, true)
        .expect("production HUD capture contract");
    if name == "12-night" {
        assert!(
            world.icons.iter().any(|(icon, image)| {
                *icon == HudIcon::Moon
                    && image
                        .image
                        .path()
                        .is_some_and(|path| path.path() == std::path::Path::new("ui/hud/moon.png"))
            }),
            "night clock must bind the actual Moon image"
        );
    }
    rehearsal.inspected = Some(shot);
    let named: Vec<_> = ["Selected character card", "World clock", "Exploration location", "Exploration navigation", "Combat battalion dock", "Combat orders help", "Siege controls"]
        .into_iter().filter_map(|name| {
            let entity = world.named(name)?;
            let (_, _, _, _, computed) = world.nodes.get(entity).ok()?;
            Some(serde_json::json!({"name":name, "visible":world.visible(name, true), "screen_bounds":world.bounds(entity).map(|rect| [rect.min.to_array(), rect.max.to_array()]), "pixels":computed.map(|n| n.size().to_array())}))
        }).collect();
    let evidence = serde_json::json!({
        "shot":name, "passed":true, "portrait_ready":world.portrait.0,
        "artwork_ready":world.artwork.ready(&world.assets),
        "map_open":world.map.0, "encyclopedia_open":world.encyclopedia.0,
        "selected_entities":world.selection.len(), "visible_dressed_actors":world.visible_actor_count(), "hud":named,
        "encyclopedia_panel_visible":world.encyclopedia_panels.iter().any(|entity|world.entity_onscreen(entity)),
        "input":"production button and Escape handlers",
        "fixture":"offline hero and town records; no connected simulation claim",
    });
    std::fs::write(
        config.out_dir.join(format!("{name}.hud.json")),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .expect("write HUD rehearsal evidence");
}

fn intersects_viewport(bounds: Rect, size: Vec2) -> bool {
    bounds.min.is_finite()
        && bounds.max.is_finite()
        && bounds.max.x > bounds.min.x
        && bounds.max.y > bounds.min.y
        && bounds.max.x > 0.0
        && bounds.max.y > 0.0
        && bounds.min.x < size.x
        && bounds.min.y < size.y
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_spring_position_is_not_reported_as_visible_hud() {
        let screen = Vec2::new(1280.0, 720.0);
        assert!(!intersects_viewport(
            Rect::from_corners(Vec2::new(350.0, 770.0), Vec2::new(1136.0, 847.0)),
            screen
        ));
        assert!(intersects_viewport(
            Rect::from_corners(Vec2::new(350.0, 629.0), Vec2::new(1136.0, 706.0)),
            screen
        ));
        assert!(!intersects_viewport(
            Rect::from_corners(Vec2::ZERO, Vec2::ZERO),
            screen
        ));
    }
}
