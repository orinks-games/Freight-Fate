//! Axle-group loads: what a CAT Scale ticket prints. No extra saved state.
//!
//! A five-axle tractor-semitrailer stands on three axle groups: the steer
//! axle, the drive tandem and the trailer tandem. This splits the mass
//! model's own parts -- tractor with its diesel, trailer tare, cargo --
//! between them with two levers:
//!
//! - the trailer and its load rest on the kingpin and the trailer tandem,
//!   a fixed share `kingpin` on the kingpin;
//! - the kingpin load rests on the fifth wheel, just ahead of the drive
//!   tandem, so a small fixed share `fifth_wheel` of it reaches the steer
//!   axle.
//!
//! Both shares are DERIVED per truck from one anchor: a truck at the federal
//! 80,000 lb gross with a full tank scales 12,000 / 34,000 / 34,000, the
//! split a legal five-axle combination is built around (both tandems at the
//! federal limit, the steer axle holding the rest). So a load dispatch calls
//! legal is legal on every axle group, and an overweight load shows on the
//! tandems -- before a state scale weighs the gross.
//!
//! What the model cannot do: move weight between groups. The game has no
//! fifth-wheel or tandem slide and no load placement, so an axle cannot go
//! over while the gross is legal (ROADMAP follow-up).

use super::{combination_tare_kg, TruckState, KG_PER_LB, LEGAL_GVW_LB, TRAILER_TARE_KG};
use crate::pyfmt::fmt_grouped;

/// Federal tandem-axle limit on the Interstate, 23 U.S.C. 127(a). READ.
pub const TANDEM_LIMIT_LB: f64 = 34_000.0;
/// The steer axle's share of a legal 80,000 lb gross with both tandems at
/// their limit: 80,000 - 2 x 34,000. DERIVED.
pub const DESIGN_STEER_LB: f64 = LEGAL_GVW_LB - 2.0 * TANDEM_LIMIT_LB;
/// Share of a bare tractor's weight, diesel included, on its steer axle.
/// ASSUMED: no manufacturer publishes one figure. Drivers' posted bobtail
/// CAT Scale tickets run 53 to 57 percent (a sleeper at "a little over
/// 10,000" steer and 8,500 to 9,000 drives; a Kenworth T680 at 11,860 and
/// 9,040), TruckersReport forum, read 2026-09-24.
pub const TRACTOR_STEER_SHARE: f64 = 0.55;

/// What each axle group carries, in kilograms.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxleLoads {
    pub steer_kg: f64,
    pub drive_kg: f64,
    /// Zero bobtail.
    pub trailer_kg: f64,
}

impl AxleLoads {
    pub fn gross_kg(&self) -> f64 {
        self.steer_kg + self.drive_kg + self.trailer_kg
    }

    /// The ticket as spoken: each group and the gross in pounds, then
    /// either the groups that are over and by how much, or that it is legal.
    pub fn ticket_text(&self) -> String {
        let lb = |kg: f64| (kg / KG_PER_LB).round();
        let pounds = |kg: f64| format!("{} pounds", fmt_grouped(lb(kg), 0));
        let mut parts = vec![
            format!("Steer axle {}.", pounds(self.steer_kg)),
            format!("Drive axles {}.", pounds(self.drive_kg)),
        ];
        if self.trailer_kg > 0.0 {
            parts.push(format!("Trailer axles {}.", pounds(self.trailer_kg)));
        }
        parts.push(format!("Gross {}.", pounds(self.gross_kg())));
        let limits = [
            ("Drive axles", self.drive_kg, TANDEM_LIMIT_LB),
            ("Trailer axles", self.trailer_kg, TANDEM_LIMIT_LB),
            ("Gross", self.gross_kg(), LEGAL_GVW_LB),
        ];
        let over: Vec<String> = limits
            .iter()
            .filter(|(_, kg, limit)| lb(*kg) > *limit)
            .map(|(name, kg, limit)| {
                format!(
                    "{name} {} pounds over the {} pound limit.",
                    fmt_grouped(lb(*kg) - limit, 0),
                    fmt_grouped(*limit, 0)
                )
            })
            .collect();
        if over.is_empty() {
            parts.push("Legal on every axle.".to_string());
        } else {
            parts.extend(over);
        }
        parts.join(" ")
    }
}

impl TruckState {
    /// The tractor alone, with whatever diesel is aboard.
    fn tractor_kg(&self) -> f64 {
        let trailer = if self.trailer_attached {
            TRAILER_TARE_KG
        } else {
            0.0
        };
        (self.tare_kg() - trailer).max(0.0)
    }

    /// Where this truck's weight sits, axle group by axle group.
    pub fn axle_loads(&self) -> AxleLoads {
        let tractor = self.tractor_kg();
        let on_trailer = (self.gross_mass_kg() - tractor).max(0.0);
        // The anchor: this tractor full of diesel, at the legal gross.
        let full_tractor = (combination_tare_kg(&self.specs) - TRAILER_TARE_KG).max(0.0);
        let legal_on_trailer = LEGAL_GVW_LB * KG_PER_LB - full_tractor;
        let kingpin_share = if legal_on_trailer > 0.0 {
            (1.0 - TANDEM_LIMIT_LB * KG_PER_LB / legal_on_trailer).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let legal_kingpin = kingpin_share * legal_on_trailer;
        let fifth_wheel_share = if legal_kingpin > 0.0 {
            ((DESIGN_STEER_LB * KG_PER_LB - TRACTOR_STEER_SHARE * full_tractor) / legal_kingpin)
                .clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (kingpin, trailer_kg) = if self.trailer_attached {
            let kingpin = kingpin_share * on_trailer;
            (kingpin, on_trailer - kingpin)
        } else {
            (on_trailer, 0.0)
        };
        AxleLoads {
            steer_kg: TRACTOR_STEER_SHARE * tractor + fifth_wheel_share * kingpin,
            drive_kg: (1.0 - TRACTOR_STEER_SHARE) * tractor + (1.0 - fifth_wheel_share) * kingpin,
            trailer_kg,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{TruckSpecs, LEGAL_GVW_KG};
    use super::*;

    fn lb(kg: f64) -> f64 {
        (kg / KG_PER_LB).round()
    }

    fn truck(specs: TruckSpecs) -> TruckState {
        let mut t = TruckState::new(specs);
        t.fuel_gal = t.specs.fuel_tank_gal;
        t
    }

    #[test]
    fn a_full_legal_load_scales_the_design_split_on_every_tractor() {
        for mass_kg in [34_600.0, 36_000.0, 37_500.0] {
            let mut t = truck(TruckSpecs {
                mass_kg,
                ..TruckSpecs::default()
            });
            t.cargo_kg = LEGAL_GVW_KG - t.tare_kg();
            let axles = t.axle_loads();
            assert!((axles.gross_kg() - t.gross_mass_kg()).abs() < 1e-6);
            assert_eq!(lb(axles.trailer_kg), 34_000.0, "{mass_kg}");
            assert!(lb(axles.drive_kg) <= 34_000.0, "{mass_kg}");
            assert!(axles.ticket_text().ends_with("Legal on every axle."));
        }
        let mut rig = truck(TruckSpecs::default());
        rig.cargo_kg = LEGAL_GVW_KG - rig.tare_kg();
        let axles = rig.axle_loads();
        assert_eq!(
            (lb(axles.steer_kg), lb(axles.drive_kg)),
            (12_000.0, 34_000.0)
        );
    }

    #[test]
    fn an_empty_trailer_reads_like_an_empty_ticket() {
        let mut t = truck(TruckSpecs::default());
        t.cargo_kg = 0.0;
        let axles = t.axle_loads();
        // Steer heaviest of the three, the empty trailer lightest: the shape
        // of any empty dry-van ticket.
        assert!(lb(axles.steer_kg) > 9_500.0 && lb(axles.steer_kg) < 12_000.0);
        assert!(axles.drive_kg > axles.trailer_kg);
        assert!(lb(axles.trailer_kg) > 6_000.0 && lb(axles.trailer_kg) < 9_000.0);
    }

    #[test]
    fn an_overweight_load_shows_on_the_tandems() {
        let mut t = truck(TruckSpecs::default());
        t.cargo_kg = LEGAL_GVW_KG - t.tare_kg() + 1_000.0 * KG_PER_LB;
        let text = t.axle_loads().ticket_text();
        assert!(text.contains("Drive axles") && text.contains("over the 34,000 pound limit"));
        assert!(text.contains("Trailer axles") && text.contains("Gross 1,000 pounds over"));
        assert!(!text.contains("Legal"));
    }

    #[test]
    fn bobtail_has_no_trailer_line() {
        let mut t = truck(TruckSpecs::default());
        t.trailer_attached = false;
        t.cargo_kg = 0.0;
        let axles = t.axle_loads();
        assert_eq!(axles.trailer_kg, 0.0);
        assert!((axles.gross_kg() - t.gross_mass_kg()).abs() < 1e-6);
        let text = axles.ticket_text();
        assert!(!text.contains("Trailer"));
        assert!(text.starts_with("Steer axle "));
    }
}
