//! Roadside inspections: what an inspector writes up on this truck and this
//! driver, what it costs, when it earns the decal, and how likely a stop is.
//! The headless half; the game decides where the truck is parked while it
//! happens.
//!
//! Sources: the CVSA North American Standard Inspection levels (Level I is
//! the full driver-and-vehicle inspection including under the truck, Level
//! II the walk-around, Level III driver and paperwork only); CVSA
//! Operational Policy 5 (a Level I with no critical vehicle violations earns
//! a decal good for up to three months, and a decaled vehicle is generally
//! not re-inspected); the CVSA out-of-service criteria (a steer tire under
//! 4/32 inch or any other tire under 2/32, and 20 percent or more of the
//! brakes defective, park the vehicle); FMCSA A&I roadside inspection
//! statistics (about 3.3 million inspections a year, 21.6 percent of
//! vehicles and 6.7 percent of drivers out of service in 2024); 49 CFR
//! 396.13 (the driver's own pre-trip). The wear percentages that stand in
//! for tread depth and brake stroke, the fines, and the miles between stops
//! are ASSUMED and say so at each constant.

use crate::sim::season::day_of_year;

/// Which inspection is being run. The number is what the officer says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectionLevel {
    /// Level I: driver, paperwork, walk-around and under the vehicle.
    Full,
    /// Level II: driver, paperwork and everything visible without getting
    /// under the truck.
    WalkAround,
    /// Level III: driver and paperwork only.
    DriverOnly,
}

impl InspectionLevel {
    /// How long the truck sits, on duty, while it runs. ASSUMED from the
    /// durations carriers quote drivers: 45 to 60 minutes for a Level I,
    /// about 30 for a Level II, 15 for a Level III.
    pub fn minutes(self) -> f64 {
        match self {
            InspectionLevel::Full => 45.0,
            InspectionLevel::WalkAround => 30.0,
            InspectionLevel::DriverOnly => 15.0,
        }
    }

    /// The spoken name of the inspection.
    pub fn spoken(self) -> &'static str {
        match self {
            InspectionLevel::Full => "Level 1 full inspection",
            InspectionLevel::WalkAround => "Level 2 walk-around inspection",
            InspectionLevel::DriverOnly => "Level 3 driver inspection",
        }
    }

    /// Whether the inspector looks at the equipment at all.
    pub fn checks_vehicle(self) -> bool {
        !matches!(self, InspectionLevel::DriverOnly)
    }

    /// Whether the inspector gets under the truck: brake adjustment is
    /// measured there, and nowhere else.
    pub fn checks_under_vehicle(self) -> bool {
        matches!(self, InspectionLevel::Full)
    }
}

/// What has to happen before an out-of-service truck rolls again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repair {
    /// Nothing mechanical: the order is served in time (hours of service).
    None,
    Tires,
    Brakes,
    /// Body or frame damage past the safe limit: the roadside mechanic's patch.
    Damage,
    /// The hooked trailer, which the driver never owned and nobody has been
    /// under since it was parked.
    Trailer,
}

/// One line on the inspection report.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    /// Spoken, as the officer reads it.
    pub what: String,
    pub fine: f64,
    /// A critical item: the truck does not move until the repair is made.
    pub out_of_service: bool,
    pub repair: Repair,
}

/// Everything the inspector can read off this truck and this driver.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InspectionInput<'a> {
    pub level: InspectionLevel,
    pub tire_wear_pct: f64,
    pub brake_wear_pct: f64,
    pub damage_pct: f64,
    /// What the hooked trailer's own walk-around would show, if anything
    /// (`TrailerUnit::defect`).
    pub trailer_defect: Option<&'a str>,
    /// Driving right now past an hours-of-service limit.
    pub over_hours: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InspectionReport {
    pub level: InspectionLevel,
    pub findings: Vec<Finding>,
}

// Wear percentages standing in for the physical limits. ASSUMED: the sim
// keeps one wear number per component, so the citation line is where a
// tire is "near the limit" and the out-of-service line is where it is past
// it. The tire fine is the ordinary equipment violation; the out-of-service
// fine is the higher schedule most states attach to a critical item.
pub const TIRE_CITATION_PCT: f64 = 75.0;
pub const TIRE_OUT_OF_SERVICE_PCT: f64 = 90.0;
pub const BRAKE_CITATION_PCT: f64 = 75.0;
pub const BRAKE_OUT_OF_SERVICE_PCT: f64 = 90.0;
/// The tire line on a report short of the out-of-service line, as the
/// officer reads it. The enforcement watch matches it to know the tires are
/// already written up.
pub const TIRES_WORN_FINDING: &str = "tires worn close to the tread limit";
/// The body-damage line short of the out-of-service line, likewise.
pub const DAMAGE_FINDING: &str = "visible body damage written up as unsafe equipment";
/// Visible body damage the officer writes up as unsafe equipment, and the
/// point past which the truck is parked, the same line the roadside
/// damage stop already uses.
pub const DAMAGE_CITATION_PCT: f64 = 40.0;
pub const DAMAGE_OUT_OF_SERVICE_PCT: f64 = 65.0;
/// ASSUMED: typical state schedules run 100 to 250 dollars for an equipment
/// violation and several hundred for an out-of-service item.
pub const EQUIPMENT_FINE: f64 = 150.0;
pub const OUT_OF_SERVICE_FINE: f64 = 300.0;

/// A clean Level I earns the decal for up to three months (CVSA Operational
/// Policy 5: the month of the inspection and the two after it). Ninety days
/// of career time.
pub const DECAL_VALID_HOURS: f64 = 90.0 * 24.0;

/// How far a clean driver goes between routine roadside inspections.
/// ASSUMED: the real rate is about one inspection per driver per year, close
/// to one per 100,000 miles, which a career here never reaches; this is
/// that rate compressed so a clean driver meets a Level III a few times in a
/// career and a targeted one meets them constantly.
pub const ROADSIDE_INSPECTION_MILES: f64 = 6_000.0;
pub const WATCHED_INSPECTION_MULT: f64 = 2.0;
pub const TARGETED_INSPECTION_MULT: f64 = 4.0;
/// CVSA's International Roadcheck: 72 hours in mid-May when every inspector
/// in North America is on the road (May 13 to 15 in 2025).
pub const ROADCHECK_MULT: f64 = 3.0;
pub const ROADCHECK_FIRST_DAY: f64 = 133.0;
pub const ROADCHECK_LAST_DAY: f64 = 135.0;
/// Relaxed hours-of-service halves the odds, the way it halves the
/// scale-house inspection lane.
pub const RELAXED_INSPECTION_MULT: f64 = 0.5;

fn finding(what: &str, fine: f64, out_of_service: bool, repair: Repair) -> Finding {
    Finding {
        what: what.to_string(),
        fine,
        out_of_service,
        repair,
    }
}

/// Run the inspection: every item the level looks at, in the order the
/// officer reads them out.
pub fn inspect(input: &InspectionInput<'_>) -> InspectionReport {
    let mut findings = Vec::new();
    // Driver items come first at every level.
    if input.over_hours {
        findings.push(finding(
            "driving past the hours-of-service limit",
            0.0,
            true,
            Repair::None,
        ));
    }
    if input.level.checks_vehicle() {
        if input.tire_wear_pct >= TIRE_OUT_OF_SERVICE_PCT {
            findings.push(finding(
                "a tire below the minimum tread depth",
                OUT_OF_SERVICE_FINE,
                true,
                Repair::Tires,
            ));
        } else if input.tire_wear_pct >= TIRE_CITATION_PCT {
            findings.push(finding(
                TIRES_WORN_FINDING,
                EQUIPMENT_FINE,
                false,
                Repair::None,
            ));
        }
        if input.damage_pct >= DAMAGE_OUT_OF_SERVICE_PCT {
            findings.push(finding(
                "body damage past the safe limit",
                OUT_OF_SERVICE_FINE,
                true,
                Repair::Damage,
            ));
        } else if input.damage_pct >= DAMAGE_CITATION_PCT {
            findings.push(finding(DAMAGE_FINDING, EQUIPMENT_FINE, false, Repair::None));
        }
        if let Some(defect) = input.trailer_defect {
            // A brake adjustment is measured under the trailer; a lamp and a
            // tire are seen on the walk-around.
            let under = defect.contains("brake");
            if !under || input.level.checks_under_vehicle() {
                let critical = defect.contains("brake") || defect.contains("tire");
                findings.push(finding(
                    defect,
                    if critical {
                        OUT_OF_SERVICE_FINE
                    } else {
                        EQUIPMENT_FINE
                    },
                    critical,
                    if critical {
                        Repair::Trailer
                    } else {
                        Repair::None
                    },
                ));
            }
        }
    }
    if input.level.checks_under_vehicle() {
        if input.brake_wear_pct >= BRAKE_OUT_OF_SERVICE_PCT {
            findings.push(finding(
                "brakes out of adjustment",
                OUT_OF_SERVICE_FINE,
                true,
                Repair::Brakes,
            ));
        } else if input.brake_wear_pct >= BRAKE_CITATION_PCT {
            findings.push(finding(
                "brakes close to the adjustment limit",
                EQUIPMENT_FINE,
                false,
                Repair::None,
            ));
        }
    }
    InspectionReport {
        level: input.level,
        findings,
    }
}

impl InspectionReport {
    pub fn minutes(&self) -> f64 {
        self.level.minutes()
    }

    pub fn clean(&self) -> bool {
        self.findings.is_empty()
    }

    pub fn total_fine(&self) -> f64 {
        self.findings.iter().map(|f| f.fine).sum()
    }

    pub fn out_of_service(&self) -> bool {
        self.findings.iter().any(|f| f.out_of_service)
    }

    /// Only a clean Level I puts the sticker on the windshield.
    pub fn earns_decal(&self) -> bool {
        self.clean() && self.level == InspectionLevel::Full
    }

    /// The repairs the out-of-service items demand, in report order, each
    /// kind once.
    pub fn repairs(&self) -> Vec<Repair> {
        let mut out: Vec<Repair> = Vec::new();
        for f in &self.findings {
            if f.out_of_service && f.repair != Repair::None && !out.contains(&f.repair) {
                out.push(f.repair);
            }
        }
        out
    }

    /// The findings as one spoken clause: "tires worn close to the tread
    /// limit, and brakes out of adjustment".
    pub fn spoken_findings(&self) -> String {
        let items: Vec<&str> = self.findings.iter().map(|f| f.what.as_str()).collect();
        match items.as_slice() {
            [] => String::new(),
            [only] => (*only).to_string(),
            [head @ .., last] => format!("{}, and {}", head.join(", "), last),
        }
    }
}

/// The driver's own pre-trip (49 CFR 396.13): the same equipment items a
/// Level I would write up, as plain lines. Empty means nothing to write up.
pub fn walk_around(
    tire_wear_pct: f64,
    brake_wear_pct: f64,
    damage_pct: f64,
    trailer_defect: Option<&str>,
) -> Vec<String> {
    let report = inspect(&InspectionInput {
        level: InspectionLevel::Full,
        tire_wear_pct,
        brake_wear_pct,
        damage_pct,
        trailer_defect,
        over_hours: false,
    });
    report
        .findings
        .iter()
        .map(|f| {
            // Each line opens a sentence of its own in the readout.
            let mut what = f.what.clone();
            if let Some(first) = what.get(..1) {
                what.replace_range(..1, &first.to_uppercase());
            }
            if f.out_of_service {
                format!("{what}: an inspector would park you for this.")
            } else {
                format!("{what}: an inspector would write this up.")
            }
        })
        .collect()
}

/// Whether the decal on the windshield still counts.
pub fn decal_valid(decal_until_h: f64, now_h: f64) -> bool {
    decal_until_h > 0.0 && now_h < decal_until_h
}

/// Whether the career clock is inside the Roadcheck blitz.
pub fn roadcheck_blitz(game_hours: f64) -> bool {
    let doy = day_of_year(game_hours).floor();
    (ROADCHECK_FIRST_DAY..=ROADCHECK_LAST_DAY).contains(&doy)
}

/// How much more often than a clean driver this one is stopped.
/// `band` is the safety record's band (`safety_record::safety_band`).
pub fn roadside_inspection_scale(band: &str, relaxed: bool, roadcheck: bool) -> f64 {
    let mut scale = match band {
        "targeted" => TARGETED_INSPECTION_MULT,
        "watched" => WATCHED_INSPECTION_MULT,
        _ => 1.0,
    };
    if roadcheck {
        scale *= ROADCHECK_MULT;
    }
    if relaxed {
        scale *= RELAXED_INSPECTION_MULT;
    }
    scale
}

/// Odds a routine inspection happens somewhere in the next `miles`, given
/// the driver's scale: a memoryless draw, so the check interval does not
/// change the rate.
pub fn roadside_inspection_chance(miles: f64, scale: f64) -> f64 {
    if miles <= 0.0 || scale <= 0.0 {
        return 0.0;
    }
    1.0 - (-miles * scale / ROADSIDE_INSPECTION_MILES).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(level: InspectionLevel) -> InspectionInput<'static> {
        InspectionInput {
            level,
            tire_wear_pct: 10.0,
            brake_wear_pct: 10.0,
            damage_pct: 0.0,
            trailer_defect: None,
            over_hours: false,
        }
    }

    #[test]
    fn a_sound_truck_passes_every_level_and_only_a_level_one_earns_the_decal() {
        for level in [
            InspectionLevel::Full,
            InspectionLevel::WalkAround,
            InspectionLevel::DriverOnly,
        ] {
            let report = inspect(&input(level));
            assert!(report.clean(), "{level:?}: {:?}", report.findings);
            assert_eq!(report.earns_decal(), level == InspectionLevel::Full);
            assert_eq!(report.minutes(), level.minutes());
        }
    }

    #[test]
    fn worn_tires_are_a_citation_then_out_of_service() {
        let mut near = input(InspectionLevel::WalkAround);
        near.tire_wear_pct = TIRE_CITATION_PCT;
        let report = inspect(&near);
        assert_eq!(report.findings.len(), 1);
        assert!(!report.out_of_service());
        assert_eq!(report.total_fine(), EQUIPMENT_FINE);

        let mut bald = near;
        bald.tire_wear_pct = TIRE_OUT_OF_SERVICE_PCT;
        let report = inspect(&bald);
        assert!(report.out_of_service());
        assert_eq!(report.repairs(), vec![Repair::Tires]);
        assert_eq!(report.total_fine(), OUT_OF_SERVICE_FINE);
        assert!(!report.earns_decal());
    }

    #[test]
    fn brake_adjustment_is_only_found_under_the_truck() {
        let mut worn = input(InspectionLevel::WalkAround);
        worn.brake_wear_pct = BRAKE_OUT_OF_SERVICE_PCT;
        assert!(
            inspect(&worn).clean(),
            "a walk-around cannot measure stroke"
        );
        worn.level = InspectionLevel::Full;
        let report = inspect(&worn);
        assert_eq!(report.repairs(), vec![Repair::Brakes]);
        assert_eq!(report.spoken_findings(), "brakes out of adjustment");
    }

    #[test]
    fn a_driver_only_inspection_ignores_the_equipment() {
        let mut wreck = input(InspectionLevel::DriverOnly);
        wreck.tire_wear_pct = 100.0;
        wreck.brake_wear_pct = 100.0;
        wreck.damage_pct = 100.0;
        wreck.trailer_defect = Some("trailer marker lamp out");
        assert!(inspect(&wreck).clean());
        wreck.over_hours = true;
        let report = inspect(&wreck);
        assert!(report.out_of_service());
        assert_eq!(report.repairs(), Vec::<Repair>::new());
        assert_eq!(
            report.total_fine(),
            0.0,
            "the hours fine is booked by the hours stop"
        );
    }

    #[test]
    fn the_hooked_trailers_defect_is_read_at_the_right_level() {
        let mut lamp = input(InspectionLevel::WalkAround);
        lamp.trailer_defect = Some("trailer marker lamp out");
        let report = inspect(&lamp);
        assert_eq!(report.findings.len(), 1);
        assert!(!report.out_of_service());

        let mut brake = input(InspectionLevel::WalkAround);
        brake.trailer_defect = Some("trailer brake out of adjustment");
        assert!(inspect(&brake).clean());
        brake.level = InspectionLevel::Full;
        assert_eq!(inspect(&brake).repairs(), vec![Repair::Trailer]);

        let mut tire = input(InspectionLevel::WalkAround);
        tire.trailer_defect = Some("worn trailer tire below tread depth");
        assert_eq!(inspect(&tire).repairs(), vec![Repair::Trailer]);
    }

    #[test]
    fn findings_read_as_one_spoken_clause() {
        let mut bad = input(InspectionLevel::Full);
        bad.tire_wear_pct = TIRE_CITATION_PCT;
        bad.damage_pct = DAMAGE_CITATION_PCT;
        bad.brake_wear_pct = BRAKE_CITATION_PCT;
        let report = inspect(&bad);
        assert_eq!(
            report.spoken_findings(),
            "tires worn close to the tread limit, visible body damage written up as unsafe \
             equipment, and brakes close to the adjustment limit"
        );
        assert_eq!(report.total_fine(), 3.0 * EQUIPMENT_FINE);
    }

    #[test]
    fn the_walk_around_says_what_an_inspector_would_do_about_each_item() {
        assert!(walk_around(10.0, 10.0, 0.0, None).is_empty());
        let lines = walk_around(
            TIRE_OUT_OF_SERVICE_PCT,
            BRAKE_CITATION_PCT,
            0.0,
            Some("trailer marker lamp out"),
        );
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines[0].starts_with("A tire below"), "{}", lines[0]);
        assert!(lines[0].ends_with("an inspector would park you for this."));
        assert!(lines[1].ends_with("an inspector would write this up."));
    }

    #[test]
    fn the_decal_lasts_ninety_days_and_zero_means_none() {
        assert!(!decal_valid(0.0, 100.0));
        assert!(decal_valid(1000.0 + DECAL_VALID_HOURS, 1000.0));
        assert!(!decal_valid(
            1000.0 + DECAL_VALID_HOURS,
            1000.0 + DECAL_VALID_HOURS
        ));
    }

    #[test]
    fn roadcheck_is_three_days_in_mid_may() {
        // Day-of-year 80 is March 21, the career's first day.
        let may_13 = (133.0 - 80.0) * 24.0 + 1.0;
        assert!(roadcheck_blitz(may_13));
        assert!(roadcheck_blitz(may_13 + 2.0 * 24.0));
        assert!(!roadcheck_blitz(may_13 - 24.0));
        assert!(!roadcheck_blitz(may_13 + 3.0 * 24.0));
        assert!(!roadcheck_blitz(0.0));
    }

    #[test]
    fn the_record_band_and_the_blitz_scale_the_odds() {
        assert_eq!(roadside_inspection_scale("clean", false, false), 1.0);
        assert_eq!(roadside_inspection_scale("watched", false, false), 2.0);
        assert_eq!(roadside_inspection_scale("targeted", false, false), 4.0);
        assert_eq!(roadside_inspection_scale("targeted", true, true), 6.0);
        let clean = roadside_inspection_chance(30.0, 1.0);
        assert!(clean > 0.004 && clean < 0.006, "{clean}");
        assert!(roadside_inspection_chance(30.0, 4.0) > 3.9 * clean);
        assert_eq!(roadside_inspection_chance(30.0, 0.0), 0.0);
        // Memoryless: two 15-mile checks match one 30-mile check.
        let half = roadside_inspection_chance(15.0, 1.0);
        let two = 1.0 - (1.0 - half) * (1.0 - half);
        assert!((two - clean).abs() < 1e-12);
    }
}
