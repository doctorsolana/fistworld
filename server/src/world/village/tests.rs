//! Village regressions grouped by the behavior they protect.
//! Shared setup lives in fixtures; all tests still use the production systems.

use super::*;

use shared::components::CompanyOwnership;

use shared::economy::{
    CompanyAccount, CompanyManagementPolicy, VILLAGE_MIN_PROSPERITY, VILLAGE_MIN_RESIDENTS,
    VILLAGE_REQUIRED_SECURE_DAYS,
};

mod businesses_tests;
mod civic_tests;
mod commerce_tests;
mod construction_tests;
mod employment_tests;
mod fixtures;
mod households_tests;
mod immigration_tests;
mod integration_tests;
mod planning_tests;
mod production_tests;

use fixtures::{spawn_test_company, village_test_app};
