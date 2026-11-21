//! The load a driver was told to sleep for.
//!
//! The board's hours warning says "Enter again accepts anyway, or sleep
//! first". A 10-hour sleep usually crosses midnight, a new market day
//! rebuilds the board, and the load the driver picked used to vanish with
//! the old one. The warned load is remembered beside the cached board and
//! carried onto the next board built in the same city, once.

use serde_json::{Map, Value};

use ff_core::data::world::World;
use ff_core::models::jobs::{job_from_payload, job_payload, normalize_job_cities, plan_hos, Job};
use ff_core::models::profile::Profile;

use crate::states::city_pickup::job_origin_exists;

use super::farthest_slot;

/// Cache field: `{"city": ..., "job": payload}` for the load awaiting a rest.
const HELD: &str = "held_for_rest";
/// Cache field: the board index the held load landed at after a rebuild.
const HELD_INDEX: &str = "held_index";

/// Remember `job` as the load the driver was warned off for hours, so the
/// board rebuilt after their rest still offers it.
pub(crate) fn hold_for_rest(p: &mut Profile, job: &Job) {
    let mut held = Map::new();
    held.insert("city".into(), Value::from(p.current_city.clone()));
    held.insert("job".into(), Value::Object(job_payload(job)));
    let cache = p
        .dispatch_board_cache
        .get_or_insert_with(|| Value::Object(Map::new()));
    if let Some(cache) = cache.as_object_mut() {
        cache.insert(HELD.into(), Value::Object(held));
    }
}

/// The held load, if it was held on this city's board and its pickup still
/// exists in the world.
pub(crate) fn held_load(p: &Profile, world: &World) -> Option<Job> {
    let held = p
        .dispatch_board_cache
        .as_ref()?
        .as_object()?
        .get(HELD)?
        .as_object()?;
    let city = held.get("city")?.as_str()?;
    if world.resolve_city_key(city) != world.resolve_city_key(&p.current_city) {
        return None;
    }
    let mut job = job_from_payload(held.get("job")?.as_object()?)?;
    normalize_job_cities(&mut job, world);
    if !job_origin_exists(&job, world) {
        return None;
    }
    // The offer was written on the tired clock. Its deadline stays (it only
    // errs generous), but the line about a forced rest is re-judged on the
    // hours the driver has now, the way the board judged it when posted.
    if job.deadline_covers_rest {
        let route = world
            .supported_route(&job.origin, &job.destination, None)
            .ok()
            .flatten();
        job.deadline_covers_rest =
            plan_hos(job.distance_mi, route.as_ref(), Some(world), Some(&p.hos)).sleeps
                > plan_hos(job.distance_mi, route.as_ref(), Some(world), None).sleeps;
    }
    Some(job)
}

/// Put the held load on a freshly built board and record where it landed
/// in the new cache. It takes the row of the farthest load from this city,
/// as a relayed load does, so the board keeps the size the driver's level
/// and standing earn. A relayed load (from another city) and the playtest
/// lever's forced load (`keep`) keep their rows.
pub(crate) fn carry_onto_board(
    fresh: &mut Vec<Job>,
    held: Job,
    cache: &mut Map<String, Value>,
    keep: impl Fn(&Job) -> bool,
) {
    let payload = Value::Object(job_payload(&held));
    let same = |job: &Job| Value::Object(job_payload(job)) == payload;
    if !fresh.iter().any(same) {
        let origin = held.origin.clone();
        match farthest_slot(fresh, |job| job.origin == origin && !keep(job)) {
            Some(slot) => fresh[slot] = held,
            None => fresh.push(held),
        }
    }
    if let Some(index) = fresh.iter().position(same) {
        cache.insert(HELD_INDEX.into(), Value::from(index as i64));
    }
}

/// Board index of the load carried over from before the driver's rest.
pub(crate) fn held_index(p: &Profile) -> Option<usize> {
    let index = p
        .dispatch_board_cache
        .as_ref()?
        .as_object()?
        .get(HELD_INDEX)?
        .as_i64()?;
    usize::try_from(index).ok()
}
