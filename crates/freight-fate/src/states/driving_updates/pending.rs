//! What is left of `driving_updates.py`'s stub module.
//!
//! Every block that stood here -- the roadside screens a resolved stop
//! pushes, and the two record-keeping helpers -- has been deleted by the
//! module that took it over, `states/driving_rest_states.rs`, exactly as
//! `states/driving.rs`'s own `pending` module and `driving_events/pending.rs`
//! work.
//!
//! One parameter struct stays, because it never was a stub: the two callers
//! in `driving_updates` fill it in differently and share nothing else.

/// The keyword arguments of `EnforcementStopState`, which the two callers in
/// `driving_updates` fill in differently.
pub struct EnforcementStopParams {
    pub title: String,
    pub summary: String,
    pub fine: f64,
    pub reputation_hit: f64,
    pub signaled: bool,
    pub return_message: String,
    pub out_of_service: bool,
    pub warned: bool,
    pub construction_zone: bool,
    pub inspection_on_stop: bool,
    /// A routine roadside inspection: the stop IS the inspection, at this
    /// level, and the report decides the fine, not the caller.
    pub inspection_level: Option<ff_core::sim::roadside_inspection::InspectionLevel>,
}
