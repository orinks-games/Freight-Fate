//! The windowed shell boots under SDL's dummy video driver -- every
//! headless run (CI, `--agent-server` under `FREIGHT_FATE_NO_SPEECH`, the
//! playtest benches) goes through it. The native window handle taken for
//! the nonblocking Windows shutdown does not exist there, and the sdl2
//! crate panics rather than errs when asked for it, which killed every
//! headless windowed boot the day the handle arrived (2026-09-01).

#[cfg(feature = "visuals")]
use ff_core::message_log::{MessageCategory, MessageLog};
use freight_fate::app::sdl_shell::SdlShell;
#[cfg(feature = "visuals")]
use freight_fate::visuals::{Panel, Scene, Screen};

#[cfg(feature = "visuals")]
#[test]
fn the_shell_boots_on_the_dummy_video_driver() {
    std::env::set_var("SDL_VIDEODRIVER", "dummy");
    std::env::set_var("SDL_AUDIODRIVER", "dummy");
    let mut shell =
        SdlShell::new("Freight Fate headless").expect("the dummy driver hosts a window");
    assert_eq!(shell.video.current_video_driver(), "dummy");
    shell.render_plain();
    // The visual display draws through the same window: a menu with a
    // caption, twice (the second is the frame it skips as unchanged).
    let mut log = MessageLog::new();
    log.add("New career. 1 of 9.", MessageCategory::General);
    let panel = Panel::from_lines(&[
        "Freight Fate".to_string(),
        String::new(),
        "> New career".to_string(),
        "  Quit".to_string(),
    ])
    .expect("a menu");
    let english = std::sync::Arc::new(ff_core::l10n::Translator::english());
    for _ in 0..2 {
        assert!(shell.can_be_seen());
        shell.render_scene(
            Scene {
                screen: Screen::Panel(panel.clone()),
                achievement: None,
            },
            &log,
            &english,
        );
    }
    shell.render_plain();
}

/// A build without the visual display boots the same window and draws only
/// the plain fill.
#[cfg(not(feature = "visuals"))]
#[test]
fn the_plain_shell_boots_on_the_dummy_video_driver() {
    std::env::set_var("SDL_VIDEODRIVER", "dummy");
    std::env::set_var("SDL_AUDIODRIVER", "dummy");
    let mut shell =
        SdlShell::new("Freight Fate headless").expect("the dummy driver hosts a window");
    assert_eq!(shell.video.current_video_driver(), "dummy");
    shell.render_plain();
    shell.render_plain();
}
