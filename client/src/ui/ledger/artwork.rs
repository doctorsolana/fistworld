//! Shared, lazily bound material and canonical illustration handles.

use bevy::prelude::*;
use shared::components::{SettlementBuildingKind, SettlementTier};

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LedgerIcon {
    Army,
    Retinue,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum LedgerIllustration {
    Hamlet,
    Village,
    Town,
    City,
    HallVillage,
    HallTown,
    HouseL2,
    HouseLong,
    HouseLongL2,
    Hall,
    House,
    Lumberjack,
    Windmill,
    Storage,
    Bakery,
    Farm,
    Quarry,
    Company,
    Fisherman,
    Market,
    Tavern,
    Church,
    Livestock,
}

impl LedgerIllustration {
    /// A settlement's scale is separate from the physical upgrade of its hall.
    /// Ruins use the smallest plate until a dedicated ruin illustration exists.
    pub(crate) fn settlement(tier: SettlementTier) -> Self {
        match tier {
            SettlementTier::Ruins | SettlementTier::Hamlet => Self::Hamlet,
            SettlementTier::Village => Self::Village,
            SettlementTier::Town => Self::Town,
            SettlementTier::City => Self::City,
        }
    }

    pub(crate) fn building(kind: SettlementBuildingKind) -> Self {
        match kind {
            SettlementBuildingKind::Hall => Self::Hall,
            SettlementBuildingKind::House => Self::House,
            SettlementBuildingKind::LumberjackHut => Self::Lumberjack,
            SettlementBuildingKind::Windmill => Self::Windmill,
            SettlementBuildingKind::StorageHall => Self::Storage,
            SettlementBuildingKind::Bakery => Self::Bakery,
            SettlementBuildingKind::Farmstead => Self::Farm,
            SettlementBuildingKind::StoneQuarry => Self::Quarry,
            SettlementBuildingKind::FishermansHut => Self::Fisherman,
            SettlementBuildingKind::Market => Self::Market,
            SettlementBuildingKind::Tavern => Self::Tavern,
            SettlementBuildingKind::Church => Self::Church,
            SettlementBuildingKind::LivestockFarm => Self::Livestock,
        }
    }

    pub(super) fn path(self) -> &'static str {
        match self {
            Self::Hamlet => "ui/ledger/hamlet.jpg",
            Self::Village => "ui/ledger/village.jpg",
            Self::Town => "ui/ledger/town.jpg",
            Self::City => "ui/ledger/city.jpg",
            Self::HallVillage => "ui/ledger/buildings/hall-village.jpg",
            Self::HallTown => "ui/ledger/buildings/hall-town.jpg",
            Self::HouseL2 => "ui/ledger/buildings/house-l2.jpg",
            Self::HouseLong => "ui/ledger/buildings/house-long.jpg",
            Self::HouseLongL2 => "ui/ledger/buildings/house-long-l2.jpg",
            Self::Hall => "ui/ledger/buildings/hall.jpg",
            Self::House => "ui/ledger/buildings/house.jpg",
            Self::Lumberjack => "ui/ledger/buildings/lumberjack.jpg",
            Self::Windmill => "ui/ledger/buildings/windmill.jpg",
            Self::Storage => "ui/ledger/buildings/storage.jpg",
            Self::Bakery => "ui/ledger/buildings/bakery.jpg",
            Self::Farm => "ui/ledger/buildings/farm.jpg",
            Self::Quarry => "ui/ledger/buildings/quarry.jpg",
            Self::Company => "ui/hud/scales.png",
            Self::Fisherman => "ui/ledger/buildings/fisherman.jpg",
            Self::Market => "ui/ledger/buildings/market.jpg",
            Self::Tavern => "ui/ledger/buildings/tavern.jpg",
            Self::Church => "ui/ledger/buildings/church.jpg",
            Self::Livestock => "ui/ledger/buildings/livestock.jpg",
        }
    }
}

#[derive(Component, Clone, Copy)]
pub(super) enum Surface {
    Paper,
    DirectoryPaper,
    Wood,
    Pennant,
    Corner,
}

/// The brass ring is drawn above its mutable portrait child at every size.
#[derive(Component)]
pub(super) struct PortraitFrame;

#[derive(Resource)]
pub(crate) struct LedgerArtwork {
    paper: Handle<Image>,
    wood: Handle<Image>,
    portrait: Handle<Image>,
    pennant: Handle<Image>,
    corner: Handle<Image>,
    army: Handle<Image>,
    retinue: Handle<Image>,
    pub(super) button_paper: Handle<Image>,
    pub(super) button_wood: Handle<Image>,
    pub(super) button_close: Handle<Image>,
}

impl LedgerArtwork {
    pub(crate) fn ready(&self, assets: &AssetServer) -> bool {
        [
            &self.paper,
            &self.wood,
            &self.portrait,
            &self.pennant,
            &self.corner,
            &self.army,
            &self.retinue,
            &self.button_paper,
            &self.button_wood,
            &self.button_close,
        ]
        .into_iter()
        .all(|image| assets.is_loaded_with_dependencies(image.id()))
    }
}

pub(super) fn load_artwork(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(LedgerArtwork {
        paper: assets.load("ui/ledger/paper.jpg"),
        wood: assets.load("ui/ledger/wood.jpg"),
        portrait: assets.load("ui/ledger/portrait-frame.png"),
        pennant: assets.load("ui/ledger/pennant.png"),
        corner: assets.load("ui/ledger/corner.png"),
        army: assets.load("ui/ledger/army.png"),
        retinue: assets.load("ui/ledger/retinue.png"),
        button_paper: assets.load("ui/ledger/button-paper.png"),
        button_wood: assets.load("ui/ledger/button-wood.png"),
        button_close: assets.load("ui/ledger/button-close.png"),
    });
}

pub(super) fn bind_portrait_frames(
    mut commands: Commands,
    art: Res<LedgerArtwork>,
    frames: Query<Entity, Added<PortraitFrame>>,
) {
    for entity in &frames {
        commands.entity(entity).with_child((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            ImageNode::new(art.portrait.clone()),
            ZIndex(5),
            Pickable::IGNORE,
        ));
    }
}

pub(super) fn bind_surfaces(
    art: Res<LedgerArtwork>,
    mut surfaces: Query<(&Surface, &mut ImageNode), Changed<Surface>>,
) {
    for (surface, mut image) in &mut surfaces {
        image.image = match surface {
            Surface::Paper | Surface::DirectoryPaper => &art.paper,
            Surface::Wood => &art.wood,
            Surface::Pennant => &art.pennant,
            Surface::Corner => &art.corner,
        }
        .clone();
        image.color = match surface {
            Surface::DirectoryPaper => Color::srgb(0.93, 0.91, 0.85),
            Surface::Wood => Color::srgb(0.67, 0.65, 0.61),
            _ => Color::WHITE,
        };
    }
}

pub(super) fn bind_icons(
    art: Res<LedgerArtwork>,
    mut icons: Query<(&LedgerIcon, &mut ImageNode), Changed<LedgerIcon>>,
) {
    for (icon, mut image) in &mut icons {
        image.image = match icon {
            LedgerIcon::Army => &art.army,
            LedgerIcon::Retinue => &art.retinue,
        }
        .clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settlement_tiers_have_distinct_shared_illustrations() {
        let cases = [
            (
                SettlementTier::Hamlet,
                LedgerIllustration::Hamlet,
                "ui/ledger/hamlet.jpg",
            ),
            (
                SettlementTier::Village,
                LedgerIllustration::Village,
                "ui/ledger/village.jpg",
            ),
            (
                SettlementTier::Town,
                LedgerIllustration::Town,
                "ui/ledger/town.jpg",
            ),
            (
                SettlementTier::City,
                LedgerIllustration::City,
                "ui/ledger/city.jpg",
            ),
        ];
        let mut paths = std::collections::HashSet::new();
        for (tier, expected, path) in cases {
            let illustration = LedgerIllustration::settlement(tier);
            assert_eq!(illustration, expected);
            assert_eq!(illustration.path(), path);
            assert!(paths.insert(path));
        }
        assert_eq!(
            LedgerIllustration::settlement(SettlementTier::Ruins),
            LedgerIllustration::Hamlet
        );
    }

    #[test]
    fn brass_ring_is_above_the_mutable_face_and_is_only_created_once() {
        let mut app = App::new();
        app.insert_resource(LedgerArtwork {
            paper: default(),
            wood: default(),
            portrait: default(),
            pennant: default(),
            corner: default(),
            army: default(),
            retinue: default(),
            button_paper: default(),
            button_wood: default(),
            button_close: default(),
        });
        app.add_systems(Update, bind_portrait_frames);
        let frame = app
            .world_mut()
            .spawn(super::super::widgets::portrait_frame(184.0))
            .id();
        let face = app
            .world_mut()
            .spawn((
                crate::ui::portraits::person(shared::components::PersonId(17), 172.0),
                ChildOf(frame),
            ))
            .id();
        app.update();
        let children = app.world().get::<Children>(frame).unwrap().to_vec();
        assert_eq!(children.len(), 2);
        assert!(children.contains(&face));
        let ring = *children.iter().find(|child| **child != face).unwrap();
        assert_eq!(app.world().get::<ZIndex>(ring).unwrap().0, 5);
        assert_eq!(
            app.world().get::<Node>(ring).unwrap().position_type,
            PositionType::Absolute
        );
        assert_eq!(
            *app.world().get::<Pickable>(ring).unwrap(),
            Pickable::IGNORE
        );
        app.world_mut()
            .get_mut::<crate::ui::portraits::PersonPortrait>(face)
            .unwrap()
            .0 = shared::components::PersonId(18);
        app.update();
        assert_eq!(
            app.world().get::<Children>(frame).unwrap().to_vec(),
            children
        );
    }

    #[test]
    fn textured_book_surfaces_preserve_native_borders() {
        let mut world = World::new();
        let paper = world.spawn(super::super::widgets::paper()).id();
        let wood = world.spawn(super::super::widgets::wood()).id();
        for entity in [paper, wood] {
            assert_eq!(
                world.get::<ImageNode>(entity).unwrap().visual_box,
                bevy::ui::VisualBox::PaddingBox
            );
        }
    }
}
