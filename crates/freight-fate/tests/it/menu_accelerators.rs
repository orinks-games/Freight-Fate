use freight_fate::app::{testing::TestApp, GameContext};
use freight_fate::states::base::{InputEvent, Menu, MenuCore, MenuItem};

struct Probe {
    menu: MenuCore<Self>,
    activated: Vec<usize>,
}
impl Menu for Probe {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }
    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }
    fn build_items(&mut self, _: &mut GameContext) -> Vec<MenuItem<Self>> {
        ["Alpha", "Another", "Beta"]
            .into_iter()
            .map(|label| MenuItem::new(label, |s: &mut Self, _| s.activated.push(s.menu.index)))
            .collect()
    }
}

#[test]
fn menu_accelerators_unique_duplicate_unmatched_and_repeat() {
    let mut app = TestApp::new();
    let mut probe = Probe {
        menu: MenuCore::new("Probe"),
        activated: vec![],
    };
    probe.refresh(&mut app.ctx, true);
    assert!(!app.ctx.settings.menu_accelerator_keys);
    probe.handle_event(&mut app.ctx, &InputEvent::typed('b'));
    assert_eq!(probe.menu.index, 2);
    assert!(probe.activated.is_empty());
    app.ctx.settings.menu_accelerator_keys = true;
    probe.handle_event(&mut app.ctx, &InputEvent::typed('b'));
    assert_eq!(probe.activated, [2]);
    probe.handle_event(&mut app.ctx, &InputEvent::typed('a'));
    assert_eq!(probe.menu.index, 0);
    probe.handle_event(&mut app.ctx, &InputEvent::typed('a'));
    assert_eq!(probe.menu.index, 1);
    probe.handle_event(&mut app.ctx, &InputEvent::typed('a'));
    assert_eq!(probe.menu.index, 0);
    probe.handle_event(&mut app.ctx, &InputEvent::typed('z'));
    assert_eq!(probe.menu.index, 0);
    let mut held = InputEvent::typed('b');
    if let InputEvent::KeyDown { repeat, .. } = &mut held {
        *repeat = true;
    }
    probe.handle_event(&mut app.ctx, &held);
    assert_eq!(probe.menu.index, 0);
    assert_eq!(probe.activated, [2]);
    probe.menu.items.clear();
    probe.first_letter_input(&mut app.ctx, "b", false);
    assert_eq!(probe.activated, [2]);
    app.shutdown();
}

#[test]
fn menu_accelerator_setting_persists_from_controls() {
    use ff_core::settings::Settings;
    use freight_fate::states::main_menu::SettingsCategoryState;
    let mut app = TestApp::new();
    let mut menu = SettingsCategoryState::new("controls");
    menu.refresh(&mut app.ctx, true);
    let index = menu
        .menu()
        .items
        .iter()
        .position(|item| {
            item.text(&menu, &app.ctx)
                .starts_with("Menu accelerator keys:")
        })
        .unwrap();
    menu.menu_mut().index = index;
    menu.activate(&mut app.ctx);
    assert!(app.ctx.settings.menu_accelerator_keys);
    assert!(Settings::load().menu_accelerator_keys);
    menu.adjust(&mut app.ctx, -1);
    assert!(!Settings::load().menu_accelerator_keys);
    app.shutdown();
}

#[test]
fn menu_accelerators_exit_confirmation_yes_and_no() {
    use freight_fate::states::main_menu::{ConfirmQuitState, MainMenuState};
    let mut app = TestApp::new();
    app.ctx.settings.menu_accelerator_keys = true;
    app.set_running(true);
    app.push_state(MainMenuState::new());
    app.push_state(ConfirmQuitState::new());
    app.dispatch_to_state(&InputEvent::typed('n'));
    assert!(app.running());
    assert!(app.state().unwrap().borrow().as_any().is::<MainMenuState>());
    app.push_state(ConfirmQuitState::new());
    app.dispatch_to_state(&InputEvent::typed('y'));
    assert!(!app.running());
    app.shutdown();
}
