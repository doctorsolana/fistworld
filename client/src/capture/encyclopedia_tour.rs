//! Opt-in visual rehearsal of the five production encyclopedia tabs.
//! Only replicated world facts are fixtures. Navigation presses the real tabs;
//! transactions and server-authoritative army actions are deliberately not simulated.
use super::{CaptureConfig, CaptureState};
use crate::ui::{
    encyclopedia::{
        self, EncyclopediaOpen, EncyclopediaPanel, EncyclopediaTab, KnownPeople, PersonKind,
        SelectedPerson, TabBody, TabButton,
    },
    ledger::{LedgerArtwork, LedgerIllustration},
    portraits::{PersonPortrait, PortraitMetrics, PortraitStatus},
};
use bevy::{
    ecs::system::SystemParam,
    input::InputSystems,
    prelude::*,
    ui::{InteractionDisabled, UiGlobalTransform, UiSystems},
};
use shared::components::*;

#[derive(Resource, Default)]
pub(super) struct Rehearsal {
    staged: bool,
    army_staged: bool,
    issued: Option<usize>,
    inspected: Option<usize>,
    source_preparations: Option<u64>,
    warm_completed: Option<u64>,
}

pub(super) fn install(app: &mut App) {
    if std::env::var("FISTFORCE_CAPTURE_ENCYCLOPEDIA_TOUR").as_deref() != Ok("1") {
        return;
    }
    app.init_resource::<Rehearsal>();
    app.add_systems(Update, stage);
    app.add_systems(PreUpdate, input.after(InputSystems).after(UiSystems::Focus));
    app.add_systems(Last, inspect);
}

fn stage(
    mut commands: Commands,
    mut rehearsal: ResMut<Rehearsal>,
    heroes: Query<Entity, With<Hero>>,
    halls: Query<(Entity, &SettlementId, &Settlement)>,
    buildings: Query<(
        Entity,
        &SettlementBuilding,
        Option<&BuildingOf>,
        Option<&BuildingId>,
    )>,
    terrain: Option<Res<shared::terrain::WorldTerrain>>,
) {
    if rehearsal.staged {
        return;
    }
    let Some(hero) = heroes.iter().next() else {
        return;
    };
    let Some((hall, town, settlement)) =
        halls.iter().find(|(_, _, town)| town.name == "Brackwater")
    else {
        return;
    };
    let Some(terrain) = terrain else {
        return;
    };
    commands
        .entity(hall)
        .insert(CivicHallLevel::for_tier(settlement.tier));
    commands.insert_resource(crate::ui::name_entry::PlayerNameInput {
        name: "Wanderer".into(),
        submitted: true,
    });
    // The legacy village fixture predates durable building ownership. Give its
    // real local structures identities so the same Places joins used online
    // can display them; company fixtures already carry their own valid ids.
    for (index, (entity, building, owner, id)) in buildings.iter().enumerate() {
        if building.settlement == "Brackwater" && owner.is_none() {
            commands.entity(entity).insert(BuildingOf(*town));
            if id.is_none() {
                commands
                    .entity(entity)
                    .insert(BuildingId(7000 + index as u64));
            }
        }
    }
    commands.entity(hero).insert((
        PersonId(1),
        CharacterName("Aldric".into()),
        CharacterAttributes::new(14, 11, 12),
        Health::new(100.0),
        shared::economy::Wallet::new(1200),
        {
            let mut inventory =
                shared::economy::GoodsInventory::new(shared::economy::capacity::VILLAGER);
            assert_eq!(inventory.add(shared::economy::Good::Wheat, 3), 3);
            assert_eq!(inventory.add(shared::economy::Good::Bread, 1), 1);
            inventory
        },
    ));
    for (index, (name, occupation, controlled, nearby)) in [
        ("Sigrid", "Farmer", false, true),
        ("Oswin", "Woodcutter", true, true),
        ("Runa", "Baker", true, true),
        ("Marta", "Miller", true, false),
    ]
    .into_iter()
    .enumerate()
    {
        let x = 14.0 + index as f32 * 2.5;
        let mut health = Health::new(100.0);
        health.current = 92.0 - index as f32 * 4.0;
        let mut actor = commands.spawn((
            PersonId(index as u64 + 2),
            CharacterName(name.into()),
            CharacterKind::Villager,
            HeroOutfit::varied(22 + index as u64 * 19),
            CharacterAttributes::new(12 + index as u8, 9, 10),
            health,
            Residence("Brackwater".into()),
            Occupation(Some(occupation.into())),
            CharacterActivity::Idle,
            CharacterObjective::WalkingAroundTown,
            shared::economy::Wallet::new(840 + index as u64 * 220),
            shared::economy::GoodsInventory::new(shared::economy::capacity::VILLAGER),
        ));
        if controlled {
            actor.insert(CommandedBy("wanderer".into()));
        }
        // An observed appearance can remain known without a currently embodied position.
        if nearby {
            actor.insert((
                PlayerPosition(Vec3::new(x, terrain.get_height(x, 12.0), 12.0)),
                PlayerRotation(0.0),
            ));
        }
    }
    let town = *town;
    commands.queue(move |world: &mut World| {
        let mut people = world.resource_mut::<KnownPeople>();
        people.records.retain(|record| record.id.0 <= 5);
        for record in &mut people.records {
            let index = record.id.0 as usize - 1;
            record.name = ["Aldric", "Sigrid", "Oswin", "Runa", "Marta"][index].into();
            record.kind = if index == 0 {
                PersonKind::Hero
            } else {
                PersonKind::Villager
            };
            record.is_self = index == 0;
            record.known = true;
            record.commanded_by = (index == 0 || index >= 2).then(|| "wanderer".into());
        }
        world.resource_mut::<EncyclopediaOpen>().0 = true;
        *world.resource_mut::<EncyclopediaTab>() = EncyclopediaTab::People;
        world.resource_mut::<SelectedPerson>().0 = Some(PersonId(2));
        world
            .resource_mut::<encyclopedia::places::SelectedPlace>()
            .0 = Some(town);
        *world.resource_mut::<encyclopedia::places::SelectedPlaceEntry>() =
            encyclopedia::places::SelectedPlaceEntry::Overview;
        world.resource_mut::<crate::ui::hud::GodCapability>().0 = false;
    });
    rehearsal.staged = true;
}

fn expected_tab(name: &str) -> EncyclopediaTab {
    match name {
        "02-places" => EncyclopediaTab::Places,
        "03-retinue" => EncyclopediaTab::Retinue,
        "04-army" => EncyclopediaTab::Army,
        "05-companies" => EncyclopediaTab::Companies,
        _ => EncyclopediaTab::People,
    }
}

fn stage_army(commands: &mut Commands, manifest: &crate::hero::HeroManifest) {
    for (ordinal, name) in [
        (1, "The Ashen Spears"),
        (2, "Riverwatch"),
        (3, "The Oak Guard"),
    ] {
        commands.spawn((
            Battalion {
                id: BattalionId(ordinal),
                name: name.into(),
                ordinal,
            },
            CommandedBy("wanderer".into()),
            SoldierRole::Infantry,
            PlayerPosition(Vec3::ZERO),
        ));
    }
    for index in 0..18u64 {
        let mut outfit = HeroOutfit::varied(200 + index);
        manifest
            .0
            .apply_outfit("soldier_mail", &mut outfit)
            .expect("canonical soldier outfit");
        let mut actor = commands.spawn((
            PersonId(9500 + index),
            CharacterName(format!("Retainer {}", index + 1)),
            CharacterKind::Villager,
            outfit,
            CharacterAttributes::from_seed(index * 31 + 5),
            Health::new(100.0),
            CommandedBy("wanderer".into()),
            SoldierRole::Infantry,
            PlayerPosition(Vec3::new(index as f32 * 2.0, 0.0, 35.0)),
        ));
        if index < 15 {
            actor.insert(MemberOfBattalion(BattalionId(index / 5 + 1)));
        }
    }
}

fn input(
    config: Res<CaptureConfig>,
    state: Res<CaptureState>,
    mut rehearsal: ResMut<Rehearsal>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut tabs: Query<(&TabButton, &mut Interaction, Has<InteractionDisabled>), With<Button>>,
    mut commands: Commands,
    manifest: Res<crate::hero::HeroManifest>,
    mut selected: ResMut<SelectedPerson>,
) {
    mouse.release(MouseButton::Left);
    for (_, mut interaction, _) in &mut tabs {
        interaction.set_if_neq(Interaction::None);
    }
    let CaptureState::Settling { shot, .. } = *state else {
        return;
    };
    if rehearsal.issued == Some(shot) {
        return;
    }
    let desired = expected_tab(&config.shots[shot].name);
    let Some((_, mut interaction, false)) =
        tabs.iter_mut().find(|(button, ..)| button.0 == desired)
    else {
        return;
    };
    if desired == EncyclopediaTab::Army && !rehearsal.army_staged {
        stage_army(&mut commands, &manifest);
        rehearsal.army_staged = true;
    }
    // Person identity is explicit offline fixture selection; the tab itself
    // still goes through the real retained button action.
    if config.shots[shot].name == "06-own-record" {
        selected.0 = Some(PersonId(1));
    }
    if config.shots[shot].name == "07-people-warm" {
        selected.0 = Some(PersonId(2));
    }
    *interaction = Interaction::Pressed;
    mouse.press(MouseButton::Left);
    rehearsal.issued = Some(shot);
}

#[derive(SystemParam)]
pub(super) struct TourWorld<'w, 's> {
    config: Res<'w, CaptureConfig>,
    open: Res<'w, EncyclopediaOpen>,
    tab: Res<'w, EncyclopediaTab>,
    assets: Res<'w, AssetServer>,
    art: Res<'w, LedgerArtwork>,
    portraits: Res<'w, PortraitMetrics>,
    people: Res<'w, KnownPeople>,
    selected: Res<'w, SelectedPerson>,
    army: Res<'w, crate::army_roster::ArmyRoster>,
    geometry: Query<
        'w,
        's,
        (
            &'static Node,
            &'static ComputedNode,
            &'static UiGlobalTransform,
            &'static InheritedVisibility,
            Option<&'static ChildOf>,
        ),
    >,
    panels: Query<'w, 's, Entity, With<EncyclopediaPanel>>,
    bodies: Query<'w, 's, (Entity, &'static TabBody)>,
    portraits_nodes: Query<'w, 's, (Entity, &'static PersonPortrait, &'static PortraitStatus)>,
    illustrations: Query<'w, 's, (Entity, &'static ImageNode), With<LedgerIllustration>>,
    texts: Query<'w, 's, (Entity, &'static Text)>,
    named: Query<'w, 's, (Entity, &'static Name)>,
}

impl TourWorld<'_, '_> {
    fn bounds(&self, entity: Entity) -> Option<Rect> {
        let (_, computed, pose, _, _) = self.geometry.get(entity).ok()?;
        let half = computed.size() * 0.5;
        let mut min = Vec2::splat(f32::INFINITY);
        let mut max = Vec2::splat(f32::NEG_INFINITY);
        for corner in [
            Vec2::new(-half.x, -half.y),
            Vec2::new(half.x, -half.y),
            half,
            Vec2::new(-half.x, half.y),
        ] {
            let point = pose.transform_point2(corner);
            min = min.min(point);
            max = max.max(point);
        }
        Some(Rect { min, max })
    }
    fn visible(&self, mut entity: Entity) -> bool {
        let Some(rect) = self.bounds(entity) else {
            return false;
        };
        if rect.max.x <= 0.0
            || rect.max.y <= 0.0
            || rect.min.x >= self.config.resolution[0] as f32
            || rect.min.y >= self.config.resolution[1] as f32
        {
            return false;
        }
        loop {
            let Ok((node, computed, _, inherited, parent)) = self.geometry.get(entity) else {
                return false;
            };
            if node.display == Display::None
                || computed.size().min_element() <= 0.0
                || !inherited.get()
            {
                return false;
            }
            // Test the actual clipping rectangles as well as inherited display.
            if (node.overflow.x != OverflowAxis::Visible
                || node.overflow.y != OverflowAxis::Visible)
                && self.bounds(entity).is_some_and(|clip| {
                    rect.max.x <= clip.min.x
                        || rect.min.x >= clip.max.x
                        || rect.max.y <= clip.min.y
                        || rect.min.y >= clip.max.y
                })
            {
                return false;
            }
            let Some(parent) = parent else {
                return true;
            };
            entity = parent.parent();
        }
    }
    fn check(&self, name: &str) -> Result<(), String> {
        let require = |condition, reason: &str| {
            if condition {
                Ok(())
            } else {
                Err(reason.to_owned())
            }
        };
        require(self.open.0, "encyclopedia must remain open")?;
        require(
            *self.tab == expected_tab(name),
            "production tab action has not selected requested page",
        )?;
        require(
            self.art.ready(&self.assets),
            "book material artwork is still loading",
        )?;
        let panel = self
            .panels
            .single()
            .map_err(|_| "one encyclopedia panel is required")?;
        require(self.visible(panel), "book has no visible layout")?;
        let bounds = self.bounds(panel).unwrap();
        require(
            bounds.min.x >= -1.0
                && bounds.min.y >= -1.0
                && bounds.max.x <= self.config.resolution[0] as f32 + 1.0
                && bounds.max.y <= self.config.resolution[1] as f32 + 1.0,
            "book must fit the actual viewport",
        )?;
        require(
            self.bodies
                .iter()
                .filter(|(entity, _)| self.visible(*entity))
                .count()
                == 1,
            "exactly one top-level page must be visible",
        )?;
        for (entity, image) in &self.illustrations {
            if self.visible(entity) {
                require(
                    self.assets.is_loaded_with_dependencies(image.image.id()),
                    "visible illustration is not ready",
                )?;
            }
        }
        for (entity, _, status) in &self.portraits_nodes {
            if self.visible(entity) {
                require(status.ready, "visible portrait work remains queued")?;
            }
        }
        require(
            self.portraits.bytes <= 32 * 1024 * 1024,
            "portrait texture budget exceeded",
        )?;
        if matches!(name, "01-people" | "07-people-warm") {
            require(
                self.selected.0 == Some(PersonId(2)),
                "expected selected Sigrid record",
            )?;
            require(
                self.people.find_by_id(PersonId(2)).is_some_and(|person| {
                    person.wallet.is_none()
                        && person.inventory.is_none()
                        && person.carried.is_none()
                }),
                "uncommanded person's cached possessions must be private",
            )?;
            require(
                self.portraits_nodes.iter().any(|(entity, id, status)| {
                    id.0 == PersonId(2) && status.known && status.ready && self.visible(entity)
                }),
                "selected actual observed portrait is not ready",
            )?;
        }
        if name == "06-own-record" {
            require(
                self.selected.0 == Some(PersonId(1)),
                "owned record must select the actual fixture hero",
            )?;
            require(
                self.texts
                    .iter()
                    .any(|(entity, text)| text.0 == "Your hero" && self.visible(entity)),
                "present hero must not be described as absent for lacking NPC activity",
            )?;
            require(
                self.people.find_by_id(PersonId(1)).is_some_and(|record| {
                    record.wallet == Some(1200)
                        && record.inventory.as_ref().is_some_and(|inventory| {
                            inventory.amount(shared::economy::Good::Wheat) == 3
                                && inventory.amount(shared::economy::Good::Bread) == 1
                        })
                }),
                "owned possessions must retain exact authoritative fixture values",
            )?;
            for (good, amount) in [
                (shared::economy::Good::Wheat, 3),
                (shared::economy::Good::Bread, 1),
            ] {
                let label = format!("{} ×{amount}", good.label());
                require(
                    self.texts
                        .iter()
                        .any(|(entity, text)| text.0 == label && self.visible(entity)),
                    "owned inventory must display both actual goods quantities",
                )?;
                let image: Handle<Image> = self.assets.load(crate::ui::good_icon_path(good));
                require(
                    self.assets.is_loaded_with_dependencies(image.id()),
                    "owned goods icon has not loaded",
                )?;
            }
        }
        if name == "03-retinue" {
            require(
                self.texts
                    .iter()
                    .any(|(entity, text)| text.0 == "Unknown" && self.visible(entity)),
                "retinue must show unavailable member honestly",
            )?;
            require(
                self.texts
                    .iter()
                    .any(|(entity, text)| text.0 == "Your hero" && self.visible(entity)),
                "present retinue hero must have a truthful activity description",
            )?;
            require(
                self.texts.iter().any(|(entity, text)| {
                    text.0 == "Beyond your sight · Miller of Brackwater" && self.visible(entity)
                }),
                "unavailable retainer must not claim a cached activity is current",
            )?;
        }
        if name == "04-army" {
            require(
                self.army.battalions.len() == 3,
                "three real battalion component fixtures must reach army roster",
            )?;
        }
        if name == "04-army" {
            for name in ["Army member viewport", "Army available viewport"] {
                let entity = self
                    .named
                    .iter()
                    .find(|(_, marker)| marker.as_str() == name)
                    .map(|(entity, _)| entity)
                    .ok_or_else(|| format!("missing {name}"))?;
                require(self.visible(entity), "both troop panes must be visible")?;
                require(
                    self.bounds(entity)
                        .is_some_and(|bounds| bounds.height() >= 100.0),
                    "each troop viewport must retain at least 100 physical pixels for readable rows",
                )?;
            }
        }
        Ok(())
    }
}

pub(super) fn ready(
    rehearsal: Option<Res<Rehearsal>>,
    state: Res<CaptureState>,
    config: Res<CaptureConfig>,
    world: TourWorld,
    mut waiting: Local<u32>,
) -> bool {
    let Some(rehearsal) = rehearsal else {
        return true;
    };
    let (name, issued, limit) = match *state {
        CaptureState::Warmup { .. } => (
            "01-people",
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
    let check = if !rehearsal.staged {
        Err("cast not staged".into())
    } else if !issued {
        Err("tab input not issued".into())
    } else {
        world.check(name)
    };
    let check=check.and_then(|()| {
        if name=="07-people-warm" && (rehearsal.source_preparations!=Some(world.portraits.source_preparations)
            || rehearsal.warm_completed!=Some(world.portraits.completed)) {
            Err("returning to an already rendered person must reuse its portrait and immutable sources".into())
        } else {Ok(())}
    });
    match check {
        Ok(()) => {
            *waiting = 0;
            true
        }
        Err(reason) => {
            *waiting += 1;
            assert!(
                *waiting < limit,
                "encyclopedia capture readiness timed out for {name}: {reason}"
            );
            false
        }
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
        .check(name)
        .unwrap_or_else(|reason| panic!("encyclopedia capture failed: {reason}"));
    if name == "01-people" {
        rehearsal.source_preparations = Some(world.portraits.source_preparations);
    }
    if name == "06-own-record" {
        rehearsal.warm_completed = Some(world.portraits.completed);
    }
    let panel = world.panels.single().unwrap();
    let bounds = world.bounds(panel).unwrap();
    let visible_portraits = world
        .portraits_nodes
        .iter()
        .filter(|(entity, ..)| world.visible(*entity))
        .count();
    let known_portraits = world
        .portraits_nodes
        .iter()
        .filter(|(entity, _, status)| status.known && world.visible(*entity))
        .count();
    let evidence = serde_json::json!({"shot":name,"passed":true,"input":"production tab button handlers",
        "fixture":"offline economic and personal facts, not server action validation",
        "panel_bounds":{"min":bounds.min.to_array(),"max":bounds.max.to_array()},
        "portraits":{"visible":visible_portraits,"known":known_portraits,"bytes":world.portraits.bytes,
            "queued":world.portraits.queued,"pending":world.portraits.pending,"source_preparations":world.portraits.source_preparations,
            "completed":world.portraits.completed,"discarded":world.portraits.discarded}});
    std::fs::write(
        config.out_dir.join(format!("{name}.encyclopedia.json")),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .expect("encyclopedia capture evidence");
    rehearsal.inspected = Some(shot);
}
