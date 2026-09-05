//! Village fixtures regression fixtures and invariants.

use super::*;

pub(super) fn village_test_app() -> App {
    let mut app = App::new();
    app.init_resource::<crate::world::identity::WorldIdAllocator>()
        .init_resource::<crate::world::identity::WorldIdentityIndex>()
        .init_resource::<BusinessEventQueue>()
        .add_systems(
            PreUpdate,
            (
                crate::world::identity::assign_stable_world_ids,
                crate::world::identity::rebuild_world_identity_index,
                crate::world::identity::reconcile_stable_world_relationships,
                crate::world::identity::reconcile_stable_adjunct_relationships,
                crate::world::identity::reconcile_stable_road_relationships,
                crate::world::identity::reconcile_stable_civic_employment,
            )
                .chain(),
        );
    app
}

pub(super) fn spawn_test_company(app: &mut App, id: u64, cash: u64) -> Entity {
    app.world_mut()
        .spawn((
            shared::components::CompanyId(id),
            shared::economy::CompanyAccount { cash, ..default() },
        ))
        .id()
}
