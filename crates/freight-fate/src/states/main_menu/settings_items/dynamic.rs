//! Settings categories whose rows depend on player choices.
use super::*;

impl SettingsCategoryState {
    pub(super) fn update_items(&self, ctx: &GameContext) -> Vec<Row> {
        let mut items = vec![
            row(
            dyn_label(|s| {
                format!(
                    "Update channel: {}",
                    match update_channel(s).as_str() {
                        "alpha" => "alpha",
                        "beta" => "beta",
                        _ => "stable releases",
                    }
                )
            }),
            adjust(|s, ctx, d| s.toggle_update_channel(ctx, d)),
            "Stable releases, beta testing, or early alpha builds.",
            ),
            row(
                dyn_label(|s| {
                    format!(
                        "Use GitHub mirror for updates: {}",
                        on_off(s.update_mirror_enabled)
                    )
                }),
                adjust(|s, ctx, d| s.toggle_update_mirror(ctx, d)),
                "Checks for and downloads updates through a GitHub mirror. Turn it on, then enter the mirror's address in GitHub mirror prefix, just below.",
            ),
        ];
        if ctx.settings.update_mirror_enabled {
            items.push(
                MenuItem::new(
                    dyn_label(|s| {
                        if s.update_mirror_prefix.is_empty() {
                            "GitHub mirror prefix: not set".to_string()
                        } else {
                            format!("GitHub mirror prefix: {}", s.update_mirror_prefix)
                        }
                    }),
                    |_s: &mut SettingsCategoryState, ctx| {
                        let prefix = ctx.settings.update_mirror_prefix.clone();
                        ctx.push_state(SettingsCategoryState::update_mirror_entry(
                            &prefix,
                        ))
                    },
                )
                .help(
                    "The mirror's address, put in front of every GitHub link. Use a mirror you trust: updates download through it.",
                ),
            );
        }
        items.push(
            MenuItem::new(
                "Check for updates",
                |_s: &mut SettingsCategoryState, ctx| ctx.push_state(UpdateCheckState::new()),
            )
            .help("Look for a new version now."),
        );
        items.push(back_row());
        items
    }

    pub(super) fn assistance_items(&self) -> Vec<Row> {
        let mut items = vec![row(
            dyn_label(|s| format!("Driving assistance preset: {}", assist_preset_label(s))),
            adjust(|s, ctx, d| s.cycle_assist_preset(ctx, d)),
            "Realistic is modern truck safety support. Balanced adds partial lane keeping, a firmer hand on descents, and stopping at your destination. All assists turns on every assist and sets lane keeping to full: the truck holds the lane, a tap changes lanes, and your destination exit is taken for you. Changing one assist makes this Custom. You still choose routes and handle yards and docks. Presets never change pacing, hours rules, transmission, weather, or hazards.",
        )];
        for (field, label, help_text) in DRIVING_ASSIST_SPECS {
            items.push(row(
                dyn_label(move |s| format!("{label}: {}", assist_value_text(s, field))),
                adjust(move |s, ctx, d| s.toggle_driving_assist(ctx, field, d)),
                help_text,
            ));
        }
        items.push(row_dyn_help(
            dyn_label(|s| format!("Lane keeping: {}", lane_keeping_label(s))),
            adjust(|s, ctx, d| s.cycle_lane_keeping(ctx, d)),
            |s| {
                "How much of the lane-holding work the truck does. Full \
                 holds the lane, turns {{steer_left}} and {{steer_right}} into tap lane \
                 changes, moves out of a lane a work zone closes, and takes \
                 your exits and open weigh stations, including the destination \
                 exit, without a signal. Partial steers the truck through \
                 the road's bends and drifts gently, with generous steering \
                 help; lane changes and speed are yours. Off drifts like a \
                 real wheel; bends are yours unless curve assistance is \
                 on, and every exit needs its signal and its exit lane. On \
                 partial or off {lean}. The road sound only says where you \
                 sit in your lane, and the road edge answers: a stutter \
                 clipping the rumble strip, a buzz fully on it, gravel off \
                 the pavement. Steering guide and Lane guide sound, under \
                 Audio, change which way to steer and what leans. \
                 Realistic sets this off, Balanced partial, All assists \
                 full."
                    .replace("{lean}", &steering_lean_text(s))
            },
        ));
        items.push(row(
            dyn_label(|s| format!("Following gap: {}", acc_gap_label(s))),
            adjust(|s, ctx, d| s.cycle_acc_gap(ctx, d)),
            "How much room adaptive cruise leaves to the vehicle \
             ahead. Close is two and a half seconds, normal three, far \
             three and a half. Weather widens the gap whichever you \
             pick. All three stay clear of a following-too-close \
             citation. The preset above never changes it.",
        ));
        // Lane and edge cue volume moved to Audio, next to the Gameplay
        // cues volume it scales. It is a volume, and a second volume
        // control hiding in the assists list is how it came to be a row
        // nobody could explain.
        items.push(back_row());
        items
    }
}
