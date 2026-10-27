//! The slow zones on a run -- invented and real work zones, congestion
//! placed from HPMS volumes, the facility approach bands and the winter
//! chain-law areas -- and the checks that walk them (the zone section of
//! `trip.py`).

use crate::sim::trip_models::*;
use crate::sim::trip_route_helpers::zone_key;
use crate::speech_text::SpokenMessage;

use super::Trip;

/// Keep only the simulated work zones clear of `spans`, by the open-road rule.
///
/// A construction zone and the merge taper ahead of it are one thing on the
/// road, so both go or neither does. Anything that is not a simulated work
/// zone is passed through untouched.
fn drop_work_zones_near(zones: &mut Vec<Zone>, spans: &[(f64, f64)]) {
    zones.retain(|zone| {
        if zone.reason != "construction" && zone.reason != "construction merge" {
            return true;
        }
        !spans.iter().any(|(start, end)| {
            zone.start_mi < end + ZONE_MIN_GAP_MI && zone.end_mi > start - ZONE_MIN_GAP_MI
        })
    });
}

impl Trip {
    pub fn place_zones(&mut self) -> Vec<Zone> {
        let mut zones: Vec<Zone> = Vec::new();
        let total = self.route.miles();
        let n = 0.max((total / 150.0) as i64);
        // Congestion is worked out FIRST and its footprint claimed before any
        // work zone is drawn, so the draw moves somewhere else instead of
        // being thrown away afterwards. Deleting it cost a route its only
        // roadworks on one run in ten -- and roadworks are the part of a slow
        // zone that is supposed to differ between runs, so deleting them is
        // exactly the wrong thing to spend.
        let congestion = self.place_congestion_zones();
        // Spans already claimed by placed zones, so independent draws cannot
        // nest one zone inside another or butt two together.
        let mut spans: Vec<(f64, f64)> =
            congestion.iter().map(|z| (z.start_mi, z.end_mi)).collect();
        for _ in 0..n {
            let mut placed: Option<(f64, f64)> = None;
            for _attempt in 0..8 {
                let at = self.rng.uniform(15.0, 16.0_f64.max(total - 20.0));
                let end = at + self.rng.uniform(3.0, 9.0);
                if spans
                    .iter()
                    .all(|(s, e)| at > e + ZONE_MIN_GAP_MI || end < s - ZONE_MIN_GAP_MI)
                {
                    placed = Some((at, end));
                    break;
                }
            }
            let Some((at, end)) = placed else {
                continue; // the route is crowded; place fewer zones instead
            };
            if self.rng.random() < 0.6 {
                // A side, not a lane number: crews cone off the outside of
                // the road.
                let mut side: Option<&str> = if self.rng.random() < CONSTRUCTION_CLOSURE_CHANCE {
                    Some(*self.rng.choice(&["right", "left"]))
                } else {
                    None
                };
                let taper_start = (at - CONSTRUCTION_TAPER_MI).max(0.0);
                // Only cone off a lane where the driver has another one to
                // move into for the whole signed stretch.
                if side.is_some() && !self.span_is_multilane(taper_start, end) {
                    side = None;
                }
                // A work zone steps the limit down, never up: through a town
                // posted 35 the fixed 55 and 45 raised it (2026-09-28).
                let road = self.lowest_limit_over(taper_start, end);
                zones.push(
                    Zone::new(
                        taper_start,
                        at,
                        CONSTRUCTION_TAPER_LIMIT_MPH.min(road),
                        "construction merge",
                    )
                    .with_closed_side(side),
                );
                zones.push(
                    Zone::new(at, end, 45.0_f64.min(road), "construction").with_closed_side(side),
                );
                spans.push((taper_start, end));
            }
        }
        // Real construction zones from state 511 APIs replace simulated
        // zones on overlapping stretches.
        let real_construction = self.place_real_construction_zones();
        if !real_construction.is_empty() {
            let real_spans: Vec<(f64, f64)> = real_construction
                .iter()
                .filter(|z| z.reason == "construction")
                .map(|z| (z.start_mi, z.end_mi))
                .collect();
            drop_work_zones_near(&mut zones, &real_spans);
            zones.extend(real_construction);
        }
        // Already claimed above, so nothing drawn here can be sitting in one.
        zones.extend(congestion);
        zones.extend(self.facility_speed_zones());
        zones.sort_by(|a, b| {
            a.start_mi
                .partial_cmp(&b.start_mi)
                .expect("finite mileposts")
        });
        zones
    }

    /// (two-way AADT, per-direction lanes) at a route mile: the baked HPMS
    /// profile where the leg has one, else the class/metro heuristic.
    pub fn route_aadt_at(&self, mile: f64) -> (f64, i64) {
        let (leg_i, leg_start) = self.leg_at_mile(mile);
        let leg = &self.route.legs[leg_i];
        let forward = self.route.cities[leg_i] == leg.a;
        let offset = mile - leg_start;
        let leg_offset = if forward { offset } else { leg.miles - offset };
        if let Some(baked) = leg_aadt_at(leg, leg_offset) {
            return baked;
        }
        let near = self.near_city(mile);
        // Urban interstates run three or more lanes per direction.
        let lanes = if near && highway_class(&leg.highway) == "interstate" {
            3
        } else {
            leg_lane_count(Some(leg))
        };
        (heuristic_aadt(&leg.highway, near), lanes)
    }

    /// Stretches where peak-hour demand approaches capacity. The zones are
    /// fixed in space; whether each is *active* follows the clock.
    pub fn place_congestion_zones(&mut self) -> Vec<Zone> {
        let total = self.route.miles();
        if self.is_facility_approach_route() || total < 10.0 {
            return Vec::new();
        }
        let peak_share = HOURLY_SHARE_WEEKDAY
            .iter()
            .cloned()
            .fold(f64::MIN, f64::max);
        // One draw for this stretch of road on this trip. Taken from a stream
        // of its own so that adding it does not shift where the work zones
        // land for a given seed -- those come off `self.rng`.
        let day = daily_volume_factor(&mut self.traffic_rng);
        let mut zones: Vec<Zone> = Vec::new();
        let mut mile = 0.0;
        while mile < total {
            let end = (mile + CONGESTION_SAMPLE_MI).min(total);
            let (aadt, lanes) = self.route_aadt_at(mile);
            let peak_ratio = aadt * day * peak_share * DIRECTIONAL_SPLIT
                / (lanes.max(1) as f64 * LANE_CAPACITY_VPH);
            if peak_ratio >= CONGESTION_MIN_RATIO {
                // Only coalesce contiguous samples with the same local inputs.
                // A median volume and minimum lane count can invent a bottleneck
                // that exists nowhere on the road. Joining across a clear gap
                // likewise turns uncongested miles into a speed restriction.
                let posted = self.corridor_limit_at(mile);
                if let Some(previous) = zones.last_mut().filter(|previous| {
                    previous.end_mi == mile
                        && previous.aadt == Some(aadt)
                        && previous.lanes == lanes
                        && self.corridor_limit_at(previous.start_mi) == posted
                }) {
                    previous.end_mi = end;
                } else {
                    zones.push(
                        // Refreshed from the clock when active.
                        Zone::new(mile, end, 50.0, "heavy traffic")
                            .with_congestion(Some(aadt), lanes)
                            .with_day_factor(day),
                    );
                }
            }
            mile = end;
        }
        zones.retain(|zone| zone.end_mi - zone.start_mi >= CONGESTION_MIN_ZONE_MI);
        zones
    }

    pub fn facility_speed_zones(&self) -> Vec<Zone> {
        let total = self.route.miles();
        if total <= 0.0 {
            return Vec::new();
        }
        // The gate zone is the yard entrance, and on a real chain that is
        // its LAST STREET -- not a fixed distance back from the end (owner
        // report, 2026-08-17; root cause found 2026-08-18).
        let gate_start =
            (total - FACILITY_GATE_ZONE_MI.min(total * FACILITY_GATE_MAX_SHARE)).max(0.0);
        if self.is_facility_approach_route() {
            if self.has_street_detail() {
                // Every street at its own posted limit, and the yard past the
                // driveway (`street_zones`).
                return self.street_zones();
            }
            // A chain baked before the street detail: ONE posted limit for
            // the whole chain, and the gate at the end (owner playtest,
            // 2026-08-21): the access road takes the state's own statutory
            // business-district limit, else the highest limit the legs offer.
            if self.route.legs.iter().any(|leg| leg.local_speed_mph > 0.0) {
                let chain_limit = self.statutory_street_mph().unwrap_or_else(|| {
                    self.route
                        .legs
                        .iter()
                        .filter(|leg| leg.local_speed_mph > 0.0)
                        .map(|leg| leg.local_speed_mph)
                        .fold(f64::MIN, f64::max)
                });
                let chain_limit = if chain_limit == f64::MIN {
                    FACILITY_ACCESS_LIMIT_MPH
                } else {
                    chain_limit
                };
                let last_leg_start = self.leg_starts.last().copied().unwrap_or(gate_start);
                let gate_zone = if self.outbound {
                    // Leaving the yard: the gate is behind you within the
                    // first street, and the chain ENDS at the on-ramp
                    // (Brandon, 2026-08-21).
                    let first_leg_end = if self.leg_starts.len() > 1 {
                        self.leg_starts[1]
                    } else {
                        total
                    };
                    Zone::new(
                        0.0,
                        first_leg_end.min(FACILITY_GATE_ZONE_MI),
                        FACILITY_GATE_LIMIT_MPH,
                        "facility gate",
                    )
                } else {
                    Zone::new(
                        gate_start.max(last_leg_start),
                        total,
                        FACILITY_GATE_LIMIT_MPH,
                        "facility gate",
                    )
                };
                return vec![
                    Zone::new(0.0, total, chain_limit, "facility access road"),
                    gate_zone,
                ];
            }
            // Graduated fallback (owner design, 2026-07-24): a long synthetic
            // approach is an arterial before it is an access road.
            let mut zones = Vec::new();
            let access_start = (total - FACILITY_ACCESS_TAIL_MI).max(0.0);
            let access_mph = self
                .statutory_street_mph()
                .unwrap_or(FACILITY_ACCESS_LIMIT_MPH);
            if access_start > 0.5 {
                zones.push(Zone::new(
                    0.0,
                    access_start,
                    FACILITY_ARTERIAL_LIMIT_MPH,
                    "facility approach",
                ));
                zones.push(Zone::new(
                    access_start,
                    total,
                    access_mph,
                    "facility access road",
                ));
            } else {
                zones.push(Zone::new(0.0, total, access_mph, "facility access road"));
            }
            if self.outbound {
                zones.push(Zone::new(
                    0.0,
                    total.min(FACILITY_GATE_ZONE_MI),
                    FACILITY_GATE_LIMIT_MPH,
                    "facility gate",
                ));
            } else {
                zones.push(Zone::new(
                    gate_start,
                    total,
                    FACILITY_GATE_LIMIT_MPH,
                    "facility gate",
                ));
            }
            return zones;
        }
        // Everything else ends on the highway, comes off at the destination
        // exit, and finishes on the facility's own local road: the local
        // approach capped at the ramp speed, and the gate itself. Ahead of
        // the local road the corridor's own limit stands up to the point a
        // driver has to start shedding for the ramp (Shane, 2026-08-15).
        let mut local_mi = self
            .destination_approach_mi
            .filter(|m| *m != 0.0)
            .unwrap_or(DESTINATION_LOCAL_APPROACH_MI);
        local_mi = local_mi.clamp(FACILITY_GATE_ZONE_MI, DESTINATION_APPROACH_TRUSTED_MAX_MI);
        let local_start = (total - local_mi).max(0.0);
        let entry_mph = self.corridor_limit_at((local_start - 0.05).max(0.0));
        let approach_start =
            (local_start - approach_shed_mi(entry_mph, DESTINATION_APPROACH_LIMIT_MPH)).max(0.0);
        vec![
            Zone::new(
                approach_start,
                total,
                DESTINATION_APPROACH_LIMIT_MPH,
                "destination approach",
            ),
            self.facility_gate_zone(),
        ]
    }

    /// Stretches under a winter chain law: sustained steep grade in a state
    /// that runs commercial chain controls (`CHAIN_CONTROL_STATES`), fixed in
    /// space at trip build. Whether the law is *active* follows the weather.
    pub fn place_chain_law_areas(&self) -> Vec<(f64, f64)> {
        self.steep_runs()
            .into_iter()
            .filter(|&(start, end)| {
                let mid = (start + end) / 2.0;
                let highway = &self.route.legs[self.leg_at_mile(mid).0].highway;
                self.state_code_at(mid).is_some_and(|code| {
                    CHAIN_CONTROL_STATES.contains(&code.as_str())
                        && on_chain_corridor(&code, highway)
                })
            })
            .collect()
    }

    /// Every sustained steep run on the route, joined across short gaps.
    fn steep_runs(&self) -> Vec<(f64, f64)> {
        if self.is_facility_approach_route() {
            return Vec::new();
        }
        let total = self.route.miles();
        let mut areas: Vec<(f64, f64)> = Vec::new();
        let mut run_start: Option<f64> = None;
        let mut mile = 0.0;
        while mile <= total {
            let steep = self.grade_at(mile).abs() >= CHAIN_LAW_MIN_GRADE;
            if steep && run_start.is_none() {
                run_start = Some(mile);
            } else if !steep {
                if let Some(start) = run_start {
                    if mile - start >= CHAIN_LAW_MIN_RUN_MI {
                        areas.push(((start - CHAIN_LAW_LEAD_MI).max(0.0), mile));
                    }
                    run_start = None;
                }
            }
            mile += CHAIN_LAW_SAMPLE_MI;
        }
        if let Some(start) = run_start {
            if total - start >= CHAIN_LAW_MIN_RUN_MI {
                areas.push(((start - CHAIN_LAW_LEAD_MI).max(0.0), total));
            }
        }
        let mut merged: Vec<(f64, f64)> = Vec::new();
        for area in areas {
            if let Some(last) = merged.last_mut() {
                if area.0 - last.1 <= CHAIN_LAW_JOIN_GAP_MI {
                    last.1 = area.1;
                    continue;
                }
            }
            merged.push(area);
        }
        merged
    }

    pub fn check_chain_law(&mut self) {
        let level = self.chain_law_level();
        if level == 0 || self.chain_law_areas.is_empty() {
            return;
        }
        let lookahead = self.zone_warning_lookahead_mi().max(1.0);
        let areas = self.chain_law_areas.clone();
        for (i, (start, end)) in areas.into_iter().enumerate() {
            let key = format!("chain-law:{i}:{level}");
            if self.announced_chain_law.contains(&key) {
                continue;
            }
            let ahead = start - self.position_mi;
            let inside = start <= self.position_mi && self.position_mi <= end;
            if !(inside || 0.0 < ahead && ahead <= lookahead) {
                continue;
            }
            self.announced_chain_law.insert(key);
            let rule = chain_law_sign_rule(self.state_code_at(start.max(0.0)).as_deref(), level);
            let (where_, pullout) = if inside {
                ("", "")
            } else {
                (" ahead", " Chain-up area on the right.")
            };
            self.emit(
                TripEventKind::GpsCue,
                SpokenMessage::new(format!("Chain law{where_}, {rule}.{pullout}")),
                TripEventData {
                    chain_law: Some(level),
                    chain_law_area: Some(i),
                    ..Default::default()
                },
            );
        }
    }

    /// Keyed by place and reason: the identity two zones share when they are
    /// "the same zone" (Python compared object identity).
    pub fn same_zone(a: Option<&Zone>, b: Option<&Zone>) -> bool {
        match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => zone_key(a) == zone_key(b),
            _ => false,
        }
    }
}
