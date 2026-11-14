//! The ambient bed's hands on the sound engine (see `ff_core::ambient_bed`).
//!
//! Screens never write the ambient channel directly. One that starts a bed
//! and stops it when it leaves claims it with [`GameContext::claim_ambient`]
//! and hands the claim back with [`GameContext::release_ambient`]; a bed set
//! frame by frame goes through [`GameContext::set_ambient`].

use ff_core::ambient_bed::{AmbientBed, AmbientCue, AmbientEvent, BedClaim};

use super::GameContext;

impl GameContext {
    /// Start `key` as this screen's bed. Keep the claim and release it on exit.
    pub fn claim_ambient(&mut self, key: &str) -> BedClaim {
        self.ambient_claims += 1;
        let claim = BedClaim(self.ambient_claims);
        self.move_ambient(AmbientEvent::Claim { key, claim });
        claim
    }

    /// The screen that made `claim` is leaving. Stops the bed only if no
    /// screen has claimed or set it since.
    pub fn release_ambient(&mut self, claim: BedClaim) {
        self.move_ambient(AmbientEvent::Release(claim));
    }

    /// Play `key` with no owner, or stop the bed with `None`.
    pub fn set_ambient(&mut self, key: Option<&str>) {
        self.move_ambient(AmbientEvent::Set(key));
    }

    /// What the bed holds now.
    pub fn ambient_bed(&self) -> &AmbientBed {
        &self.ambient_bed
    }

    fn move_ambient(&mut self, event: AmbientEvent<'_>) {
        let (bed, cue) = self.ambient_bed.next(event);
        self.ambient_bed = bed;
        match cue {
            AmbientCue::Start(key) => self.audio.set_ambient(Some(&key)),
            AmbientCue::Stop => self.audio.set_ambient(None),
            AmbientCue::Keep => {}
        }
    }
}
