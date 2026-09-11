//! Personal belongings are knowledge only for the local hero and commanded people.
//! This guards presentation and retained client knowledge. Recipient filtering
//! of replicated economic components remains the server's responsibility.
use super::*;

pub(super) fn can_read_possessions(record: &PersonRecord, account: &str) -> bool {
    record.is_self || (!account.is_empty() && record.commanded_by.as_deref() == Some(account))
}

pub(in crate::ui::encyclopedia) fn enforce_possessions_privacy(
    account: Option<Res<crate::ui::name_entry::PlayerNameInput>>,
    mut people: ResMut<KnownPeople>,
) {
    if !people.is_changed() && !account.as_ref().is_some_and(|input| input.is_changed()) {
        return;
    }
    let account = account
        .as_ref()
        .map(|input| input.name.trim().to_lowercase())
        .unwrap_or_default();
    // Clear rather than merely hiding old values when command is transferred.
    // Do not dirty the registry if it already contains no private information.
    for index in 0..people.records.len() {
        let record = &people.records[index];
        if !can_read_possessions(record, &account)
            && (record.wallet.is_some() || record.inventory.is_some() || record.carried.is_some())
        {
            let record = &mut people.records[index];
            record.wallet = None;
            record.inventory = None;
            record.carried = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    fn record(id: u64, own_hero: bool, commander: Option<&str>) -> PersonRecord {
        PersonRecord {
            id: shared::components::PersonId(id),
            name: "Aldric".into(),
            kind: PersonKind::Villager,
            affiliation: Affiliation::default(),
            level: 0,
            prestige: 0,
            online: true,
            alive: true,
            health: None,
            death_day: None,
            death_cause: None,
            known: true,
            is_self: own_hero,
            commanded_by: commander.map(str::to_owned),
            residence: None,
            home: None,
            occupation: None,
            workplace: None,
            wallet: Some(1234),
            nutrition: None,
            activity: None,
            objective: None,
            day_plan: None,
            navigation: None,
            attributes: None,
            work_status: None,
            daily_wage: None,
            workforce_requirements: None,
            inventory: Some(shared::economy::GoodsInventory::new(10)),
            carried: Some(shared::economy::CarriedLoad::default()),
        }
    }

    #[test]
    fn banner_and_acquaintance_do_not_grant_access_to_personal_belongings() {
        assert!(can_read_possessions(&record(1, true, None), ""));
        assert!(can_read_possessions(
            &record(2, false, Some("player")),
            "player"
        ));
        assert!(!can_read_possessions(
            &record(3, false, Some("other")),
            "player"
        ));
        assert!(!can_read_possessions(&record(4, false, None), "player"));
        assert!(!can_read_possessions(&record(5, false, Some("")), ""));
    }

    #[test]
    fn live_command_removal_updates_authority_without_erasing_remote_retinue() {
        use shared::components::{CharacterName, CommandedBy, PersonId};
        let mut world = World::new();
        let mut people = KnownPeople::default();
        people.records = vec![
            record(1, false, Some("player")),
            record(2, false, Some("player")),
        ];
        world.insert_resource(people);
        let dismissed = world
            .spawn((
                PersonId(1),
                CharacterName("Aldric".into()),
                CommandedBy("player".into()),
            ))
            .id();
        let remote = world
            .spawn((
                PersonId(2),
                CharacterName("Runa".into()),
                CommandedBy("player".into()),
            ))
            .id();
        world
            .run_system_once(super::super::track_retinue_changes)
            .unwrap();
        world.entity_mut(dismissed).remove::<CommandedBy>();
        world.despawn(remote);
        world
            .run_system_once(super::super::track_retinue_changes)
            .unwrap();
        let people = world.resource::<KnownPeople>();
        assert!(people.records[0].commanded_by.is_none());
        assert_eq!(people.records[1].commanded_by.as_deref(), Some("player"));
    }

    #[test]
    fn losing_command_erases_private_values_but_preserves_public_knowledge() {
        let mut world = World::new();
        let mut account = crate::ui::name_entry::PlayerNameInput::default();
        account.name = " Player ".into();
        world.insert_resource(account);
        let mut people = KnownPeople::default();
        people.records = vec![
            record(1, true, None),
            record(2, false, Some("player")),
            record(3, false, None),
        ];
        world.insert_resource(people);
        world.run_system_once(enforce_possessions_privacy).unwrap();
        assert_eq!(
            world.resource::<KnownPeople>().records[1].wallet,
            Some(1234)
        );
        assert_eq!(world.resource::<KnownPeople>().records[2].wallet, None);
        world.resource_mut::<KnownPeople>().records[1].commanded_by = Some("other".into());
        world.run_system_once(enforce_possessions_privacy).unwrap();
        let person = &world.resource::<KnownPeople>().records[1];
        assert!(person.wallet.is_none() && person.inventory.is_none() && person.carried.is_none());
        assert!(person.known);
        assert_eq!(person.name, "Aldric");
        assert_eq!(
            world.resource::<KnownPeople>().records[0].wallet,
            Some(1234)
        );
    }
}
