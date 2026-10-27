//! The spoken side of cloud backup: the reason tables and every
//! player-facing sentence the queue, the Save game watch and the Cloud
//! backup menus share. Split out of `cloud_saves.rs` so the wording is one
//! file to read; everything is re-exported from `crate::cloud_saves`.

// The spoken guidance for CloudAuthError, shared by every cloud menu so the
// player always hears the same recovery path. orinks.net issues one token
// per computer, so a refusal means this computer's token was signed out
// from the account's computer list (or replaced by a sign-out-everywhere).
//
// Every control named here has to be one this player can actually find. The
// recovery used to be "choose Add computer to get a fresh token, then paste
// it": both halves of that died with the clipboard setup, and the setup page
// has no Add computer button at all now -- a computer is added by activating
// it from the game. The menu item is named as it reads while the identity
// file is still on disk (which it is, in this failure): the Online hub says
// "orinks.net account: connected", not "Set up orinks.net account".
pub const AUTH_HELP: &str = "orinks.net no longer accepts this computer's sign-in. Usually this \
computer was signed out from the computer list on your driver setup page. To connect it \
again, open the Online menu, choose orinks.net account, then Set up this computer with \
orinks.net, and enter the activation code in your browser. If your driver is not on that \
page at all, the account itself is gone. Make a new account and connect it the same way.";

// -- upload failure classification ---------------------------------------------
//
// `upload_save` hands back a `reason` string that is a network problem, an
// auth problem, or one of the validator's refusal codes -- three situations a
// player needs three different honest sentences for (Jessie's report,
// 2026-08-14: an `invalid_achievement` refusal was told to the player as
// "check your connection", which sent them chasing their network for a
// problem that was never there). Every caller that turns an upload result
// into player-facing wording -- the background queue in `upload_slot` and
// the foreground "keep this computer's save" retry in
// `CloudSaves::resolve_keep_mine` -- must classify through the one
// table below, so a new validator code only has to be added in one place.

// The credentials were retired (usually by connecting another computer, or a
// driver record that no longer exists); every retry fails identically until
// the player reconnects from the Online menu.
pub const AUTH_FAILURE_REASONS: &[&str] = &["unauthorized", "driver_not_found", "http_401"];

// The server read this save and refused it outright. Retrying with the same
// save can never succeed -- it is not a connection problem, it is something
// for the developers to fix.
pub const REJECTED_UPLOAD_REASONS: &[&str] = &[
    "too_large",
    "invalid_schema",
    "invalid_name",
    "invalid_city",
    "invalid_range",
    "invalid_possession",
    "invalid_career",
    "impossible_xp",
    "impossible_money",
    "invalid_market",
    "invalid_hos",
    "invalid_achievement",
    "unsupported_version",
    // Neither of these can succeed on a retry, and both were missing here
    // until Shane's three careers refused at once (2026-08-15): they fell
    // through to "network", so the queue backed off and tried again
    // forever while the player was told nothing at all.
    "too_many_slots",
    "signing_unavailable",
    // A career carrying the "changed outside the game" mark waits for the
    // owner to review it; a retry cannot change that answer, so the queue
    // must stop instead of retrying every two minutes forever.
    "review_declined",
];

/// Sort an `upload_save` failure `reason` into the family its
/// player-facing wording actually differs by.
///
/// Returns `"auth"`, `"rejected"`, or `"network"` -- the last one is
/// the honest default for anything not recognized (a raw network error, a
/// 5xx, or a code the validator has not been taught to this table yet):
/// treating an unknown reason as transient and worth a retry is the safe
/// failure mode, never the other way around.
pub fn classify_upload_failure(reason: Option<&str>) -> &'static str {
    let Some(reason) = reason else {
        return "network";
    };
    if AUTH_FAILURE_REASONS.contains(&reason) {
        return "auth";
    }
    if REJECTED_UPLOAD_REASONS.contains(&reason) {
        return "rejected";
    }
    "network"
}

// Within "rejected", the two arithmetic cross-checks -- recomputed XP ceiling,
// recomputed money ceiling -- earn a different story than every other refusal:
// only a real cross-check failure means the numbers themselves do not add up,
// so only this pair says "flagged for review" and offers the appeal. A false
// flag hit a real career on this exact wording (2026-08-14), so the appeal
// sentence stays attached to it on purpose.
pub const ARITHMETIC_REJECTION_REASONS: &[&str] = &["impossible_xp", "impossible_money"];

// Schema and version refusals mean this build and the server disagree about
// what a save even looks like -- almost always a build gap, not something the
// player did to the save.
pub const SCHEMA_REJECTION_REASONS: &[&str] = &["invalid_schema", "unsupported_version"];

// The server checks the town a career is parked in against its own city list,
// so a city the game knows and the server has not caught up with refuses every
// backup from that career until the server is updated. That is a real failure
// mode, not a hypothetical -- a tester's backups stopped for a day on a stale
// deployed catalog (2026-08-14) -- and under the generic wording it looked like
// an unexplained refusal. Nothing the player can do about it, so say so.
pub const CATALOG_REJECTION_REASONS: &[&str] = &["invalid_city"];

// The one refusal a player can clear without anyone's help: the server keeps a
// fixed number of backed-up careers, and the answer is to remove one from the
// Cloud backup menu. Under the generic line it read as an unexplained failure
// with nothing to do about it, which is the opposite of the truth.
pub const SLOTS_FULL_REJECTION_REASONS: &[&str] = &["too_many_slots"];

// The server accepted the save and then could not sign it -- its own
// configuration, nothing about this career. Says so plainly rather than
// implying the save was judged and found wanting.
pub const SERVER_FAULT_REJECTION_REASONS: &[&str] = &["signing_unavailable"];

/// The player-facing status line for a server-refused upload.
///
/// Always names the career (Shane's report, 2026-08-14: with more than one
/// career backed up he could not tell which one had been refused, or why),
/// then splits the "rejected" family by what the reason code actually means
/// to a player instead of one line for every cause. Shared by the background
/// auto-backup queue and the foreground "keep this computer's save" retry
/// (`CloudSaves::resolve_keep_mine`, via `cloud_save_states`) so both speak
/// the same story for the same reason code.
pub fn rejection_status(name: &str, reason: Option<&str>) -> String {
    let reason = reason.unwrap_or("");
    if reason == "review_declined" {
        return format!(
            "{name}: backup declined after review. This career no longer backs \
up to your orinks.net account. Your local career is safe."
        );
    }
    if ARITHMETIC_REJECTION_REASONS.contains(&reason) {
        return format!(
            "{name}: backup not accepted. The numbers in this save do not \
look like possible play, so the server declined it and flagged \
it for review. Your local career is safe and nothing public \
changed. If you think this is wrong, say so in the tester \
document."
        );
    }
    if SCHEMA_REJECTION_REASONS.contains(&reason) {
        return format!(
            "{name}: backup not accepted. Your game and the server \
disagree about this save's shape, usually a build mismatch, \
not something you did. Your local career is safe."
        );
    }
    if CATALOG_REJECTION_REASONS.contains(&reason) {
        return format!(
            "{name}: backup not accepted. The server does not recognize the \
town this career is parked in, which usually means it has not \
caught up with this build yet. Your local career is safe, and \
backups start working again on their own once it has."
        );
    }
    if SLOTS_FULL_REJECTION_REASONS.contains(&reason) {
        return format!(
            "{name}: backup not accepted. You have as many careers backed up \
as the server keeps, so there is no room for this one. Remove a \
career from the Cloud backup menu and this will back up again. \
Your local career is safe."
        );
    }
    if SERVER_FAULT_REJECTION_REASONS.contains(&reason) {
        return format!(
            "{name}: backup not accepted. The server could not finish signing \
this backup, which is a problem at our end and not anything about \
your career. Your local career is safe, and backups start working \
again on their own once it is fixed."
        );
    }
    format!(
        "{name}: backup not accepted. Your local career is safe. Public details were not updated."
    )
}

// The status line for the auth family, shared by the background queue and the
// manual "Save game" announcement so a paused sign-in is always told the same
// way. AUTH_HELP (above) carries the full recovery path when a menu can offer
// it; this is the short standing line.
pub const AUTH_PAUSED_STATUS: &str = "Backups are paused: orinks.net no longer accepts this \
computer's sign-in. Reconnect from the Online menu.";

/// The player-facing line for a slot the server refused to overwrite
/// because another computer advanced it. Shared by the manual "Save game"
/// result (states/city.py) and the background queue's spoken announcement
/// so a conflict is always told the same way.
pub fn conflict_status(name: &str) -> String {
    format!(
        "{name} needs attention: the cloud copy changed on another \
computer. Open Restore a cloud backup on the Online menu \
to choose which copy to keep."
    )
}

/// The spoken all-clear for an ordinary accepted background backup.
///
/// Career-named like every other backup story, and deliberately the shortest
/// of them: it fires at every rest stop, motel, sleep and delivery, so it has
/// to be something a driver can hear many times a run without it becoming
/// noise. "backed up" is the wording the rest of the feature already uses --
/// the Save game line, the recovery line, the Cloud backup menu -- so this
/// adds a moment to say it, not a new noun to learn.
pub fn backup_status(name: &str) -> String {
    format!("{name} is backed up.")
}

/// The spoken all-clear for a career whose backup refusal was announced:
/// one line, career-named like every other backup story, when a later
/// upload of that slot is accepted again. Says "again" because it answers
/// the refusal the driver already heard; an ordinary success uses
/// [`backup_status`].
pub fn recovery_status(name: &str) -> String {
    format!("{name} is backed up again.")
}

pub(crate) fn accepted_outcome(result: &serde_json::Map<String, serde_json::Value>) -> String {
    let Some(raw_name) = result
        .get("evictedSaveName")
        .and_then(serde_json::Value::as_str)
    else {
        return "accepted".to_string();
    };
    let safe_name = super::save_slot_name(raw_name);
    if raw_name != safe_name || safe_name.chars().count() > 64 {
        log::warn!("Ignored malformed cloud eviction save name");
        return "accepted".to_string();
    }
    format!("accepted:evicted:{safe_name}")
}

/// The exact line used after the server removes an old cloud career.
pub fn eviction_status(name: &str) -> String {
    format!(
        "Cloud backup removed {name}, the least recently played cloud career. Your local career was not deleted."
    )
}
