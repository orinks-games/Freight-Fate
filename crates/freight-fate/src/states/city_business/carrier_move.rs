//! Business status: moving up from the training fleet to another carrier.
//!
//! The rows exist only for a company driver at Great Lakes Training
//! Transport. Locked, the row says what is still in the way; open, there is
//! one row per carrier, and each asks twice, because there is no moving back.

use ff_core::models::carrier_move::{
    apply_carrier_move, at_training_fleet, move_up_carriers, move_up_eligibility,
};
use ff_core::models::start_options::CareerStartOption;

use super::{save_business_change, BusinessStatusState};
use crate::app::GameContext;
use crate::states::base::{Label, Menu, MenuItem};
use crate::states::city::{profile, profile_mut};

impl BusinessStatusState {
    pub(super) fn carrier_move_items(&self, ctx: &GameContext) -> Vec<MenuItem<Self>> {
        let p = profile(ctx);
        if !at_training_fleet(p) {
            return Vec::new();
        }
        let (ok, reasons) = move_up_eligibility(p);
        if !ok {
            return vec![MenuItem::new(
                "Move to another carrier: locked",
                move |_s: &mut Self, ctx: &mut GameContext| {
                    ctx.say(&format!("Move to another carrier. {}", reasons.join(" ")));
                },
            )
            .help("What another carrier needs before it hires you away.")];
        }
        move_up_carriers()
            .into_iter()
            .map(|option| {
                let key = option.key;
                MenuItem::new(
                    Label::dynamic(move |s: &Self, _ctx| {
                        if s.move_armed == Some(key) {
                            format!(
                                "Move to {}: press Enter again to confirm",
                                option.carrier_name
                            )
                        } else {
                            format!("Move to {}", option.label)
                        }
                    }),
                    move |s: &mut Self, ctx| s.move_to_carrier(ctx, option),
                )
                .help(option.menu_summary)
            })
            .collect()
    }

    fn move_to_carrier(&mut self, ctx: &mut GameContext, option: &'static CareerStartOption) {
        self.return_armed = false;
        if self.move_armed != Some(option.key) {
            self.move_armed = Some(option.key);
            ctx.say(&format!(
                "{}. {} Press Enter again to move.",
                option.carrier_name, option.menu_summary
            ));
            self.refresh(ctx, true);
            return;
        }
        self.move_armed = None;
        let line = apply_carrier_move(profile_mut(ctx), option);
        save_business_change(ctx);
        ctx.audio.play("ui/notify");
        ctx.say(&line);
        self.refresh(ctx, false);
    }
}
