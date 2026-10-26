//! Port of `freight_fate/single_instance.py` — single-instance launch guard
//! for the game process.
//!
//! One named mutex per Windows session: the second launch sees
//! `ERROR_ALREADY_EXISTS` and declines to start. Everything fails open --
//! a mutex the OS will not create must never keep a player out of the game.
//! The guard is Hearken's; the name is Freight Fate's, and equals
//! `hearken::single_instance::mutex_name("FreightFate")`.

pub use hearken::single_instance::*;

pub const SINGLE_INSTANCE_MUTEX_NAME: &str = "Local\\FreightFate.SingleInstance";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mutex_name_is_hearkens_name_for_the_data_folder() {
        assert_eq!(mutex_name("FreightFate"), SINGLE_INSTANCE_MUTEX_NAME);
    }
}
