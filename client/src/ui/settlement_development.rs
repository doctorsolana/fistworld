//! One vocabulary for the observed development checklist in both settlement views.
//! These are authoritative daily readings; living conditions remain a separate ledger.

use shared::components::{SettlementDevelopment, SettlementProgressGate, SettlementTier};
use shared::economy::*;

pub(super) fn checklist(
    tier: SettlementTier,
    development: Option<&SettlementDevelopment>,
) -> Vec<(String, String)> {
    let target = match tier {
        SettlementTier::Hamlet => "Village",
        SettlementTier::Village => "Town",
        SettlementTier::Town | SettlementTier::City => {
            return vec![(
                "NEXT STEP".into(),
                "City advancement is not yet implemented".into(),
            )];
        }
        SettlementTier::Ruins => {
            return vec![("NEXT STEP".into(), "Rebuild a settlement first".into())];
        }
    };
    let evidence = development.map(|reading| reading.evidence);
    let count = |value: Option<u32>, required: u32| {
        value.map_or_else(
            || format!("— / {required}"),
            |value| format!("{value} / {required}"),
        )
    };
    let village = tier == SettlementTier::Hamlet;
    let mut rows = vec![
        ("NEXT TIER".into(), target.into()),
        (
            "REVIEW".into(),
            if evidence.is_some() {
                "Last daily review"
            } else {
                "Awaiting local daily review"
            }
            .into(),
        ),
        (
            "RESIDENTS".into(),
            count(
                evidence.map(|e| e.residents),
                if village {
                    VILLAGE_MIN_RESIDENTS
                } else {
                    TOWN_MIN_RESIDENTS
                },
            ),
        ),
        (
            "HOUSED".into(),
            count(
                evidence.map(|e| e.housed_residents),
                if village {
                    VILLAGE_MIN_HOUSED_RESIDENTS
                } else {
                    TOWN_MIN_HOUSED_RESIDENTS
                },
            ),
        ),
    ];
    if village {
        rows.push((
            "OCCUPIED HOMES".into(),
            count(
                evidence.map(|e| e.occupied_homes),
                VILLAGE_MIN_OCCUPIED_HOMES,
            ),
        ));
    } else {
        rows.push((
            "MARKET ACCESS".into(),
            evidence
                .map_or("Unobserved", |e| {
                    if e.market_accessible {
                        "Accessible"
                    } else {
                        "Required"
                    }
                })
                .into(),
        ));
        rows.push((
            "BUSINESS TYPES".into(),
            count(
                evidence.map(|e| u32::from(e.operating_business_types)),
                u32::from(TOWN_MIN_OPERATING_BUSINESS_TYPES),
            ),
        ));
        rows.push((
            "RECENT TRADE".into(),
            evidence.map_or_else(
                || "Unobserved".into(),
                |e| {
                    if e.paid_trade_pennies > 0 {
                        format!("{} coin paid", format_money(e.paid_trade_pennies))
                    } else {
                        "Paid trade required".into()
                    }
                },
            ),
        ));
    }
    let project_approved = development.is_some_and(|reading| {
        matches!(
            reading.next_gate,
            SettlementProgressGate::CivicHallMaterials
                | SettlementProgressGate::CivicHallConstruction
        )
    });
    if project_approved {
        // A commissioned Hall keeps its approval; its saved observations are
        // no longer the current calendar window while construction continues.
        rows.push(("QUALIFICATION".into(), "Approved".into()));
    } else {
        let qualified = development.map_or(0, |reading| reading.progress_days);
        rows.push((
            "QUALIFYING DAYS".into(),
            format!(
                "{qualified} of last {DEVELOPMENT_WINDOW_DAYS} · {DEVELOPMENT_REQUIRED_DAYS} needed"
            ),
        ));
    }
    if let Some(reading) = development {
        if matches!(
            reading.next_gate,
            SettlementProgressGate::CivicHallMaterials
                | SettlementProgressGate::CivicHallConstruction
        ) {
            rows.push((
                "HALL MATERIALS".into(),
                format!(
                    "{} / {} {}",
                    reading.material_staged,
                    reading.material_required,
                    if village { "Wood" } else { "Stone" }
                ),
            ));
            rows.push((
                "HALL WORK".into(),
                if reading.next_gate == SettlementProgressGate::CivicHallConstruction {
                    "Hall work underway"
                } else {
                    "Staging paid materials"
                }
                .into(),
            ));
        } else {
            rows.push(("NEXT STEP".into(), reading.next_gate.label().into()));
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checklist_uses_daily_evidence_and_keeps_materials_out_of_day_counts() {
        let mut reading = SettlementDevelopment::from_seed(1, 0);
        reading.evidence.residents = 12;
        reading.evidence.housed_residents = 8;
        reading.evidence.occupied_homes = 2;
        reading.progress_days = 2;
        reading.qualification_bits = 0b101;
        reading.next_gate = SettlementProgressGate::CivicHallConstruction;
        reading.material_staged = 12;
        reading.material_required = 12;
        let rows = checklist(SettlementTier::Hamlet, Some(&reading));
        assert!(rows.contains(&("HOUSED".into(), "8 / 8".into())));
        assert!(rows.contains(&("QUALIFICATION".into(), "Approved".into())));
        assert!(rows.contains(&("HALL MATERIALS".into(), "12 / 12 Wood".into())));
        assert!(rows.contains(&("HALL WORK".into(), "Hall work underway".into())));
        assert!(rows.iter().all(|(_, value)| !value.contains("sustained")));
    }

    #[test]
    fn committed_projects_do_not_present_approval_as_a_current_three_day_window() {
        let mut reading = SettlementDevelopment::from_seed(1, 0);
        reading.progress_days = 2;
        reading.qualification_bits = 0b101;
        reading.last_progress_day = 4;
        for gate in [
            SettlementProgressGate::CivicHallMaterials,
            SettlementProgressGate::CivicHallConstruction,
        ] {
            reading.next_gate = gate;
            let rows = checklist(SettlementTier::Hamlet, Some(&reading));
            assert!(rows.contains(&("QUALIFICATION".into(), "Approved".into())));
            assert!(rows
                .iter()
                .all(|(label, value)| label != "QUALIFYING DAYS" && !value.contains("of last 3")));
        }
        reading.next_gate = SettlementProgressGate::Sustaining;
        let rows = checklist(SettlementTier::Hamlet, Some(&reading));
        assert!(rows.contains(&("QUALIFYING DAYS".into(), "2 of last 3 · 2 needed".into())));
    }

    #[test]
    fn town_requires_current_business_and_paid_trade_not_a_tavern_or_wellbeing_score() {
        let mut reading = SettlementDevelopment::from_seed(1, 0);
        reading.evidence.operating_business_types = 2;
        reading.evidence.paid_trade_pennies = 1;
        let rows = checklist(SettlementTier::Village, Some(&reading));
        assert!(rows.contains(&("BUSINESS TYPES".into(), "2 / 2".into())));
        assert!(rows.contains(&("RECENT TRADE".into(), "0.01 coin paid".into())));
        assert!(rows
            .iter()
            .all(|(label, _)| !["TAVERN", "PROSPERITY", "FOOD"].contains(&label.as_str())));
        assert_eq!(checklist(SettlementTier::Town, Some(&reading)).len(), 1);
        assert!(
            checklist(SettlementTier::Hamlet, None).contains(&("HOUSED".into(), "— / 8".into()))
        );
    }
}
