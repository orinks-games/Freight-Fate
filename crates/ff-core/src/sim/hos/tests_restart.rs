//! The restart the driver can choose: the offer, the rest that takes it,
//! and its spoken lines byte for byte.

use super::*;

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// A clock with `used_h` on-duty hours on its cycle, all ending now, and the
/// shift fresh, under `rules`.
fn used(rules: HosRules, used_h: f64) -> HosClock {
    let mut c = HosClock::new();
    c.rules = rules;
    c.cycle_entries = vec![CycleEntry {
        end_min: c.cycle_clock_min,
        on_duty_min: used_h * 60.0,
    }];
    c
}

#[test]
fn restart_offer_table() {
    use RestartOffer as O;
    let us = HosRules::Us;
    let ca = HosRules::CanadaSouth60;
    let mut ten_rested_us = used(us, 70.0);
    ten_rested_us.sleep();
    let mut ten_rested_ca = used(ca, 70.0);
    ten_rested_ca.sleep();
    let mut complete = used(us, 70.0);
    complete.finish_restart("sleeper_berth");
    let rows: Vec<(&str, HosClock, &str, RestartOffer)> = vec![
        ("not enforced", used(us, 70.0), "debug_off", O::None),
        ("30 hours left", used(us, 40.0), "realistic", O::None),
        (
            "24 left",
            used(us, 46.0),
            "realistic",
            O::Fresh { hours: 34 },
        ),
        ("blown", used(us, 70.0), "relaxed", O::Fresh { hours: 34 }),
        (
            "blown, 10 rested",
            ten_rested_us,
            "realistic",
            O::Finish {
                left_min: 24.0 * 60.0,
                hours: 34,
            },
        ),
        ("complete", complete, "realistic", O::None),
        ("canada not enforced", used(ca, 70.0), "debug_off", O::None),
        ("canada 30 left", used(ca, 40.0), "realistic", O::None),
        (
            "canada 24 left",
            used(ca, 46.0),
            "realistic",
            O::Fresh { hours: 36 },
        ),
        (
            "canada blown",
            used(ca, 70.0),
            "realistic",
            O::Fresh { hours: 36 },
        ),
        (
            "canada blown, 10 rested",
            ten_rested_ca,
            "realistic",
            O::Finish {
                left_min: 26.0 * 60.0,
                hours: 36,
            },
        ),
    ];
    for (name, clock, mode, want) in rows {
        assert_eq!(clock.restart_offer(mode), want, "{name}");
    }
}

#[test]
fn finish_restart_from_zero_ten_and_thirty_hours_rested() {
    for rested_h in [0.0, 10.0, 30.0] {
        let mut c = used(HosRules::Us, 70.0);
        c.drive(120.0);
        if rested_h > 0.0 {
            c.off_duty(rested_h * 60.0);
        }
        let events_before = c.history.len();
        let minutes = c.finish_restart("sleeper_berth");
        assert!(
            approx(minutes, (34.0 - rested_h) * 60.0),
            "{rested_h}: {minutes}"
        );
        assert!(c.cycle_entries.is_empty(), "{rested_h}");
        assert!(approx(c.cycle_used_min(), 0.0));
        assert_eq!(c.driving_min, 0.0);
        assert_eq!(c.duty_min, 0.0);
        assert_eq!(c.shift_on_duty_min, 0.0);
        assert_eq!(c.since_break_min, 0.0);
        assert_eq!(c.history.len(), events_before + 1, "{rested_h}");
        let event = c.history.last().expect("the restart's event");
        assert!(approx(event.minutes, minutes));
        assert_eq!(event.status, "sleeper_berth");
        assert_eq!(event.source, "full_reset");
        assert!(c.restart_complete());
        assert_eq!(c.restart_offer("realistic"), RestartOffer::None);
        // Taken again, it rests nothing.
        assert_eq!(c.finish_restart("sleeper_berth"), 0.0);
    }
}

#[test]
fn finish_restart_ends_the_canadian_counts() {
    let mut c = used(HosRules::CanadaSouth60, 70.0);
    c.drive(300.0);
    assert!(c.ca_driving_min > 0.0);
    let minutes = c.finish_restart("sleeper_berth");
    assert!(approx(minutes, 36.0 * 60.0));
    assert_eq!(c.ca_driving_min, 0.0);
    assert_eq!(c.ca_on_duty_min, 0.0);
    assert_eq!(c.ca_elapsed_min, 0.0);
    assert!(c.restart_complete());
}

#[test]
fn restart_records_its_true_length() {
    let mut c = used(HosRules::Us, 70.0);
    c.restart();
    let event = c.history.last().expect("an event");
    assert!(approx(event.minutes, RESTART_MIN));
    assert_eq!(event.source, "full_reset");
}

#[test]
fn restart_labels_and_help_lines() {
    use RestartOffer as O;
    use RestartPlace as P;
    let fresh = O::Fresh { hours: 34 };
    let finish = O::Finish {
        left_min: 24.0 * 60.0,
        hours: 34,
    };
    let ca_fresh = O::Fresh { hours: 36 };
    let ca_finish = O::Finish {
        left_min: 26.0 * 60.0,
        hours: 36,
    };
    assert_eq!(
        fresh.label(P::Terminal).as_deref(),
        Some("Take a 34-hour restart")
    );
    assert_eq!(
        fresh.label(P::Sleeper).as_deref(),
        Some("Take a 34-hour restart")
    );
    assert_eq!(
        fresh.label(P::Lot).as_deref(),
        Some("Take a 34-hour restart in the lot")
    );
    assert_eq!(
        finish.label(P::Terminal).as_deref(),
        Some("Rest 24 hours more to finish a 34-hour restart")
    );
    assert_eq!(
        finish.label(P::Lot).as_deref(),
        Some("Rest 24 hours more to finish a 34-hour restart in the lot")
    );
    assert_eq!(
        ca_fresh.label(P::Terminal).as_deref(),
        Some("Take 36 hours off")
    );
    assert_eq!(
        ca_fresh.label(P::Lot).as_deref(),
        Some("Take 36 hours off in the lot")
    );
    assert_eq!(
        ca_finish.label(P::Sleeper).as_deref(),
        Some("Rest 26 hours more to finish 36 hours off")
    );
    assert_eq!(O::None.label(P::Terminal), None);
    assert_eq!(O::None.help(P::Terminal), None);

    assert_eq!(
        fresh.help(P::Terminal).as_deref(),
        Some(
            "A fresh cycle and fresh hours of service. Sleep takes off 8 points of fatigue an \
             hour. Clock advances 34 hours."
        )
    );
    assert_eq!(
        fresh.help(P::Sleeper).as_deref(),
        Some(
            "A fresh cycle and fresh hours of service. Sleep takes off 8 points of fatigue an \
             hour. Clock and deadline advance 34 hours."
        )
    );
    assert_eq!(
        fresh.help(P::Lot).as_deref(),
        Some(
            "A fresh cycle with poor rest: 5 points of fatigue off an hour, never below 30. \
             Clock and deadline advance 34 hours."
        )
    );
    assert_eq!(
        finish.help(P::Terminal).as_deref(),
        Some(
            "Your rest so far counts. A fresh cycle and fresh hours of service. Sleep takes off \
             8 points of fatigue an hour. Clock advances 24 hours."
        )
    );
}

#[test]
fn restart_result_lines() {
    let fresh = RestartOffer::Fresh { hours: 34 };
    let finish = RestartOffer::Finish {
        left_min: 24.0 * 60.0,
        hours: 34,
    };
    assert_eq!(
        fresh.rested_line("4 PM", "afternoon"),
        "Rested 34 hours. It is 4 PM, afternoon. Cycle clock reset."
    );
    assert_eq!(
        finish.rested_line("4 PM", "afternoon"),
        "Rested 24 hours more. It is 4 PM, afternoon. Cycle clock reset."
    );
    assert_eq!(
        fresh.woke_line(true),
        "You rested 34 hours and woke rested."
    );
    assert_eq!(
        fresh.woke_line(false),
        "You rested 34 hours and woke still tired."
    );
    assert_eq!(
        finish.woke_line(true),
        "You rested 24 hours more, 34 hours off in a row, and woke rested."
    );
    assert_eq!(
        RestartOffer::Finish {
            left_min: 26.0 * 60.0,
            hours: 36
        }
        .woke_line(false),
        "You rested 26 hours more, 36 hours off in a row, and woke still tired."
    );
    assert_eq!(
        fresh.effect_text(),
        "34 hours off in a row restart your cycle and reset your driving hours and legal \
         driving window."
    );
}

#[test]
fn a_sleep_that_leaves_the_cycle_used_up_says_so() {
    let mut c = used(HosRules::Us, 70.0);
    c.sleep();
    assert_eq!(
        c.cycle_still_used_up_text("realistic").as_deref(),
        Some("Your cycle is still used up. Rest 24 hours more to restart it.")
    );
    assert_eq!(c.cycle_still_used_up_text("debug_off"), None);
    // Not blown: nothing said.
    let mut short = used(HosRules::Us, 69.0);
    short.sleep();
    assert_eq!(short.cycle_still_used_up_text("realistic"), None);
    // Restart complete: nothing said.
    c.finish_restart("sleeper_berth");
    assert_eq!(c.cycle_still_used_up_text("realistic"), None);
}

#[test]
fn motel_restart_rows_name_the_room_and_its_nights() {
    use RestartOffer as O;
    use RestartPlace as P;
    let fresh = O::Fresh { hours: 34 };
    let finish = O::Finish {
        left_min: 20.0 * 60.0,
        hours: 34,
    };
    let ca_fresh = O::Fresh { hours: 36 };
    let ca_finish = O::Finish {
        left_min: 26.0 * 60.0,
        hours: 36,
    };
    assert_eq!(fresh.motel_nights(), 2);
    assert_eq!(finish.motel_nights(), 1);
    assert_eq!(ca_finish.motel_nights(), 2);
    assert_eq!(
        fresh.label(P::Motel).as_deref(),
        Some("Take a 34-hour restart in a motel room for 190 dollars")
    );
    assert_eq!(
        finish.label(P::Motel).as_deref(),
        Some("Rest 20 hours more to finish a 34-hour restart in a motel room for 95 dollars")
    );
    assert_eq!(
        ca_fresh.label(P::Motel).as_deref(),
        Some("Take 36 hours off in a motel room for 190 dollars")
    );
    assert_eq!(
        ca_finish.label(P::Motel).as_deref(),
        Some("Rest 26 hours more to finish 36 hours off in a motel room for 190 dollars")
    );
    assert_eq!(
        fresh.help(P::Motel).as_deref(),
        Some(
            "A real bed, paid from your own pocket. A fresh cycle and fresh hours of service. \
             Sleep takes off 8 points of fatigue an hour. Clock and deadline advance 34 hours."
        )
    );
    assert_eq!(O::None.label(P::Motel), None);
    assert_eq!(P::Motel.duty_status(), "off_duty");
    assert_eq!(P::Sleeper.duty_status(), "sleeper_berth");
}
