//! Radio, weather, and terrain-law abuse (port of
//! `tools/playtest_break_scenarios/radio_weather.py`).
//!
//! Chains left on bare pavement, a full jake on glare ice, rolling a Level 2
//! chain law bare, and spinning the radio dial across a 1,200-mile teleport.

use crate::playtest::breaker::{outcome, outcome_of, Outcome, Rig, RigOptions, DT};
use crate::states::base::Key;
use crate::states::driving_core::{RadioSettingsView, CHAIN_LAW_CHECKPOINT_CHANCE};

use ff_core::models::enforcement::{career_citations, citation_fine, CHAIN_LAW_FINE};
use ff_core::pyrandom::PyRandom;
use ff_core::radio::{
    default_radio_catalog, RadioPlaybackBackend, RadioPlaybackError, RadioState, RadioStation,
};
use ff_core::settings::Settings;
use ff_core::sim::vehicle::TIRE_WINTER;
use ff_core::sim::weather::WeatherKind;

use super::text::grouped_number_after;

/// Leave the chains on and run 70 on bare pavement: wear math, snap event,
/// spoken truth.
pub fn chains_on_dry_interstate_at_70() -> Outcome {
    let mut rig = Rig::new(RigOptions::default());
    let mut findings: Vec<String> = Vec::new();
    rig.drive.trip.position_mi = 12.0;
    rig.drive.trip.curves.clear();
    rig.drive.truck_mut().chains_on = true;
    rig.prepare(70.0, None);
    let start_mi = rig.drive.trip.position_mi;
    rig.hold(Key::Up);
    rig.step(9000, DT, Some(&|rig: &Rig| !rig.drive.truck().chains_on));
    let snap_mi = rig.drive.trip.position_mi - start_mi;
    if rig.drive.truck().chains_on {
        findings.push("chains survived miles of bare pavement at 70".to_string());
    } else {
        if rig.said("chain broke") == 0 {
            findings.push("chain snap happened silently".to_string());
        }
        if rig.said("Too fast for chains") == 0 {
            findings.push("no overspeed-chain warning before the snap".to_string());
        }
        // 0.2 %/mi x40 bare x6 overspeed = 48 %/mi -> ~2.1 miles to scrap.
        if !(0.8..=4.5).contains(&snap_mi) {
            findings.push(format!(
                "chains lasted {snap_mi:.1} mi; wear model predicts ~2.1"
            ));
        }
    }
    let note = format!("snapped after {snap_mi:.1} mi with both warnings spoken");
    outcome("chains_on_dry_interstate_at_70", &rig, findings, &note)
}

/// Stage-3 jake in a low gear on glare ice: the drive axle must slide, and say
/// so.
///
/// Python also asserted `jake_brake_force() <= _jake_traction_cap()`. Both of
/// those are private here and the delivered force is `min(demand, cap)` by
/// construction, so that check could only ever be a tautology. What it was
/// guarding -- the cap not being applied at all -- is checked instead by the
/// two things the cap causes and a player can hear: the axle breaking loose,
/// and full-brake deceleration on ice being far worse than on dry pavement.
pub fn glare_ice_full_jake() -> Outcome {
    let mut rig = Rig::new(RigOptions {
        automatic: false,
        ..RigOptions::default()
    });
    let mut findings: Vec<String> = Vec::new();
    rig.drive.trip.position_mi = 12.0;
    rig.drive.trip.curves.clear();
    rig.drive.weather_mut().forced = Some(WeatherKind::Ice);
    rig.drive.weather_mut().current = WeatherKind::Ice;
    rig.prepare(30.0, Some(4));
    rig.drive.truck_mut().set_engine_brake(true);
    rig.drive.truck_mut().throttle = 0.0;
    rig.run_frames(90);
    if !rig.drive.truck().jake_slipping() {
        findings.push(format!(
            "full jake in gear 4 at 30 mph on ice (grip {:.2}) never broke the drive axle loose",
            rig.drive.truck().effective_grip()
        ));
    }
    if rig.said("drive wheels are sliding") == 0 {
        findings.push("jake slip has no spoken warning on ice".to_string());
    }
    let ice_decel = rig.drive.truck().full_service_decel_mps2();
    rig.drive.weather_mut().forced = Some(WeatherKind::Clear);
    rig.drive.weather_mut().current = WeatherKind::Clear;
    rig.run_frames(2);
    let dry_decel = rig.drive.truck().full_service_decel_mps2();
    if dry_decel <= ice_decel * 2.0 {
        findings.push(format!(
            "full-brake decel on ice ({ice_decel:.2}) is not meaningfully worse than dry \
             ({dry_decel:.2})"
        ));
    }
    let note =
        format!("axle slid, warning spoken, ice stop {ice_decel:.2} vs dry {dry_decel:.2} m/s2");
    outcome("glare_ice_full_jake", &rig, findings, &note)
}

/// Roll through a Level 2 chain law bare; citation, spoken balance, and tire
/// claims checked.
pub fn chain_law_citation_balance() -> Outcome {
    // Pick a trip seed whose deterministic checkpoint roll is a hit.
    let seed = (1000..1200)
        .find(|s| {
            let mut rng = PyRandom::new_from_str(&format!("{s}:chain-law:0:2"));
            rng.random() < CHAIN_LAW_CHECKPOINT_CHANCE
        })
        .expect("some seed in [1000, 1200) rolls a checkpoint hit");
    let mut rig = Rig::new(RigOptions {
        seed,
        ..RigOptions::default()
    });
    // What a citation costs, not the new-driver warning.
    rig.app.seasoned_driver();
    let mut findings: Vec<String> = Vec::new();
    rig.drive.trip.chain_law_areas = vec![(10.0, 14.0)];
    rig.drive.weather_mut().forced = Some(WeatherKind::Ice); // surface ice -> Level 2
    rig.drive.weather_mut().current = WeatherKind::Ice;
    rig.prepare(30.0, None);
    rig.run_frames(3); // let the trip push weather surface onto the truck
    rig.drive.trip.position_mi = 12.5; // past the area midpoint: checkpoint territory
    let money_before = rig.app.ctx.profile.as_ref().map_or(0.0, |p| p.money());
    // What this driver owes for it: the base, scaled by priors and by whether
    // the checkpoint caught them inside roadwork.
    let in_construction = rig.drive.trip.in_construction_zone();
    let priors = career_citations(rig.app.ctx.profile.as_ref().expect("a profile"));
    let expected = citation_fine(CHAIN_LAW_FINE, priors, in_construction, None);
    rig.drive.update_chain_law(&mut rig.app.ctx);
    rig.app.ctx.run_deferred();
    if rig.said("without chains") == 0 {
        findings.push("no spoken warning for entering a Level 2 chain law bare".to_string());
    }
    rig.drive.update_chain_law(&mut rig.app.ctx);
    rig.app.ctx.run_deferred();
    let cited = rig.lines_with("chain-law citation");
    if cited.is_empty() {
        findings.push("seeded checkpoint roll was a hit but no citation was written".to_string());
    } else {
        let money = rig.app.ctx.profile.as_ref().map_or(0.0, |p| p.money());
        let delta = money_before - money;
        if (delta - expected).abs() > 0.01 {
            findings.push(format!(
                "citation took {delta:.0}, the fine is {expected:.0}"
            ));
        }
        if let Some(spoken) = grouped_number_after(&cited[0], "You have ") {
            if (spoken - money.round()).abs() > 0.5 {
                findings.push(format!(
                    "citation spoke a balance of {spoken:.0} but the ledger holds {money:.0}"
                ));
            }
        }
    }
    // Winter tires satisfy Level 1 but never Level 2 -- and the warning must
    // name chains, not tires, when only chains will do.
    rig.drive.weather_mut().forced = Some(WeatherKind::Snow);
    rig.drive.weather_mut().current = WeatherKind::Snow;
    rig.run_frames(3);
    rig.drive.truck_mut().tire_type = TIRE_WINTER.to_string();
    rig.drive.trip.position_mi = 10.5;
    let before = rig.transcript().len();
    rig.drive.update_chain_law(&mut rig.app.ctx);
    rig.app.ctx.run_deferred();
    if rig.transcript().len() != before {
        findings.push("winter tires drew a chain-law warning at Level 1 (compliant)".to_string());
    }
    rig.drive.weather_mut().forced = Some(WeatherKind::Ice);
    rig.drive.weather_mut().current = WeatherKind::Ice;
    rig.run_frames(3);
    rig.drive.update_chain_law(&mut rig.app.ctx);
    rig.app.ctx.run_deferred();
    let transcript = rig.transcript();
    let level2: Vec<&String> = transcript[before.min(transcript.len())..]
        .iter()
        .filter(|line| line.contains("chain law without"))
        .collect();
    if let Some(last) = level2.last() {
        let requirement = last
            .split_once("without")
            .map(|(_, rest)| rest)
            .unwrap_or("");
        if !requirement.contains("chains") {
            findings.push("Level 2 warning did not name chains as the requirement".to_string());
        }
    }
    let note = format!("citation billed {expected:.0}, spoken balance matched, tier claims held");
    outcome("chain_law_citation_balance", &rig, findings, &note)
}

/// A radio backend that records what it was asked to play.
#[derive(Default)]
struct StubBackend {
    played: Vec<String>,
    volumes: Vec<f64>,
    stopped: usize,
}

impl RadioPlaybackBackend for StubBackend {
    fn play_station(
        &mut self,
        station: &RadioStation,
        volume: f64,
    ) -> Result<(), RadioPlaybackError> {
        self.played.push(station.id.clone());
        self.volumes.push(volume);
        Ok(())
    }

    fn stop_radio(&mut self) {
        self.stopped += 1;
    }
}

/// Spin the dial, favorite it, tune through a dead handover, then teleport
/// 1,200 miles.
///
/// No app and no drive: this is the radio model on its own, offline, the way
/// Python built it.
pub fn radio_dial_abuse_offline() -> Outcome {
    let mut findings: Vec<String> = Vec::new();
    let settings = Settings {
        radio_enabled: true,
        ..Settings::default()
    };
    let mut radio = RadioState::from_settings(
        default_radio_catalog().to_vec(),
        &RadioSettingsView(&settings),
        &[],
    );
    let mut backend = StubBackend::default();
    radio.update_position(Some((42.886, -78.878)), Some(600.0)); // Buffalo
    let mut messages: Vec<String> = Vec::new();
    messages.push(radio.toggle(Some(&mut backend)).message);
    for _ in 0..18 {
        messages.push(radio.tune(1, Some(&mut backend)).message);
        let reception = radio.current_reception();
        if !(0.0..=1.0).contains(&reception.signal) {
            findings.push(format!(
                "reception signal out of bounds: {}",
                reception.signal
            ));
        }
    }
    messages.push(radio.toggle_favorite());

    // Volume 0 must read as "silent," never claim to be "off" -- muting and
    // zero-volume are different states and a screen reader user relies on the
    // status line to tell them apart. (Python guarded this on `set_volume`
    // existing; here the level is a field, so the check always runs.)
    radio.volume = 0.0;
    let zero_vol_text = radio.status_text();
    let lower = zero_vol_text.to_lowercase();
    if lower.contains("off") && !lower.contains("volume") {
        findings.push(format!(
            "volume 0 status reads {zero_vol_text:?} -- indistinguishable from radio off"
        ));
    }
    radio.volume = 0.5;

    radio.update_position(Some((25.77, -80.19)), Some(10.0)); // Miami, 1,200 miles away
    let reception = radio.current_reception();
    if !(0.0..=1.0).contains(&reception.signal) {
        findings.push(format!(
            "post-teleport signal out of bounds: {}",
            reception.signal
        ));
    }
    messages.push(radio.status_text());
    if messages.iter().any(|message| message.trim().is_empty()) {
        findings.push("a radio action produced an empty spoken message".to_string());
    }
    if let Some(leak) = messages
        .iter()
        .find(|message| message.to_lowercase().contains("http") || message.contains("://"))
    {
        findings.push(format!(
            "radio speech leaked a stream URL: {}",
            leak.chars().take(80).collect::<String>()
        ));
    }
    let count = messages.len();
    let mut outcome = outcome_of(
        "radio_dial_abuse_offline",
        findings,
        &format!("{count} spoken radio actions, signals bounded, no URLs leaked"),
    );
    outcome.transcript = messages;
    outcome
}
