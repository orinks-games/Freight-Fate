use crate::cloud_saves::{
    cloud_slot_name, conflict_status, eviction_status, rejection_status, AUTH_PAUSED_STATUS,
    STALE_BACKUP_STATUS,
};

use super::*;

impl CityMenuState {
    /// `_save`: write the career and, with cloud backup on, back it up now.
    pub fn save(&mut self, ctx: &mut GameContext) {
        ctx.save_profile();
        ctx.audio.play("ui/notify");
        // A manual save is the player asking for certainty (Shane's report,
        // 2026-08-14): the cloud backup runs right away and the result is
        // spoken, because a silent background upload is indistinguishable
        // from no backup for a screen reader user. update() speaks exactly
        // one result line when the attempt lands.
        // Sandbox runs (driving school, forced playtest scenarios) never reach
        // disk in save_profile, so their throwaway profile must never reach
        // the cloud either -- it would overwrite the real career's slot.
        let sandbox = ctx.school_sandbox || ctx.playtest_sandbox;
        let cloud = ctx.cloud_saves_service();
        if cloud.enabled() && ctx.profile.is_some() && !sandbox {
            let p = profile(ctx);
            let token = cloud.backup_now(&p.name, serde_json::Value::Object(p.to_dict()));
            if let Some(token) = token {
                self.backup_watch = Some((cloud_slot_name(&p.name), token, BACKUP_RESULT_WAIT_S));
                ctx.say("Game saved. Backing up.");
                return;
            }
        }
        ctx.say("Game saved.");
        let cloud = ctx.cloud_saves_service();
        if cloud.identity().is_some() && !cloud.enabled() {
            // The player set up an account but backup is off: say so here,
            // where they asked to save, instead of only in the Online menu.
            // With no account configured, saving stays local and quiet.
            let status = cloud.status();
            ctx.say_with(status, Say::queued());
        }
    }

    /// One spoken line per cloud backup outcome family, reusing the
    /// standing status wording wherever one already exists.
    pub(super) fn backup_outcome_text(name: &str, outcome: &str) -> String {
        if outcome == "accepted" {
            return "Backed up to the cloud.".to_string();
        }
        if let Some(name) = outcome.strip_prefix("accepted:evicted:") {
            return eviction_status(name);
        }
        if outcome == "unchanged" {
            // "Already backed up" alone reads as a refusal to a driver who
            // just fuelled and bought tires: they know the career changed, so
            // being told there is nothing to send sounds like the game is
            // wrong (Shane, 2026-08-15). It is not -- every one of those
            // actions saves, and a save backs up on its own, so the server
            // really is current. Say the part that answers his worry: the
            // copy up there matches the one on this computer.
            return "Already backed up. The cloud copy matches this computer's save.".to_string();
        }
        if let Some(reason) = outcome.strip_prefix("rejected:") {
            return rejection_status(name, Some(reason));
        }
        if outcome == "conflict" {
            return conflict_status(name);
        }
        if outcome == "stale" {
            return STALE_BACKUP_STATUS.to_string();
        }
        if outcome == "auth" {
            return AUTH_PAUSED_STATUS.to_string();
        }
        "The backup will keep retrying in the background.".to_string()
    }
}
