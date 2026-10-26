//! What the window should show this frame, as plain data.
//!
//! [`Scene::capture`] reads the screen stack and, when a drive is on it,
//! the drive itself, into values the painter can draw without touching the
//! game again. Keeping that copy separate is what lets the display skip a
//! frame that would look exactly like the last one: two captures compare
//! equal when nothing on screen would change.

use crate::app::GameContext;
use crate::playtest::menu::parse_rows;
use crate::states::base::State;
use crate::states::driving::DrivingState;
use crate::states::driving_school::SchoolDrivingState;

pub use super::scene_drive::{
    DriveView, Hud, LaneLook, RoadAhead, SignKind, SignView, Signal, TrafficView, VehicleKind,
    WeatherLook, WorkZone,
};

/// A screen that is not the road: a menu, a reader, a prompt.
#[derive(Clone, Debug, PartialEq)]
pub struct Panel {
    pub title: String,
    /// Lines between the title and the rows (an arrival summary, a status
    /// block), or every line of a screen that is not a menu.
    pub body: Vec<String>,
    /// The menu's rows and which has focus, when the screen is a menu.
    pub rows: Option<(Vec<String>, usize)>,
    /// Which body line the screen marks as current (a help page's `>`).
    pub body_focus: Option<usize>,
}

impl Panel {
    /// Read a screen's visible lines the way the playtest harness does: the
    /// first line is the title, a menu's rows come last after a blank line.
    pub fn from_lines(lines: &[String]) -> Option<Panel> {
        let (title, rest) = lines.split_first()?;
        let rows = parse_rows(lines);
        let body_end = if rows.is_some() {
            lines.iter().rposition(|l| l.is_empty()).unwrap_or(0)
        } else {
            lines.len()
        };
        let mut body: Vec<String> = rest[..body_end.saturating_sub(1).min(rest.len())].to_vec();
        // Drop the blank separator lines at either end; inner ones are
        // paragraph breaks and stay.
        while body.first().is_some_and(|l| l.trim().is_empty()) {
            body.remove(0);
        }
        while body.last().is_some_and(|l| l.trim().is_empty()) {
            body.pop();
        }
        let mut body_focus = None;
        for (i, line) in body.iter_mut().enumerate() {
            if let Some(stripped) = line.strip_prefix("> ") {
                body_focus = Some(i);
                *line = stripped.to_string();
            } else if let Some(stripped) = line.strip_prefix("  ") {
                *line = stripped.to_string();
            }
        }
        Some(Panel {
            title: title.trim().to_string(),
            body,
            rows,
            body_focus,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Screen {
    /// Nothing on the stack (the moment before the first screen).
    Blank,
    /// A drive is on the stack. `overlay` is whatever screen sits over it
    /// (the pause menu, a stop, Tab's status pages); `None` when the player
    /// is at the wheel.
    Drive {
        view: Box<DriveView>,
        overlay: Option<Panel>,
    },
    /// Any other screen, over the title backdrop.
    Panel(Panel),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub screen: Screen,
    /// An achievement just earned, shown as a banner while it lasts.
    pub achievement: Option<String>,
}

impl Scene {
    pub fn capture(ctx: &GameContext) -> Scene {
        let achievement =
            (!ctx.achievement_notice.is_empty()).then(|| ctx.achievement_notice.clone());
        let states = ctx.states();
        let Some(top) = states.last() else {
            return Scene {
                screen: Screen::Blank,
                achievement,
            };
        };
        // The drive, wherever it is on the stack (screens pushed over a
        // drive keep it underneath; the arrival screens replace it).
        let drive_index = states
            .iter()
            .rposition(|s| s.try_borrow().is_ok_and(|s| as_drive(&*s).is_some()));
        let top_lines = top.try_borrow().map(|s| s.lines(ctx)).unwrap_or_default();
        let screen = match drive_index {
            Some(i) => {
                let view = states[i]
                    .try_borrow()
                    .ok()
                    .and_then(|s| as_drive(&*s).map(|d| DriveView::capture(d, ctx)));
                match view {
                    Some(view) => Screen::Drive {
                        view: Box::new(view),
                        overlay: (i + 1 < states.len())
                            .then(|| Panel::from_lines(&top_lines))
                            .flatten(),
                    },
                    None => panel_or_blank(&top_lines),
                }
            }
            None => panel_or_blank(&top_lines),
        };
        Scene {
            screen,
            achievement,
        }
    }

    /// Whether the world on screen moves by itself this frame: the road at
    /// the wheel, while anything is moving or falling from the sky. A menu
    /// over the drive freezes it (the drive is paused or waiting).
    pub fn animates(&self) -> bool {
        match &self.screen {
            Screen::Drive {
                view,
                overlay: None,
            } => {
                view.hud.speed_mph.abs() > 0.05
                    || view.weather.precipitating()
                    || view.lights_flash()
            }
            _ => false,
        }
    }
}

fn panel_or_blank(lines: &[String]) -> Screen {
    match Panel::from_lines(lines) {
        Some(panel) => Screen::Panel(panel),
        None => Screen::Blank,
    }
}

/// The drive inside a screen: the drive itself, or the one the driving
/// school wraps.
fn as_drive(state: &dyn State) -> Option<&DrivingState> {
    let any = state.as_any();
    if let Some(drive) = any.downcast_ref::<DrivingState>() {
        return Some(drive);
    }
    any.downcast_ref::<SchoolDrivingState>()
        .map(SchoolDrivingState::drive)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &[&str]) -> Vec<String> {
        text.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_menu_reads_as_title_rows_and_focus() {
        let panel = Panel::from_lines(&lines(&[
            "Freight Fate 1.9.0",
            "",
            "> New career",
            "  Achievements",
            "  Quit",
        ]))
        .unwrap();
        assert_eq!(panel.title, "Freight Fate 1.9.0");
        assert!(panel.body.is_empty());
        let (rows, focus) = panel.rows.unwrap();
        assert_eq!(rows, vec!["New career", "Achievements", "Quit"]);
        assert_eq!(focus, 0);
    }

    #[test]
    fn a_summary_above_the_rows_becomes_the_body() {
        let panel = Panel::from_lines(&lines(&[
            "Delivered",
            "",
            "Paid 1,240 dollars.",
            "On time.",
            "",
            "> Continue",
        ]))
        .unwrap();
        assert_eq!(panel.body, vec!["Paid 1,240 dollars.", "On time."]);
        assert_eq!(panel.rows.unwrap().0, vec!["Continue"]);
    }

    #[test]
    fn a_reader_keeps_its_lines_and_marks_the_current_one() {
        let panel = Panel::from_lines(&lines(&[
            "How to play - Driving (2/9)",
            "",
            "  Up arrow is the throttle.",
            "> Down arrow is the brake.",
            "Then a closing line.",
        ]))
        .unwrap();
        assert!(panel.rows.is_none());
        assert_eq!(panel.body_focus, Some(1));
        assert_eq!(panel.body[1], "Down arrow is the brake.");
        assert_eq!(panel.body.len(), 3);
    }

    #[test]
    fn no_lines_is_no_panel() {
        assert!(Panel::from_lines(&[]).is_none());
    }
}
