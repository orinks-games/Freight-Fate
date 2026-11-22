//! A load wrecked on the road (Memphis to Nashville, rolled over in the bend
//! before exit 207): what each business status owes at the dock.

use ff_core::models::business::{COMPANY_DRIVER, INDEPENDENT_AUTHORITY, LEASED_OWNER_OPERATOR};
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::sim::vehicle::DAMAGE_BAND_OUT_OF_SERVICE;
use freight_fate::app::testing::TestApp;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::DRIVE_PHASE_DELIVERY;
use freight_fate::states::driving_menu_states::ArrivalState;

/// Settle the wrecked Memphis to Nashville load for `status`, the freight at
/// `cargo_damage` percent, the truck out of service on the road.
fn wrecked(status: &'static str, cargo_damage: f64) -> (ArrivalState, f64) {
    let mut app = TestApp::new();
    let mut job = Job::new(
        &CARGO_CATALOG["electronics"],
        20.0,
        "Memphis",
        "Memphis pickup",
        "Nashville",
        212.0,
        2500.0,
        12.0,
    );
    job.destination_location = "Nashville receiver".to_string();
    job.destination_type = "retail_distribution".to_string();
    let mut profile = Profile::named_in("Wrecked Load", &job.origin);
    profile.business_status = status.to_string();
    profile.set_money(5000.0);
    app.ctx.profile = Some(profile);
    let route = app
        .ctx
        .world
        .supported_route("Memphis", "Nashville", None)
        .expect("the world routes")
        .expect("Memphis reaches Nashville");
    let mut driving = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(4),
        DRIVE_PHASE_DELIVERY,
        None,
    );
    driving.trip.truck.cargo_damage_pct = cargo_damage;
    driving.trip.truck.damage_pct = 60.0;
    driving.trip.truck.preventable_damage_pct = 60.0;
    driving.worst_damage_band = DAMAGE_BAND_OUT_OF_SERVICE;
    driving.trip.position_mi = driving.trip.total_miles();
    driving.trip.update(0.0);
    let arrival = ArrivalState::new(&mut app.ctx, &mut driving);
    let owed = app.ctx.profile.as_ref().expect("a career").fines_owed;
    (arrival, owed)
}

/// The figure after `prefix` in the first row that has it.
fn dollars_after(rows: &[String], prefix: &str) -> Option<f64> {
    let row = rows.iter().find_map(|r| r.split(prefix).nth(1))?;
    let digits: String = row
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit() || *c == ',')
        .filter(|c| *c != ',')
        .collect();
    digits.parse().ok()
}

/// A company driver never owes more on a refused load than its deductible:
/// the pay they lose is their own, not the load's revenue.
#[test]
fn a_refused_company_load_owes_no_more_than_the_deductible() {
    let (arrival, owed) = wrecked(COMPANY_DRIVER, 100.0);
    let rows = &arrival.summary_lines;
    let all = rows.join(" | ");
    let deductible = dollars_after(rows, "Deductible ").expect("a deductible row");
    assert!(
        owed <= deductible + 0.5,
        "owes {owed} past a {deductible} deductible: {all}"
    );
    if let Some(stays) = dollars_after(rows, "dollars of those charges. ") {
        assert!(stays <= deductible + 0.5, "{stays} stays owed: {all}");
    }
    assert!(all.contains("Pay for the load: none."), "{all}");
}

/// A claimed company load loses the claim share of the driver's wage, so
/// the driver keeps the rest of their pay.
#[test]
fn a_claimed_company_load_takes_its_share_off_the_drivers_wage() {
    let (arrival, _) = wrecked(COMPANY_DRIVER, 40.0);
    let all = arrival.summary_lines.join(" | ");
    let off = dollars_after(&arrival.summary_lines, "dollars, ").expect("a claim row");
    let revenue = dollars_after(&arrival.summary_lines, "Pay for the load: ").expect("pay");
    assert!(
        off < revenue * 0.45 - 1.0,
        "{off} off a {revenue} load: {all}"
    );
}

/// The figures for the PR body: what each status owes on the wreck.
#[test]
fn the_wreck_settles_for_every_business_status() {
    for status in [COMPANY_DRIVER, LEASED_OWNER_OPERATOR, INDEPENDENT_AUTHORITY] {
        for damage in [100.0, 40.0] {
            let (arrival, owed) = wrecked(status, damage);
            let rows: Vec<&String> = arrival
                .summary_lines
                .iter()
                .filter(|r| {
                    r.contains("dollars") || r.contains("Deductible") || r.contains("refused")
                })
                .collect();
            eprintln!("FIGURES {status} cargo {damage}: owed {owed:.0}");
            for row in rows {
                eprintln!("FIGURES   {row}");
            }
        }
    }
}
