//! The spoken manual: `HELP_PAGES` and the page-by-page, line-by-line
//! [`HelpState`] reader (port of `freight_fate/states/main_menu_help.py`).

use crate::app::GameContext;
use crate::bindings::Action;
use crate::states::base::{InputEvent, Key, State};
use crate::states::main_menu::ShortcutDevice;

/// Control names in the page text are `{{id}}` placeholders (`{{engine}}`,
/// `{{pad:fuel}}`, `{{key:horn}}`) resolved at render time against the
/// player's own shortcuts. A bare id follows the device in use: the pad
/// button when a controller is active and the control has one, else the
/// keyboard key. `pad:` and `key:` pin a device (the Controller page
/// describes the pad whatever you are holding). See
/// [`crate::bindings`] for the ids.
pub static HELP_PAGES: &[(&str, &[&str])] = &[
    (
        "Main menu",
        &[
            "Welcome to Freight Fate. Start a trucking career, learn the road sounds, and choose the work you want to haul.",
            "Choose New career to create a driver. Before starting, you can explore the help, sounds, and settings.",
            "How to play is this guide. Learn game sounds lets you hear a road cue and learn what it means.",
            "Settings lets you change driving assistance, sound, speech, and controls. Report a problem opens the bug-report page in your browser. Quit closes the game.",
            "Achievements lets you review a saved driver's earned and locked achievements. With no saved careers, there is no record to review yet.",
            "Online brings together driver profiles, account achievements, sharing, and private cloud backups.",
            "Once you have a saved career, Continue latest career returns to your newest saved trip. Choose career lets you select another driver.",
            "Manage careers offers reset and deletion. The game asks you to confirm before either happens.",
        ],
    ),
    (
        "Your first delivery",
        &[
            "Choose New career, enter a name, then choose your start and home terminal.",
            "A company driver uses carrier equipment. An owner-operator owns the truck and pays its operating costs. Both begin at level one.",
            "Read the first-day briefing at the terminal, then open dispatch and accept a load.",
            "You first drive to the shipper without cargo. Stop at the gate, check in, and load or hook the trailer.",
            "Company drivers follow dispatch's route. Owner-operators choose a route after loading.",
            "Deliver before the deadline and keep the cargo undamaged. At the receiver, stop and choose Dock and deliver.",
            "Read settlement, then continue from the destination terminal. Career plan explains the next step as you gain experience.",
            "For a first drive, All assists handles steering while you learn the sounds. Change it under Settings, Gameplay, Driving assistance.",
        ],
    ),
    (
        "Menus and message review",
        &[
            "Up and Down choose an option. Enter or Space selects it; Escape returns. Home and End jump to the first or last option.",
            "Type a letter to find an option starting with it. F1 explains the selected option.",
            "Comma repeats the latest message, then steps backward. Period moves forward. Control with either jumps to the oldest or newest message.",
            "The brackets choose all, general, or driving messages. Control C copies the selected message. Review keys type normally inside text fields.",
            "In How to play, Left and Right or the Page keys change pages. Up and Down let you read a little at a time. Enter or Space reads the page; either Control key or F1 stops speech.",
            "Learn game sounds plays road cues and explains their meaning. It is available on the main and pause menus.",
        ],
    ),
    (
        "Starting and stopping the truck",
        &[
            "{{engine}} starts the engine. To shut it down, slow below five miles per hour.",
            "Wait for air pressure to reach 100 psi, then use {{parking_brake}} to release the parking brake. Repeated hard braking uses air.",
            "Hold {{accelerate}} to accelerate and {{brake}} to brake. Hold {{emergency_brake}} for the hardest stop.",
            "In automatic, stop fully, release {{brake}}, then press and hold it again to reverse. A quick tap or a brake held through the stop keeps the truck stopped.",
            "While reversing, stop with {{accelerate}}, release it, then press and hold again for forward. Both direction-change styles use this gesture.",
            "With Latching brake on, tap the brake, then press and hold again for half a second. A click and announcement confirm the latch.",
            "Press the brake again or accelerate to release the latch. The throttle never latches.",
            "Set the parking brake when stopped. Applying it at speed causes a hard stop and tire damage. Parked with it set, {{cruise}} holds high idle.",
            "For manual clutch, hold either Shift key on a keyboard or left bumper on a controller. Then {{shift_up}} shifts up, {{shift_down}} down, {{neutral}} selects neutral, and {{reverse}} selects reverse.",
            "{{transmission_mode}} switches automatic and manual shifting. {{horn}} sounds the horn while held.",
        ],
    ),
    (
        "Speed control and hills",
        &[
            "{{cruise}} starts automatic speed control: adaptive cruise on open roads, speed keeper on supported low-speed roads.",
            "Cruise follows traffic. Its normal three second clear-weather gap increases in poor weather. It does not steer.",
            "Plus and Minus, including keypad keys, move the open-road target to the next multiple of five. Hold Control to change it by one.",
            "Cruise prepares for sharp posted-limit drops and holds no more than five over the posted limit. {{speed}} reads the active speed-control mode and open-road target.",
            "Speed keeper follows local limits, queues, and corners, then hands over to cruise on open road. The target keys still change the remembered cruise speed.",
            "Press {{cruise}} again or touch the brakes to cancel. {{cruise_resume}} resumes the previous target. Pickup pauses the session through loading and resumes it after departure.",
            "Before a descent, reduce speed and choose a lower gear. Use the engine brake for steady control and short brake applications with cooling time between them.",
            "{{engine_brake}} enables the engine brake. While enabled, {{jake_stage_1}}, {{jake_stage_2}}, and {{jake_stage_3}} choose its stage. {{auto_jake}} selects automatic or manual stage management.",
            "Hot brakes lose stopping power. {{safe_speed}} gives a safe speed for the load, grade, weather, and bend. Strong engine braking can break traction on ice.",
            "Noise-restricted towns warn before fining engine braking. Downgrades and emergencies have exceptions.",
        ],
    ),
    (
        "Driving information keys",
        &[
            "{{speed}} speaks your speed, gear, RPM, active speed-control mode, open-road target, air pressure, and brake state. With an exit signal set, it adds distance to that exit.",
            "{{speed_limit}} speaks the posted speed limit, zone, and how far over you are. {{safe_speed}} gives the current safe speed; {{grade}} reads the current and next relevant grade.",
            "{{fuel}} reads fuel and range. {{clock}} reads the clock, deadline, and nearest hours limit.",
            "{{hos_wheel}} speaks time at the wheel and on duty. {{hos_break}} reads when the break is due. {{hos_drive}} reads what ends the shift and reachable legal rest.",
            "{{route}} speaks how far along you are, distance remaining, and your location. A planned stop changes the distance to that stop.",
            "{{place_state}} reads the state, {{place_road}} the road, {{place_town}} the town or nearest town, and {{place_direction}} the direction. Keypad equivalents work too.",
            "{{weather}} reads weather. {{lane}} reads lane position and whether the lane beside you is open.",
            "{{take_exit}} signals for an announced exit or cancels the signal. {{rest}} plans rest while moving or opens a nearby stop when fully stopped.",
            "{{upcoming}} speaks the road ahead that no other key answers: your signaled exit, ramp control, next lower limit, stop, and bend requiring slowing.",
            "{{last_announcement}} repeats the last driving announcement. {{cb}} repeats the last CB chatter with its current distance.",
            "{{status}} opens Route, Driver, Map, and Radio reports, plus Driver apps. Use the reports for trip details or open an app for a specific task.",
            "Left or Right Control stops the driving event voice. Escape pauses. F2 lists driving commands by name; Enter runs the selected command.",
            "Change eligible shortcuts under Settings, Gameplay, Controls, then Keyboard shortcuts or Controller buttons. These pages follow your assignments.",
        ],
    ),
    (
        "Steering and curves",
        &[
            "Full lane keeping centers the truck, follows bends, and takes route exits including the destination. Tap {{steer_left}} or {{steer_right}} to change lanes.",
            "Partial follows bends with gentle drift. Off leaves steering to you unless curve assistance is enabled. Hold a steering control across the lane line to change lanes.",
            "With the default steering guide, steer toward the engine's lean. Road noise tells you position: sound to the right means the truck sits right of center.",
            "With lane keeping off and curve assistance off, hold toward a called bend to follow the road. Releasing the control squares the truck with the road.",
            "Hold {{straighten}} to straighten while another control stays held. Your position within the lane remains yours to correct.",
            "The curve call gives direction, distance, and advisory speed. Brake to that speed before the bend. A tone marks the bend's side.",
            "Rumble-strip and gravel sounds come from the edge you reached. Correct gently. Crossing an undivided centerline puts you in opposing traffic.",
            "{{lane_locator}} toggles a continuous lane-position tock on partial or off. Steering guide can reverse the convention; Lane guide sound can use a separate soft tone.",
            "Curve assistance slows and steers through mapped bends. Your brake releases its slowing for that bend, while steering assistance remains.",
        ],
    ),
    (
        "Exits, ramps, and arrival",
        &[
            "{{rest}} plans a nearby sleep-capable stop while rolling. With hours-planning hints on, it plans the recommended break or sleep stop. Press again to cancel the plan.",
            "{{take_exit}} separately signals for the announced exit or cancels the signal. Planning a stop does not signal for it.",
            "Unless lane keeping is on full, move to the right lane, then steer right into the exit lane when it opens.",
            "Keep road speed until the exit lane. Brake to the announced exit speed before its curve. Full lane keeping makes the lane moves for you.",
            "Most ramps end at a light or stop sign. Stop at red; stop safely for yellow if possible. At a stop sign, wait for a clear gap after stopping.",
            "Every light change is spoken. The sensor tick speeds up near the stop bar. Cross traffic can hit a truck that runs through.",
            "When clear, pull ahead as instructed. Facility stopping assistance can take the pedals to the gate; your brake releases it.",
            "Follow the local street and yard guidance, stop at the gate, then confirm arrival. At the receiver choose Dock and deliver.",
            "Miss the destination exit and follow the next safe turnaround. Ordinary exits without a current action stay quiet.",
            "Fully stopped at a route stop, {{rest}} opens its services. Fully stopped elsewhere, {{rest}} opens the emergency shoulder-sleep warning; nearby route points always take priority.",
        ],
    ),
    (
        "Traffic, weather, and enforcement",
        &[
            "GPS announces state lines, towns, route changes, traffic, work zones, and useful stops. Grades and terrain come from the route; weather and traffic vary by place and time.",
            "Nearby vehicles merge, slow, pass, and queue. {{lane}} tells you which neighboring lanes are available. Check before changing lanes or returning after a pass.",
            "A passing vehicle grows louder alongside you, then fades ahead. Listen for which side it is on, but use the lane check before moving over.",
            "Mainline traffic stays to your left on an on-ramp. The distant freeway sound follows traffic activity, so busy and quiet stretches sound different.",
            "Brake now means slow promptly. Change lanes or brake names an available lane. A fixed object needs a near-stop if you cannot change lanes.",
            "No lane open means brake. Full lane keeping can pass slow vehicles, but leaves debris avoidance to you and avoids starting a pass near an exit.",
            "Leave a coned lane when warned. Driving through barrels causes damage, a citation, and a serious violation.",
            "Weather changes grip, visibility, safe speed, following distance, and fuel use. {{weather}} reports conditions. Snow and ice need gentler braking and steering.",
            "The calendar starts in spring and advances with driving and rest. Live weather can follow the real date or an independent career calendar.",
            "When a trooper signals you, use {{take_exit}} and brake to a stop for a license and logbook check. Ignoring the lights escalates to a forced stop or pursuit.",
            "CB chatter reports what other drivers saw, with uncertainty. It can be stale and never claims the road is clear. {{cb}} repeats it with updated distance.",
            "Citations cost money and affect your record. Serious violations can suspend your CDL and remove driving work; fleeing is a major offense.",
            "The dash overspeed chime is a free warning. Tickets already paid on the road are not charged again at settlement.",
            "At an open scale, slow into its lane, signal with {{take_exit}}, stop, then use {{rest}} for inspection check-in.",
        ],
    ),
    (
        "Hours, fatigue, and rest stops",
        &[
            "The hours clock permits eleven driving hours after ten hours off, inside a fourteen-hour duty window. A thirty-minute break is due after eight cumulative driving hours.",
            "Any thirty consecutive non-driving minutes satisfy that break, including loading or inspection. They do not necessarily reset the duty window.",
            "Both hours modes use these limits. Relaxed changes enforcement pressure. Warnings come at two hours, one hour, and thirty minutes left.",
            "{{hos_wheel}} reads time used, {{hos_break}} the break, and {{hos_drive}} the shift limits and reachable rest. Check again as traffic or weather changes.",
            "Ten-hour sleep is the simplest reset. Supported sleeper parking offers two, three, seven, or eight hours for an eight-plus-two or seven-plus-three split.",
            "The long split pauses the duty window. The short half counts until the pair completes. A nap that leaves the window running tells you when it closes.",
            "Fatigue rises faster at night and reduces reaction time. Respond to nod-off warnings by steering or braking. Repeated misses can force you off the road.",
            "Meals and coffee help temporarily. A half-hour meal counts as a break. Proper sleep clears fatigue; a legal reset with poor rest can leave you tired.",
            "A stop offers only its listed services. A full parking lot can still sell diesel. A motel may offer full rest at your expense.",
            "Sleep 10 hours in the lot is poor rest. Sleeper parking gives fully-rested ten-hour sleep.",
            "Away from route points and fully stopped, {{rest}} or the pause menu offers emergency shoulder sleep: ten hours pass, with poor rest and possible parking ticket or minor damage.",
            "The deadline keeps running during all rest. Logbook at the terminal and the ELD app let you review duty and limits.",
        ],
    ),
    (
        "Career, equipment, and money",
        &[
            "New company hires get an assigned load. Load choice opens at level eight. Declining costs trust; low trust can remove load choice again.",
            "Jobs name cargo, weight, pay, deadline, facilities, and equipment requirements. F1 reads the details. Fuel adds weight; check before filling a marginal load.",
            "Walk around a dropped trailer before leaving. A defect can be refused at the yard or found later at inspection.",
            "Company drivers receive wages and bonuses. Owners receive higher gross with fuel, repair, reserves, trailer charges, and fees to pay.",
            "Settlement separates gross pay, carrier-paid or reimbursed charges, driver costs, and net driver pay. On-time and undamaged freight pays better.",
            "Unpaid charges become a balance owed. Later settlements use one quarter for debt and advances together, leaving three quarters as take-home.",
            "Debt warnings name the ceiling and consequence. At the ceiling, a company driver changes carrier or an owner loses tractors and returns to company driving.",
            "You keep career level, experience, endorsements, and record. Career stats reads debt and trust; Business status explains ownership requirements.",
            "The garage refuels, repairs, replaces tires, and services brakes and engines. Company routine costs use the carrier account; owners can buy partial service when cash is short.",
            "At eighty percent component wear, arrange service. At one hundred, that component puts the truck out of service. Damage has separate reduced-power, limp-mode, and out-of-service bands.",
            "Owners buy tractors, upgrades, and trailer programs. Each truck keeps its condition; upgrades apply across the fleet.",
            "Licenses and training opens refrigerated, heavy-haul, high-value, liquid bulk, doubles, hazardous, port, and long-combination freight as requirements clear.",
        ],
    ),
    (
        "The garage and freight markets",
        &[
            "Each metro represents a wider freight area with many shippers: rail and intermodal ramps, parcel hubs, farms, grain elevators, chemical terminals, and other facilities.",
            "Facilities determine local cargo. Dispatch routes include stops for fuel and legal rest; F1 reads the load's requirements before you accept it.",
            "Owners can add reefer, flatbed, and bulk programs. Company trailers cover the equipment requirements of approved loads.",
            "An engine tune adds pulling power. An aerodynamic kit reduces highway fuel burn. A long-range tank adds fifty gallons; reinforced brakes resist fade longer.",
            "Company equipment improves with seniority. Low trust can hold it back until your record and finances recover.",
            "Terminal garages offer repair, tires, brakes, engine service, and washing. Road stops offer only the services listed for that location.",
            "Damage above fifty percent reduces power; above seventy-five brings limp mode and a forty-five mile per hour cap. Above ninety puts the truck out of service.",
            "A roadside mechanic can help during a trip. Stop safely and use the pause menu or Call dispatch for help.",
        ],
    ),
    (
        "Winter equipment",
        &[
            "Winter tires improve snow and ice grip. Chains are carried until you install them while stopped through the pause menu.",
            "Level one chain law accepts winter tires or chains. Level two requires drive-axle chains. Signs announce the requirement before its checkpoint.",
            "Chain work uses on-duty time and adds fatigue, especially at night. Keep near thirty miles per hour and remove chains on bare pavement before they break.",
            "Worn tires and strong engine braking make slippery roads harder to control. Stop for rest or safer conditions when necessary.",
        ],
    ),
    (
        "Driver apps",
        &[
            "Open driving status with {{status}}, then choose Driver apps. Up and Down choose an app; Enter opens it. Escape returns.",
            "In a report, Up and Down move through the information and Enter repeats what you selected. Escape returns to Driver apps, then to driving status.",
            "Radio lets you switch the radio on or off, tune, manage favorites, and suggest a station. Search by part of a name, call sign, or format, then choose a result. Out-of-range stations cannot be tuned here.",
            "Navigation reviews your next guidance, progress, and next listed exit. Map under driving status gives a broader view of the route and stops.",
            "Weather gives conditions, suggested safe speed, and how old the report is. Simulated weather includes a forecast; a live report may not. The app tells you if a live check is updating or has failed.",
            "Traffic reports traffic ahead, including a reported vehicle's distance and speed. No reported pinch in the next twenty miles does not guarantee an empty road.",
            "Truck stops lists up to three route stops in the next hundred miles, with distance and services. Check for fuel, repairs, and parking before choosing a stop.",
            "Road chatter gives informal driver reports that may be stale. Enforcement reports are quiet in hours modes without enforcement. {{cb}} repeats the last chatter with its distance updated.",
            "ELD gives your hours and guidance about reachable legal rest. Check again if traffic or weather slows your trip.",
        ],
    ),
    (
        "Radio and playlists",
        &[
            "With the engine running, {{radio}} switches the radio on or off. Page Down or apostrophe tunes forward; Page Up or semicolon tunes backward.",
            "Hold Control with a tune key to jump between categories, including route music, Freight Fate stations, your playlists, favorites, and broadcast or web stations.",
            "Hold Shift to change volume in ten percent steps: Page Up or semicolon raises it, Page Down or apostrophe lowers it.",
            "{{radio_favorite}} saves or removes a favorite. {{radio_status}} reads station, reception, volume, and streamer-safe status. {{radio_now_playing}} reads available song information.",
            "Radio status lists receivable stations. Use the Radio app to choose or search for a station and browse favorites.",
            "Regional stations fade as you leave their range. If reception fails, the radio tries a clear local station, then a fallback. Failed streams leave the dial for that session. Roadhouse and Night Line are available across the map.",
            "Put M3U, M3U8, or PLS playlists in Playlists beside your saves. Radio status tells you the folder location and reloads it when opened. Entries can be audio files, network paths, or stream addresses.",
            "Personal playlists normally play in order and resume where you left them. Shuffle plays every track once in a random order, then chooses a new order. Unreadable entries are skipped.",
            "Original music uses licensed menu and Roadhouse tracks. Synthesized uses music the game composes and the original three tracks, removing fictional regional stations and voiced breaks. Real streams and personal playlists remain unless streamer-safe mode hides them.",
            "Streamer-safe mode keeps built-in safe stations. With Synthesized music, only the synthesized Roadhouse remains. Radio power, volume, and now playing still work; other station controls do nothing.",
            "Suggest a station in the Radio app asks for its type, name, and stream address. AM and FM suggestions also need a call sign and state. City and frequency are optional. Connect an account and enable Online services first.",
            "Suggestions are checked for playback and duplicates before review. Accepted stations can appear at a later launch without a game update.",
        ],
    ),
    (
        "Settings and accessibility",
        &[
            "Settings save as you change them. Choose a setting with Up or Down, then Right or Enter for its next value, or Left for the previous value.",
            "Gameplay holds driving assistance, difficulty and hours, world and traffic, and controls. Audio, Speech, Updates, Problem reports, and Online have separate choices where supported. F1 explains the selected setting.",
            "Realistic leaves lane keeping off. Balanced adds partial lane keeping and facility stops. All assists adds full lane keeping and a fifty-five mile per hour descent ceiling. Adjusting assistance yourself changes the preset name to Custom.",
            "Presets leave driving pace, legal hours, transmission, world sources, latching brake, and cue volumes as they are.",
            "Relaxed pacing gives more response time and gentler consequences. Standard moves driving time and distance twice as fast. Real time follows the computer clock at real speed.",
            "Change pacing mid-drive from pause; it begins when the truck next stops. Legal hours and time remaining on a deadline stay intact.",
            "Choose simulated or available live weather, traffic, and parking separately from online sharing. Fuel prices can use the weekly national diesel average with regional differences or simulated regional prices.",
            "Adjust music, radio, engine, weather, and gameplay cues separately if one covers another. Game sounds step back for speech lowers engine, weather, and radio to half volume while the road voice speaks.",
            "Driving speech chooses Standard, Quiet, or Urgent only announcements. Readout keys still answer when asked. Place names and roadside chatter have separate controls.",
            "Speech and braille sends messages to both. Braille only requires NVDA or JAWS; speech returns if the screen reader closes. You can also choose a separate driving event voice.",
            "For a problem report, use Where the game log is saved to find the session record, including spoken messages. It stays on your computer unless you share it.",
        ],
    ),
    (
        "Online sharing and backups",
        &[
            "Online lets you browse driver profiles and account achievements, connect an account, choose sharing, and manage private backups. Viewing drivers shares nothing about you.",
            "Drivers on duty refreshes about once a minute. Enter opens a driver's public profile. Driver directory includes drivers who are not currently playing; Check again refreshes it.",
            "Use an activation code in your browser to connect an existing account and enable profile sharing and backup. You can turn either off afterward.",
            "Profile sharing includes your activity, achievements, and road journal. Public profiles omit current cash, full saves, coordinates, and precise location. Career statistics need an accepted private backup.",
            "Cloud backup keeps ten accepted revisions per career and up to ten careers. Restore checks the signed backup and keeps the replaced local save as a fallback. Back up this career now uploads immediately.",
            "Mastodon can share deliveries that earn an achievement, level, or perfect streak after you link an account. Discord shows broad activity and clears it after half an hour idle.",
            "Online services switches sharing services off together while remembering your choices. Live weather, traffic, and parking keep their separate settings.",
        ],
    ),
    (
        "Controller",
        &[
            "Controllers work alongside the keyboard. Button names use the Xbox layout. Settings, Gameplay, Controls lets you change eligible assignments.",
            "In menus, D-pad moves, A confirms, B returns, and Back reads help. During driving speech, Back stops the voice; otherwise it reads help. Start pauses.",
            "Right trigger accelerates, left trigger brakes, and fully pressing the left trigger gives emergency braking. The left stick steers.",
            "To reverse in automatic, stop, release the left trigger, then press and hold it again. Use a fresh right-trigger hold at a standstill for forward.",
            "Hold left bumper for manual clutch. {{pad:shift_up}} shifts up and {{pad:shift_down}} down. {{pad:cruise}} starts speed control; {{pad:speed}} reads speed.",
            "{{pad:horn}} sounds the horn; {{pad:engine_brake}} enables the engine brake. {{pad:route}} reads location, {{pad:weather}} weather, and {{pad:clock}} clock and hours.",
            "Right bumper accesses the second layer. {{pad:engine}} controls the engine, {{pad:fuel}} reads fuel, and {{pad:speed_limit}} reads the limit.",
            "{{pad:parking_brake}} controls the parking brake. {{pad:take_exit}} signals for an exit; {{pad:rest}} opens route-stop actions or emergency shoulder sleep when fully stopped.",
            "{{pad:cruise_down}} and {{pad:cruise_up}} change the cruise target. {{pad:status}} opens driving status.",
        ],
    ),

];

/// One line of the manual with its control names filled in for this
/// player and device. A placeholder that names nothing the table knows is
/// left as written, so a typo reads aloud in a test instead of vanishing.
pub fn render_help_line(ctx: &GameContext, template: &str) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        let Some(len) = rest[start..].find("}}") else {
            break;
        };
        out.push_str(&rest[..start]);
        let token = &rest[start + 2..start + len];
        let (device, id) = match token.split_once(':') {
            Some(("pad", id)) => (Some(ShortcutDevice::Controller), id),
            Some(("key", id)) => (Some(ShortcutDevice::Keyboard), id),
            _ => (None, token),
        };
        let name = match Action::from_id(id) {
            Some(action) => match device {
                Some(ShortcutDevice::Controller) => ctx.bindings.pad_spoken(action),
                Some(ShortcutDevice::Keyboard) => ctx.bindings.spoken(action),
                None => ctx.control_name(action),
            },
            None => format!("{{{{{token}}}}}"),
        };
        // A name opening the line opens a sentence: "the A button" reads
        // "The A button" there and nowhere else.
        if out.is_empty() {
            let mut chars = name.chars();
            if let Some(first) = chars.next() {
                out.extend(first.to_uppercase());
                out.push_str(chars.as_str());
            }
        } else {
            out.push_str(&name);
        }
        rest = &rest[start + len + 2..];
    }
    out.push_str(rest);
    out
}

/// One page, rendered: its title and lines.
pub fn help_page(ctx: &GameContext, page: usize) -> (&'static str, Vec<String>) {
    let (title, lines) = HELP_PAGES[page.min(HELP_PAGES.len() - 1)];
    (
        title,
        lines
            .iter()
            .map(|line| render_help_line(ctx, line))
            .collect(),
    )
}

/// Every page, rendered.
pub fn help_pages(ctx: &GameContext) -> Vec<(&'static str, Vec<String>)> {
    (0..HELP_PAGES.len()).map(|i| help_page(ctx, i)).collect()
}

/// Index of the driving-keys page, so callers can open help straight to it.
pub fn controls_help_page() -> usize {
    HELP_PAGES
        .iter()
        .position(|(title, _lines)| *title == "Driving information keys")
        .unwrap_or(0)
}

/// Page-by-page, line-by-line spoken manual.
pub struct HelpState {
    pub page: usize,
    /// `-1` = page title.
    pub line: i64,
}

impl HelpState {
    /// `HelpState(ctx, start_page=0)`.
    pub fn new() -> Self {
        Self::at_page(0)
    }

    /// `HelpState(ctx, start_page=...)`: out-of-range requests clamp.
    pub fn at_page(start_page: usize) -> Self {
        Self {
            page: start_page.min(HELP_PAGES.len() - 1),
            line: -1,
        }
    }

    fn page_title(&self) -> String {
        let (title, _) = HELP_PAGES[self.page];
        format!("Page {} of {}: {title}.", self.page + 1, HELP_PAGES.len())
    }
}

impl Default for HelpState {
    fn default() -> Self {
        Self::new()
    }
}

impl State for HelpState {
    fn enter(&mut self, ctx: &mut GameContext) {
        let controls = match ctx.controller.device() {
            ff_core::input_hints::CONTROLLER => "D-pad Left and Right change pages, Up and Down let you read a little at a time. A reads the whole page, Back stops speech, and B goes back.",
            _ => "Left and Right arrows or Page Up and Page Down change pages. Up and Down let you read a little at a time. Enter or Space reads the whole page, either Control or F1 stops speech, and Escape goes back.",
        };
        ctx.say(&format!("How to play. {controls} {}", self.page_title()));
    }

    fn handle_event(&mut self, ctx: &mut GameContext, event: &InputEvent) {
        let Some((key, _, _)) = event.key_down() else {
            return;
        };
        let (title, lines) = help_page(ctx, self.page);
        match key {
            Key::Escape => {
                ctx.audio.play("ui/menu_back");
                ctx.pop_state();
            }
            Key::LCtrl | Key::RCtrl | Key::F1 => ctx.stop_speech(),
            Key::Right | Key::PageDown => {
                self.page = (self.page + 1) % HELP_PAGES.len();
                self.line = -1;
                ctx.audio.play("ui/menu_move");
                ctx.say(&self.page_title());
            }
            Key::Left | Key::PageUp => {
                self.page = (self.page + HELP_PAGES.len() - 1) % HELP_PAGES.len();
                self.line = -1;
                ctx.audio.play("ui/menu_move");
                ctx.say(&self.page_title());
            }
            Key::Down => {
                self.line = (self.line + 1).min(lines.len() as i64 - 1);
                ctx.say(&lines[self.line as usize]);
            }
            Key::Up => {
                self.line = (self.line - 1).max(0);
                ctx.say(&lines[self.line as usize]);
            }
            Key::Return | Key::KpEnter | Key::Space => {
                ctx.say(&format!("{title}. {}", lines.join(" ")));
            }
            _ => {}
        }
    }

    fn lines(&self, ctx: &GameContext) -> Vec<String> {
        let (title, lines) = help_page(ctx, self.page);
        let mut out = vec![
            format!(
                "How to play - {title} ({}/{})",
                self.page + 1,
                HELP_PAGES.len()
            ),
            String::new(),
        ];
        for (i, text) in lines.iter().enumerate() {
            let marker = if i as i64 == self.line { "> " } else { "  " };
            out.push(format!("{marker}{text}"));
        }
        out
    }
}
