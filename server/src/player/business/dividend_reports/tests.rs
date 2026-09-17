use super::*;
use lightyear::prelude::server::ClientOf;
use shared::components::{CompanyId, PersonId};

fn paid_outcome(link: Entity) -> DividendOutcome {
    DividendOutcome {
        requester_link: link,
        requester_person: PersonId(7),
        company: CompanyId(42),
        paid: 30_000,
        per_ten_shares: 300,
        shareholders: 2,
        own_shares: 600,
        own_take: 18_000,
        from_profit: 20_000,
        return_of_capital: 10_000,
        withheld_reserves: 1_250,
        refusal: None,
    }
}

#[test]
fn report_dividend_outcomes_sends_one_result_and_drops_disconnected_links() {
    let mut app = App::new();
    app.init_resource::<CompanyDividendOutcomes>();
    app.add_systems(Update, report_dividend_outcomes);
    let connected = app
        .world_mut()
        .spawn((ClientOf, MessageSender::<HeroCompanyResult>::default()))
        .id();
    let gone = app
        .world_mut()
        .spawn((ClientOf, MessageSender::<HeroCompanyResult>::default()))
        .id();
    app.world_mut().despawn(gone);
    app.update();
    app.world_mut().clear_trackers();
    {
        let mut outcomes = app.world_mut().resource_mut::<CompanyDividendOutcomes>();
        outcomes.push(paid_outcome(connected));
        outcomes.push(DividendOutcome {
            refusal: Some(DividendRefusal::NothingDistributable),
            ..paid_outcome(gone)
        });
    }

    // `App::update` clears trackers afterwards; run the schedule directly so
    // `Changed` filters observe this frame.
    app.world_mut().run_schedule(Update);

    assert!(
        app.world().resource::<CompanyDividendOutcomes>().is_empty(),
        "every outcome is consumed once, including one for a vanished link"
    );
    let touched: Vec<Entity> = app
        .world_mut()
        .query_filtered::<Entity, Changed<MessageSender<HeroCompanyResult>>>()
        .iter(app.world())
        .collect();
    assert_eq!(
        touched,
        vec![connected],
        "only the connected requester receives a reply"
    );

    app.world_mut().clear_trackers();
    app.world_mut().run_schedule(Update);
    assert!(
        app.world_mut()
            .query_filtered::<Entity, Changed<MessageSender<HeroCompanyResult>>>()
            .iter(app.world())
            .next()
            .is_none(),
        "an empty outcome queue must not touch any sender"
    );
}

#[test]
fn dividend_report_text_names_paid_rate_own_take_the_split_and_the_reserve() {
    let outcome = paid_outcome(Entity::PLACEHOLDER);
    assert_eq!(
        dividend_report_text(&outcome),
        "Paid 300.00 coin to 2 shareholders: 3.00 coin per 10 shares; your 600 shares received 180.00 coin. 200.00 coin of it from retained profit, 100.00 coin a return of capital. Held back 12.50 coin: wage debt / tax debt / one day of payroll / 2.00 coin float."
    );
    let all_profit = DividendOutcome {
        from_profit: 30_000,
        return_of_capital: 0,
        ..outcome
    };
    assert!(
        dividend_report_text(&all_profit)
            .contains("300.00 coin of it from retained profit, 0.00 coin a return of capital."),
        "{}",
        dividend_report_text(&all_profit)
    );
    let sole = DividendOutcome {
        shareholders: 1,
        own_shares: 1_000,
        own_take: 30_000,
        ..outcome
    };
    assert!(dividend_report_text(&sole).starts_with("Paid 300.00 coin to 1 shareholder:"));
}

#[test]
fn every_refusal_reads_as_a_concrete_failure_with_the_held_back_figures() {
    for refusal in [
        DividendRefusal::NoOperatingSite,
        DividendRefusal::ShareholderUnreachable,
        DividendRefusal::NothingDistributable,
        DividendRefusal::NothingRequested,
        DividendRefusal::WalletFull,
        DividendRefusal::TreasuryDebitFailed,
    ] {
        let outcome = DividendOutcome {
            paid: 0,
            per_ten_shares: 0,
            own_take: 0,
            from_profit: 0,
            return_of_capital: 0,
            refusal: Some(refusal),
            ..paid_outcome(Entity::PLACEHOLDER)
        };
        let text = dividend_report_text(&outcome);
        assert!(text.starts_with("No dividend: "), "{refusal:?}: {text}");
        assert!(
            text.ends_with(
                "Held back 12.50 coin: wage debt / tax debt / one day of payroll / 2.00 coin float."
            ),
            "{refusal:?}: {text}"
        );
        assert!(!text.contains("return of capital"), "{refusal:?}: {text}");
    }
    assert!(dividend_report_text(&DividendOutcome {
        refusal: Some(DividendRefusal::NoOperatingSite),
        ..paid_outcome(Entity::PLACEHOLDER)
    })
    .contains("no site"),);
    // A company that vanished has no treasury to decompose.
    let vanished = dividend_report_text(&DividendOutcome {
        paid: 0,
        per_ten_shares: 0,
        own_take: 0,
        from_profit: 0,
        return_of_capital: 0,
        withheld_reserves: 0,
        refusal: Some(DividendRefusal::CompanyUnavailable),
        ..paid_outcome(Entity::PLACEHOLDER)
    });
    assert!(vanished.starts_with("No dividend: "), "{vanished}");
    assert!(vanished.contains("no longer exists"), "{vanished}");
    assert!(!vanished.contains("Held back"), "{vanished}");
}
