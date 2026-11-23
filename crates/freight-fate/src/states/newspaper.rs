//! The Newspaper driver app: the issue orinks.net published, read a section
//! at a time (`NewspaperState`, then `NewspaperSectionState`).
//!
//! It opens from Driver apps at the wheel and from the Online menu, and
//! exists only while the game holds an issue (`crate::newspaper`). Menu
//! speech only: nothing about the paper is ever said at the wheel on its
//! own. The issue's text is the site's English, kept out of the
//! translation catalog (`Translator::keep_verbatim`) only while the app is
//! open: its headings ("Records", "Achievements") are also rows of the
//! game's own menus, which must translate again once the player leaves.

use ff_core::newspaper::{PaperIssue, PaperSection};

use crate::app::GameContext;
use crate::app::Say;
use crate::impl_state_for_menu;
use crate::states::base::{default_enter, end_sentence, Menu, MenuCore, MenuItem};

/// The row's label, in Driver apps and on the Online menu.
pub const NEWSPAPER_LABEL: &str = "Newspaper";
/// The row's help.
pub const NEWSPAPER_HELP: &str = "Driver news, records and carrier standings from orinks.net.";

const SECTIONS_HELP: &str = "Enter opens a section. Escape goes back.";
const SECTION_HELP: &str =
    "Up and down review the lines. Enter repeats a line. Escape returns to the sections.";

/// Where the sections menu goes back to, which names its back row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenedFrom {
    DriverApps,
    OnlineMenu,
}

/// The issue's sections, Since Monday first when the site printed it.
pub struct NewspaperState {
    menu: MenuCore<Self>,
    issue: PaperIssue,
    from: OpenedFrom,
    /// False until the first announcement: opening says the whole title,
    /// coming back from a section the masthead alone.
    announced: bool,
}

impl NewspaperState {
    /// The screen for the issue held now, or `None` when there is none (the
    /// row that opens it is not shown then). Asks the site again if the
    /// held issue is old; this opening reads what is held.
    pub fn open(ctx: &mut GameContext, from: OpenedFrom) -> Option<Self> {
        let issue = ctx.services.newspaper.issue()?;
        ctx.services.newspaper.refresh_if_stale();
        Some(Self {
            menu: MenuCore::new(&issue.title()).with_intro_help(SECTIONS_HELP),
            issue,
            from,
            announced: false,
        })
    }

    fn open_section(&mut self, ctx: &mut GameContext, index: usize) {
        if let Some(section) = self.issue.reading_order().get(index) {
            let state = NewspaperSectionState::new((*section).clone());
            ctx.push_state(state);
        }
    }
}

impl Menu for NewspaperState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = self
            .issue
            .reading_order()
            .iter()
            .enumerate()
            .map(|(index, section)| {
                MenuItem::new(section.heading.clone(), move |s: &mut Self, ctx| {
                    s.open_section(ctx, index)
                })
                .help(SECTIONS_HELP)
            })
            .collect();
        items.push(match self.from {
            OpenedFrom::DriverApps => {
                MenuItem::new("Back to Driver apps", |s: &mut Self, ctx| s.go_back(ctx))
                    .help("Return to the driver tablet app list.")
            }
            OpenedFrom::OnlineMenu => MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)),
        });
        items
    }

    /// Entered on opening and again on coming back from a section: the
    /// issue's text stays untranslated for as long as the app is open.
    fn enter(&mut self, ctx: &mut GameContext) {
        ctx.translator.keep_verbatim(self.issue.spoken_text());
        default_enter(self, ctx);
    }

    /// Every way out of the app pops this screen, so this is where the
    /// game's own menus get their translations back.
    fn exit(&mut self, ctx: &mut GameContext) {
        ctx.translator.keep_verbatim(Vec::new());
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let title = if self.announced {
            end_sentence(&self.issue.masthead)
        } else {
            end_sentence(&self.menu.title)
        };
        self.announced = true;
        ctx.say(&title);
        let current = self.current_text(ctx);
        ctx.say_with(current, Say::queued().review(false));
    }
}

impl_state_for_menu!(NewspaperState);

/// One section as a reviewable list of lines, the way the other driver
/// apps read.
pub struct NewspaperSectionState {
    menu: MenuCore<Self>,
    section: PaperSection,
}

impl NewspaperSectionState {
    pub fn new(section: PaperSection) -> Self {
        Self {
            menu: MenuCore::new(&section.heading).with_intro_help(SECTION_HELP),
            section,
        }
    }
}

impl Menu for NewspaperSectionState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = self
            .section
            .lines
            .iter()
            .map(|line| {
                let spoken = line.clone();
                MenuItem::new(line.clone(), move |_s: &mut Self, ctx: &mut GameContext| {
                    ctx.say(&spoken)
                })
                .help("Repeat this line.")
            })
            .collect();
        items.push(
            MenuItem::new("Back to sections", |s: &mut Self, ctx| s.go_back(ctx))
                .help("Return to the sections."),
        );
        items
    }
}

impl_state_for_menu!(NewspaperSectionState);
