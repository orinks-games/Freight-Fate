//! The no-chains reminder the dispatch board speaks once before it
//! accepts a load that needs chains the truck does not carry.

use ff_core::models::jobs::Job;
use ff_core::sim::chain_control::{posted_level, RoadReading};
use ff_core::sim::real_weather_alerts::{alerts_raise_chain_law, warm_route_alerts};
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::trip_models::{rig_must_chain, ChainCorridorAhead, CHAIN_LAW_LEAD_MI};
use ff_core::sim::vehicle::TruckState;
use ff_core::sim::weather::{effects, WeatherSystem};
use ff_core::sim::weather_service::WeatherService;

use crate::app::GameContext;
use crate::states::city::profile;

/// Stop planning's no-chains reminder, while the driver is still at the
/// terminal and its garage is open, with no usable chains aboard: a chain
/// law in effect right now on a chain area of the loaded route that chains
/// this rig (`posted_chain_law_on_route`), or a carry-rule corridor in
/// season (`Trip::chain_carry_corridors`). Speech only; a second Enter
/// accepts as usual.
pub(super) fn chain_reminder_before_accepting(ctx: &mut GameContext, job: &Job) -> Option<String> {
    {
        let p = profile(ctx);
        if !ctx.settings.hos_planning_hints
            || job.bobtail
            || (p.chains_owned() && p.chain_wear_pct() < 100.0)
        {
            return None;
        }
    }
    let route = ctx
        .world
        .supported_route(&job.origin, &job.destination, None)
        .ok()??;
    // The sky here is never the offline forecast: that draw is unseeded
    // until the drive starts, so it would name a law the road won't post.
    let trip = Trip::new(
        route,
        TruckState::default(),
        WeatherSystem::new("", Some(1), None, None, false),
        TripOptions {
            seed: Some(1),
            career_hours: Some(profile(ctx).game_hours),
            world: Some(ctx.world),
            ..Default::default()
        },
    );
    if let Some(ahead) = posted_chain_law_on_route(ctx, &trip) {
        let highway = ahead.highway;
        return Some(format!(
            "A chain law is in effect on {highway} on this route. You have no chains. The \
             garage here sells them. Enter again to accept."
        ));
    }
    let ahead = trip.chain_carry_corridors().into_iter().next()?;
    let highway = ahead.highway;
    Some(format!(
        "Trucks on {highway} must carry chains this season. You have no chains. The garage \
         here sells them. Enter again to accept."
    ))
}

/// The first chain area on `trip`'s route where a chain law is in effect
/// now and chains this rig (`rig_must_chain`). The level is read from the
/// live sources only: the Weather Service alerts cached for the cities at
/// either end of the area's leg (`warm_route_alerts`, the same cache
/// dispatch reads at the pickup), and with live weather on, those cities'
/// observed snow or ice. With live sources off there is nothing to read,
/// so the board reminds about carry-rule corridors only.
fn posted_chain_law_on_route(ctx: &mut GameContext, trip: &Trip) -> Option<ChainCorridorAhead> {
    let alerts = ctx.weather_alerts_provider_arc();
    if let Some(alerts) = &alerts {
        warm_route_alerts(&trip.route, alerts, ctx.world);
    }
    let world = ctx.world;
    let cities = trip.route.cities.clone();
    let mut sky_level = |key: &str| -> i64 {
        let Some(city) = world.cities.get(key) else {
            return 0;
        };
        let alert_raises = alerts
            .as_ref()
            .and_then(|a| a.get(city.lat, city.lon))
            .is_some_and(|found| alerts_raise_chain_law(&found));
        let (lat, lon) = (city.lat, city.lon);
        // The city's country picks the live service.
        let service = WeatherService::for_country(&city.country);
        let observed = ctx
            .real_weather_provider()
            .zip(service)
            .and_then(|(provider, service)| {
                // Keyed as the pickup's route weather is (`city_pickup`).
                provider.request_from(service, key, lat, lon);
                provider.get(key)
            });
        // The law follows the road: only observed snow or ice posts one,
        // and a warning only raises it (`chain_control::posted_level`).
        posted_level(RoadReading {
            surface: observed.map_or("dry", |kind| effects(kind).surface),
            alert_raises,
        })
    };
    let areas = trip.chain_corridor_areas(0.0, trip.route.miles(), |_, _| true);
    for area in areas {
        let (leg, _) = trip.leg_at_mile(area.start_mi + CHAIN_LAW_LEAD_MI);
        let level = cities
            .get(leg..=leg + 1)
            .unwrap_or_default()
            .iter()
            .map(|key| sky_level(key))
            .max()
            .unwrap_or(0);
        if level > 0 && rig_must_chain(Some(&area.state), level) {
            return Some(area);
        }
    }
    None
}
