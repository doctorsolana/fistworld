//! One accepted parcel capacity for both embodied and strategic farm labour.

use bevy::ecs::system::SystemParam;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use shared::components::{AttachedTo, BuildingId, FarmField, FARM_FIELDS_PER_FARMSTEAD};

#[derive(Default)]
struct CapacityIndex {
    fields: HashMap<Entity, (BuildingId, f32)>,
    totals: HashMap<BuildingId, f32>,
}

impl CapacityIndex {
    fn remove(&mut self, entity: Entity) {
        if let Some((building, capacity)) = self.fields.remove(&entity) {
            if let Some(total) = self.totals.get_mut(&building) {
                *total = (*total - capacity).max(0.0);
                if *total < 1.0e-6 {
                    self.totals.remove(&building);
                }
            }
        }
    }

    fn insert(&mut self, entity: Entity, building: BuildingId, capacity: f32) {
        if self.fields.get(&entity) == Some(&(building, capacity)) {
            return;
        }
        self.remove(entity);
        self.fields.insert(entity, (building, capacity));
        *self.totals.entry(building).or_default() += capacity;
    }
}

/// Changes update only the affected capacity. Stable ticks do not allocate or
/// recompute polygon areas. The count check repairs missed removal notifications
/// when a lab schedule has been idle for several updates.
#[derive(SystemParam)]
pub struct FarmProductivity<'w, 's> {
    fields: Query<'w, 's, (Entity, &'static FarmField, &'static AttachedTo)>,
    changed: Query<
        'w,
        's,
        (Entity, &'static FarmField, &'static AttachedTo),
        Or<(Changed<FarmField>, Changed<AttachedTo>)>,
    >,
    removed_fields: RemovedComponents<'w, 's, FarmField>,
    removed_attachments: RemovedComponents<'w, 's, AttachedTo>,
    cache: Local<'s, CapacityIndex>,
}

impl FarmProductivity<'_, '_> {
    pub(crate) fn refresh(&mut self) {
        for entity in self
            .removed_fields
            .read()
            .chain(self.removed_attachments.read())
        {
            self.cache.remove(entity);
        }
        for (entity, field, attached) in &self.changed {
            self.cache
                .insert(entity, attached.0, field.productive_fraction());
        }
        if self.cache.fields.len() != self.fields.iter().len() {
            self.cache.fields.clear();
            self.cache.totals.clear();
            for (entity, field, attached) in &self.fields {
                self.cache
                    .insert(entity, attached.0, field.productive_fraction());
            }
        }
    }

    pub(crate) fn field(&self, entity: Entity) -> Option<(&FarmField, &AttachedTo)> {
        self.fields
            .get(entity)
            .ok()
            .map(|(_, field, attached)| (field, attached))
    }

    /// Each worker tends the farm's accepted land, regardless of which visual
    /// work stand it uses. Missing/clipped halves reduce both simulation tiers
    /// identically; extra decorative area never raises the existing output cap.
    pub(crate) fn fraction(&self, building: BuildingId) -> f32 {
        (self.cache.totals.get(&building).copied().unwrap_or(0.0)
            / f32::from(FARM_FIELDS_PER_FARMSTEAD))
        .clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::components::FarmFieldShape;

    #[derive(Resource, Default)]
    struct Observed([f32; 2]);

    fn observe(mut fields: FarmProductivity, mut observed: ResMut<Observed>) {
        fields.refresh();
        observed.0 = [
            fields.fraction(BuildingId(1)),
            fields.fraction(BuildingId(2)),
        ];
    }

    #[test]
    fn capacity_tracks_clipping_reassignment_and_removal_without_worker_slot_bias() {
        let mut app = App::new();
        app.init_resource::<Observed>().add_systems(Update, observe);
        let field = |plot_index, fraction| {
            let mut shape = FarmFieldShape::legacy_rectangle();
            for section in &mut shape.sections {
                section.left *= fraction;
                section.right *= fraction;
            }
            FarmField {
                shape: Some(shape),
                plot_index,
                settlement: "Test".into(),
                farmstead: Vec3::ZERO,
                layout_version: 0,
                quality: 1.0,
            }
        };
        let a = app
            .world_mut()
            .spawn((field(0, 1.0), AttachedTo(BuildingId(1))))
            .id();
        let b = app
            .world_mut()
            .spawn((field(1, 0.5), AttachedTo(BuildingId(1))))
            .id();
        app.update();
        assert_eq!(app.world().resource::<Observed>().0, [0.75, 0.0]);
        app.world_mut().entity_mut(b).insert(field(1, 0.25));
        app.update();
        assert_eq!(app.world().resource::<Observed>().0, [0.625, 0.0]);
        app.world_mut()
            .entity_mut(b)
            .insert(AttachedTo(BuildingId(2)));
        app.update();
        assert_eq!(app.world().resource::<Observed>().0, [0.5, 0.125]);
        app.world_mut().despawn(a);
        app.update();
        assert_eq!(app.world().resource::<Observed>().0, [0.0, 0.125]);
        app.world_mut().entity_mut(b).remove::<AttachedTo>();
        app.update();
        assert_eq!(app.world().resource::<Observed>().0, [0.0, 0.0]);
    }
}
