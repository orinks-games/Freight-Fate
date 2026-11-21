//! The two moments a career changes shape: losing the seat, losing the truck
//! (port of `freight_fate/states/career_setback.py`).
//!
//! Both are the longest and most consequential things the game ever says, and
//! both remove the tractor the driver was in. A single spoken line would be
//! gone to the first keypress, so each one lands as a screen the player can
//! arrow through, re-read line by line, and leave when they are ready -- the
//! same cadence as the save notices.
//!
//! Neither is an ending. The save is intact, the career is intact, and there
//! is freight on the board in the morning; the last line of every notice says
//! where to go next, and the screen refuses to close without the player
//! acknowledging it.

use ff_core::models::solvency;

use crate::app::{GameContext, Say};
use crate::impl_state_for_menu;
use crate::states::base::{Menu, MenuCore, MenuItem};

fn setback_title(kind: &str) -> &'static str {
    match kind {
        "termination" => "Your carrier has ended your employment",
        "repossession" => "The lender has taken the truck back",
        "disqualification" => "Your driving career is over",
        _ => "A change to your career",
    }
}

/// A termination or a repossession, told once and re-readable.
pub struct CareerSetbackNoticeState {
    menu: MenuCore<Self>,
    pub kind: String,
    pub lines: Vec<String>,
}

impl CareerSetbackNoticeState {
    /// Reads the pending notice off `ctx.profile.driving_record`.
    pub fn new(ctx: &GameContext) -> Self {
        let (kind, lines) = match ctx.profile.as_ref() {
            Some(p) => (
                p.driving_record.setback_notice_kind.clone(),
                p.driving_record.setback_notice_lines.clone(),
            ),
            None => (String::new(), Vec::new()),
        };
        Self {
            menu: MenuCore::new(setback_title(&kind)).with_intro_help(
                "Up and down reread the lines. Enter repeats a line. Continue or Escape \
                 returns to the terminal.",
            ),
            kind,
            lines,
        }
    }

    pub fn title(&self) -> &str {
        &self.menu.title
    }

    fn acknowledge(&mut self, ctx: &mut GameContext) {
        if let Some(p) = ctx.profile.as_mut() {
            solvency::clear_setback_notice(p);
        }
        ctx.save_profile();
        ctx.pop_state();
    }
}

impl Menu for CareerSetbackNoticeState {
    /// A prompt to answer: Escape answers it, so touch adds no Back row.
    fn touch_back_row(&self) -> bool {
        false
    }

    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        ctx.audio.play("ui/notify");
        ctx.say(&format!("{}. {}", self.menu.title, self.lines.join(" ")));
        let current = self.current_text(ctx);
        ctx.say_with(current, Say::queued().review(false));
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = self
            .lines
            .iter()
            .map(|line| {
                let spoken = line.clone();
                MenuItem::new(line.clone(), move |_s: &mut Self, ctx| ctx.say(&spoken))
                    .help("Repeat this line.")
            })
            .collect();
        items.push(
            MenuItem::new("Continue", |s: &mut Self, ctx| s.acknowledge(ctx))
                .help("Back to the terminal menu."),
        );
        items
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        // Escape acknowledges too; the player is never stuck on this screen.
        self.acknowledge(ctx);
    }
}

impl_state_for_menu!(CareerSetbackNoticeState);
