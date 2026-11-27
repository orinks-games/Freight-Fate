//! How to play as a phone or tablet reads it.
//!
//! Two tables. [`PHONE_LINES`] swaps single desktop lines, with one version
//! for a phone with a hardware keyboard and one for touch. [`TOUCH_PAGES`]
//! replaces a whole page for a touch player where most of its lines change.
//! The desktop pages in [`super::main_menu_help::HELP_PAGES`] stay as they
//! are, byte for byte, because the tests and translations pin them.
//!
//! Touch lines name fixed gestures in words. A command that sits on a
//! gesture slot by default is a `{{do:id}}` placeholder, filled in with the
//! gesture that runs it as an instruction ("Tap", "Flick up with three
//! fingers"), so a moved slot reads right. A command only the driving
//! command list has is named by its row, which stays true whatever the
//! player moves: each page says once which group of the three-finger tap
//! list it is in.

/// Lines a phone or tablet build reads differently: `(desktop line, with a
/// keyboard, by touch)`, `None` leaving the line out. Phones have no
/// personal playlists (their Playlists folder is out of the player's reach),
/// no Quit row, no Updates category, no braille output and no log folder
/// the player can open, and a touch player tunes with three-finger flicks,
/// not the Page keys. A page in [`TOUCH_PAGES`] takes its touch lines from
/// there, so only the keyboard column of its entries here is read.
pub(super) static PHONE_LINES: &[(&str, Option<&str>, Option<&str>)] = &[
    (
        "Page Up and Page Down move ten options at a time.",
        Some("Flick left or right with three fingers to move ten options back or forward."),
        Some("Flick left or right with three fingers to move ten options back or forward."),
    ),
    (
        "Settings changes driving assistance, sound, speech, and controls. Report a problem opens the report form in your browser, with your game log attached. Quit closes the game.",
        Some("Settings changes driving assistance, sound, speech, and controls. Report a problem opens the report form in your browser, with your game log attached."),
        Some("Settings changes driving assistance, sound, speech, and controls. Report a problem opens the report form in your browser, with your game log attached."),
    ),
    (
        "Jobs name cargo, weight, pay, deadline, facilities, and equipment requirements. F1 reads the details. Fuel adds weight, so check the weight before filling up on a load near the limit.",
        Some("Jobs name cargo, weight, pay, deadline, facilities, and equipment requirements. F1 reads the details. Fuel adds weight, so check the weight before filling up on a load near the limit."),
        Some("Jobs name cargo, weight, pay, deadline, facilities, and equipment requirements. Flick up with two fingers on a job for the details. Fuel adds weight, so check the weight before filling up on a load near the limit."),
    ),
    (
        "Facilities determine local cargo. Dispatch routes include stops for fuel and legal rest. F1 reads the load's requirements before you accept it.",
        Some("Facilities determine local cargo. Dispatch routes include stops for fuel and legal rest. F1 reads the load's requirements before you accept it."),
        Some("Facilities determine local cargo. Dispatch routes include stops for fuel and legal rest. Flick up with two fingers on a load for its requirements before you accept it."),
    ),
    (
        "Partial follows bends with gentle drift. Off leaves steering to you unless curve assistance is enabled. Hold a steering control across the lane line to change lanes.",
        Some("Partial follows bends with gentle drift. Off leaves steering to you unless curve assistance is enabled. Hold a steering control across the lane line to change lanes. With Tilt steering on under Touch gestures, tilting the phone steers too, and with lane keeping on full a tilt changes one lane."),
        None,
    ),
    (
        "With the engine running, {{radio}} switches the radio on or off. Page Down or apostrophe tunes forward. Page Up or semicolon tunes backward.",
        Some("With the engine running, {{radio}} switches the radio on or off. Page Down or apostrophe tunes forward. Page Up or semicolon tunes backward."),
        Some("The radio plays while the engine runs. A three-finger flick right tunes forward, and a three-finger flick left tunes backward."),
    ),
    (
        "Hold Control with a tune key to jump between categories, including route music, Freight Fate stations, your playlists, favorites, and broadcast or web stations.",
        Some("Hold Control with a tune key to jump between categories, including route music, Freight Fate stations, favorites, and broadcast or web stations."),
        Some("Radio and CB on the three-finger tap list turns the radio on or off, saves favorites, and changes the radio volume and category. Categories include route music, Freight Fate stations, favorites, and broadcast or web stations."),
    ),
    (
        "Hold Shift to change volume in ten percent steps: Page Up or semicolon raises it, Page Down or apostrophe lowers it.",
        Some("Hold Shift to change volume in ten percent steps: Page Up or semicolon raises it, Page Down or apostrophe lowers it."),
        None,
    ),
    (
        "{{radio_favorite}} saves or removes a favorite. {{radio_status}} reads station, reception, volume, and streamer-safe status. {{radio_now_playing}} reads available song information.",
        Some("{{radio_favorite}} saves or removes a favorite. {{radio_status}} reads station, reception, volume, and streamer-safe status. {{radio_now_playing}} reads available song information."),
        Some("Radio status reads station, reception, volume, and streamer-safe status. What the radio is playing reads available song information. Both are under Radio and CB."),
    ),
    (
        "Put M3U, M3U8, or PLS playlists in the Playlists folder inside your saves folder. Radio status gives the folder location and reloads the playlists when opened. Entries can be audio files, network paths, or stream addresses.",
        None,
        None,
    ),
    (
        "Personal playlists normally play in order and resume where you left them. Shuffle plays every track once in a random order, then chooses a new order. Unreadable entries are skipped.",
        None,
        None,
    ),
    (
        "Original uses the licensed menu and Roadhouse tracks. Synthesized plays music the game composes plus the three original tracks, and takes Freight Fate's regional stations and voiced breaks off the dial. Real streams and personal playlists remain unless streamer-safe mode hides them.",
        Some("Original uses the licensed menu and Roadhouse tracks. Synthesized plays music the game composes plus the three original tracks, and takes Freight Fate's regional stations and voiced breaks off the dial. Real streams remain unless streamer-safe mode hides them."),
        Some("Original uses the licensed menu and Roadhouse tracks. Synthesized plays music the game composes plus the three original tracks, and takes Freight Fate's regional stations and voiced breaks off the dial. Real streams remain unless streamer-safe mode hides them."),
    ),
    (
        "Gameplay holds driving assistance, difficulty and hours, world and traffic, and controls. Audio, Speech, Updates, and Problem reports have separate choices where supported. F1 explains the selected setting.",
        Some("Gameplay holds driving assistance, difficulty and hours, world and traffic, and controls. Audio, Speech, and Problem reports have separate choices. F1 explains the selected setting."),
        None,
    ),
    (
        "Speech and braille sends messages to both. Braille only requires NVDA or JAWS. Speech returns if the screen reader closes. You can also choose a separate driving event voice.",
        Some("You can choose a separate driving event voice."),
        None,
    ),
    (
        "For a problem report, use Where the game log is saved to find the session record, including spoken messages. It stays on your computer unless you share it.",
        None,
        None,
    ),
    (
        "Discord shows broad activity and clears it after half an hour idle.",
        None,
        None,
    ),
    (
        "How do I change stations or play my own music? With the engine running, {{radio}} switches the radio on, and Page Up or Page Down tunes. Put playlist files in the Playlists folder inside your saves folder. Radio status gives its location.",
        Some("How do I change stations? With the engine running, {{radio}} switches the radio on, and Page Up or Page Down tunes."),
        Some("How do I change stations? With the engine running, a three-finger flick right or left tunes. Radio on or off is under Radio and CB on the three-finger tap list."),
    ),
    (
        "Can I play in braille only? Set Output to braille only under Settings, Speech. It needs NVDA or JAWS.",
        None,
        None,
    ),
    (
        "Controllers work alongside the keyboard. Button names use the Xbox layout. Change button assignments under Settings, Gameplay, Controls.",
        Some("Controllers work alongside the keyboard. Button names use the Xbox layout. Change button assignments under Settings, Gameplay, Controls."),
        Some("Controllers work alongside the screen. Button names use the Xbox layout. Change button assignments under Settings, Gameplay, Controls."),
    ),
    (
        "On newer Xbox controllers, Back is the View button, left of the Xbox button, and Start is the Menu button on its right. On Windows, the Xbox button opens the Game Bar; press it again to return.",
        Some("On newer Xbox controllers, Back is the View button, left of the Xbox button, and Start is the Menu button on its right."),
        Some("On newer Xbox controllers, Back is the View button, left of the Xbox button, and Start is the Menu button on its right."),
    ),
    (
        "If the controller does nothing, check that Controller is enabled under Settings, Gameplay, Controls. The game uses the first controller it finds, so unplug other controllers and close Steam. If the Xbox button opens Steam Big Picture, press B to close it.",
        Some("If the controller does nothing, check that Controller is enabled under Settings, Gameplay, Controls. The game uses the first controller it finds, so disconnect other controllers."),
        Some("If the controller does nothing, check that Controller is enabled under Settings, Gameplay, Controls. The game uses the first controller it finds, so disconnect other controllers."),
    ),
];

/// Whole pages a touch player reads in place of the desktop page:
/// `(desktop title, touch title, lines)`.
pub(super) static TOUCH_PAGES: &[(&str, &str, &[&str])] = &[
    (
        "Menus and message review",
        "Menus and message review",
        &[
            "Flick right or left to move to the next or previous option. Double tap chooses it. Tap reads it again.",
            "Flick down with two fingers to go back. Flick up or down with three fingers to jump to the first or last option.",
            "Flick left or right with three fingers to move ten options back or forward.",
            "On a setting, flick up for its next value and down for the previous one.",
            "Flick up with two fingers to hear what the selected option does.",
            "Flick left with two fingers to repeat the latest message, then step back. Flick right with two fingers to move forward.",
            "Tap with two fingers to stop speech.",
            "In How to play, flick right or left to read one line at a time. Flick up for the next page and down for the previous one. Double tap reads the whole page.",
            "Flick up with three fingers for the first page, and down for the last.",
            "Learn game sounds plays road cues and explains their meaning. It is available on the main and pause menus.",
            "Practice gestures names each gesture without moving the truck. It is on the main and pause menus. Flick down with two fingers twice to leave.",
        ],
    ),
    (
        "Starting and stopping the truck",
        "Starting and stopping the truck",
        &[
            "Turn two fingers right to start the engine. Turn them left to stop it once the truck has stopped.",
            "Wait for air pressure to reach 100 psi, then swipe up and hold to release the parking brake. Repeated hard braking uses air.",
            "Swipe up and hold to accelerate. Lift to coast. Swipe down and hold to brake.",
            "A deep swipe down, about three flicks long, is the hardest stop. Under one mile per hour it sets the parking brake.",
            "In automatic, stop fully, lift, then swipe down and hold again to reverse. A quick swipe or a brake held through the stop keeps the truck stopped.",
            "While reversing, stop with a swipe up and hold, lift, then swipe up and hold again for forward.",
            "With Latching brake on, brake briefly, then swipe down and hold again for half a second. A click and announcement confirm the latch.",
            "Brake again or accelerate to release the latch. The gas never latches.",
            "Applying the parking brake at speed causes a hard stop and tire damage. Parked with the parking brake set, {{do:cruise}} to run the engine at a fast idle.",
            "In a manual, hold a pedal and flick up or down with a second finger to shift. The flick works the clutch.",
            "Slowing in a tall gear takes neutral. A flick up from neutral takes the gear that fits your speed.",
            "Neutral, Reverse, and Automatic or manual shifting are under Gears and engine brake on the three-finger tap list.",
            "Hold two fingers to sound the horn.",
        ],
    ),
    (
        "Cruise and hills",
        "Cruise and hills",
        &[
            "With cruise off, {{do:cruise}} to turn it on. On open roads it follows traffic. On low-speed roads it follows local limits, queues and corners.",
            "Cruise follows traffic. Its normal gap is three seconds in clear weather and longer in poor weather. It does not steer.",
            "With cruise on, {{do:cruise_up}} or {{do:cruise_down}} to move the cruise speed to the next multiple of five.",
            "That changes the open-road cruise speed, even while cruise holds a lower limit.",
            "Cruise slows early for a sharp drop in the posted limit, and never holds more than five over it.",
            "{{do:speed}} for the active speed-control mode and cruise speed.",
            "Touch the brakes to cancel cruise. Turned on again, it resumes the previous target. Cruise pauses at the pickup and comes back on after you leave with the load.",
            "Before a descent, reduce speed and choose a lower gear. Hold speed with the engine brake. Use the brakes in short presses, with time to cool between them.",
            "{{do:engine_brake}} to turn the engine brake on or off.",
            "Engine brake stage 1, 2 and 3, and Automatic engine brake on or off, are under Gears and engine brake on the three-finger tap list.",
            "Hot brakes lose stopping power. Safe speed, under Speed and cruise on the list, gives a safe speed for the load, grade, weather, and bend. Strong engine braking can break traction on ice.",
            "Towns that restrict engine braking warn you before they fine it. Downhill grades and emergencies are exempt.",
        ],
    ),
    (
        "Driving information keys",
        "Driving gestures",
        &[
            "{{do:speed}} for your speed, gear, and speed-control mode. With a manual transmission it adds RPM. It also adds air or brakes when they need attention, and the distance to an exit you signaled for.",
            "{{do:route}} for how far along you are, distance remaining, and your location. A planned stop changes the distance to that stop.",
            "{{do:upcoming}} for what is coming up: your signaled exit, the light or stop sign at the end of the ramp, the next lower speed limit, the next stop, and the next bend you need to slow for.",
            "Double tap to signal for an announced exit or cancel the signal.",
            "Everything else is on the three-finger tap list. Tap with three fingers, flick to a command, and double tap to run it.",
            "Its first row is the Status menu, with Route, Driver, Map, and Radio reports, plus Driver apps.",
            "Under Speed and cruise, Posted speed limit reads the limit, the zone, and how far over you are. Safe speed is there too.",
            "Under Road and stops are Grade, Weather, and Lane position, which also says whether the lane beside you is open.",
            "Rest stop, also under Road and stops, plans rest while moving or opens a nearby stop when fully stopped.",
            "Repeat the last announcement is on the list. Repeat the last CB chatter, under Radio and CB, gives its current distance.",
            "Where am I has the state, the road, the town or nearest town, the direction, and the county.",
            "Under Truck, Fuel reads fuel and range.",
            "Under Hours and clock, Clock reads the clock, deadline, and nearest hours limit. Time at the wheel adds time on duty. What ends this shift adds reachable legal rest.",
            "Tap with two fingers to stop the driving event voice. Flick down with two fingers to pause. Flick up with two fingers for the wheel's help.",
            "Flick left or right with two fingers to review messages. Flick left or right with three fingers to tune the radio.",
            "Most gestures can be moved under Settings, Gameplay, Controls, then Touch gestures. These pages follow your assignments.",
        ],
    ),
    (
        "Steering and curves",
        "Steering and curves",
        &[
            "Full lane keeping centers the truck, follows bends, and takes route exits including the destination. Flick left or right to change one lane.",
            "Partial follows bends with gentle drift. Off leaves steering to you unless curve assistance is enabled. Swipe left or right and hold to steer.",
            "With partial or off, hold the swipe across the lane line to change lanes. A flick changes one lane with any lane keeping.",
            "With Tilt steering on under Touch gestures, tilting the phone steers too. With lane keeping on full, a tilt changes one lane.",
            "If small movements of the phone steer, raise Tilt dead zone or Tilt for full steering under Touch gestures, so the phone has to tilt further.",
            "With the default steering guide, steer toward the side the engine sound moves to. Road noise tells you position: sound to the right means the truck sits right of center.",
            "With lane keeping off and curve assistance off, swipe and hold toward a called bend to follow the road. Lifting straightens the truck with the road.",
            "Hold one finger still for half a second to straighten the wheel. Your position within the lane remains yours to correct.",
            "The curve call gives direction, distance, and advisory speed. Brake to that speed before the bend. A tone marks the bend's side.",
            "Rumble-strip and gravel sounds come from the edge you reached. Correct gently. Crossing an undivided centerline puts you in opposing traffic.",
            "Lane locator, under Road and stops on the three-finger tap list, is a soft tock that follows your place in the lane, with lane keeping partial or off.",
            "The Steering guide setting can reverse which way to steer, and Lane guide sound can use a soft tone instead of the engine.",
            "Curve assistance slows and steers through mapped bends. If you brake, it stops slowing for that bend but keeps steering.",
        ],
    ),
    (
        "Exits, ramps, and arrival",
        "Exits, ramps, and arrival",
        &[
            "While rolling, Rest stop asks what the stop is for: fuel, a break, sleep, fuel and sleep, or food. It is under Road and stops on the three-finger tap list.",
            "With hours-planning hints on, a break or sleep plan takes the recommended stop. Run Rest stop again to cancel the plan.",
            "Planning a stop does not signal for it. Double tap to signal for the announced exit or cancel the signal.",
            "Unless lane keeping is on full, move to the right lane, then steer right into the exit lane when it opens.",
            "Keep road speed until the exit lane. Brake to the announced exit speed before its curve. Full lane keeping makes the lane moves for you.",
            "Most ramps end at a light or stop sign. Stop at red; stop safely for yellow if possible. At a stop sign, wait for a clear gap after stopping.",
            "The sensor tick speeds up near the stop bar. Cross traffic can hit a truck that runs the light.",
            "When clear, pull ahead as instructed. Facility stopping assistance can take the pedals to the gate. Braking ends it.",
            "Follow the local street and yard guidance, stop at the gate, then double tap to confirm arrival. At the receiver choose Dock and deliver.",
            "If you miss the destination exit, follow the safe turnaround. Exits you have no reason to take are not announced.",
            "Fully stopped at a route stop, double tap to open its services. Fully stopped elsewhere, Rest stop on the list offers emergency sleep on the shoulder. A nearby stop always comes first.",
        ],
    ),
    (
        "Traffic, weather, and enforcement",
        "Traffic, weather, and enforcement",
        &[
            "GPS announces state lines, towns, route changes, traffic, work zones, and useful stops. Grades and terrain come from the route. Weather and traffic vary by place and time.",
            "Nearby vehicles merge, slow, pass, and queue. Lane position, under Road and stops on the three-finger tap list, tells you which neighboring lanes are available. Check before changing lanes or returning after a pass.",
            "A passing vehicle grows louder alongside you, then fades ahead. Listen for which side it is on, but use the lane check before moving over.",
            "Mainline traffic stays to your left on an on-ramp. The distant freeway sound follows traffic activity, so busy and quiet stretches sound different.",
            "When you hear Brake now, slow down right away. Change lanes or brake means something is in your lane: move to the open lane it names, or slow to 30 and take it. Rockfall or a stopped car needs a stop.",
            "Tire debris is called out and driven over. Nothing brakes for it. Full lane keeping moves over for debris and slow vehicles when a lane is open, and avoids starting a pass near an exit.",
            "Leave a coned lane when warned. Driving through barrels causes damage, a citation, and a serious violation.",
            "Weather changes grip, visibility, safe speed, following distance, and fuel use. Weather, under Road and stops on the list, reports conditions. Snow and ice need gentler braking and steering.",
            "The calendar starts in spring and advances with driving and rest. Live weather can follow the real date or an independent career calendar.",
            "When a trooper signals you, double tap and brake to a stop for a license and logbook check. Ignoring the lights escalates to a forced stop or pursuit.",
            "CB chatter is what other drivers say they saw, and it is not always right. It can be stale and never claims the road is clear. Repeat the last CB chatter, under Radio and CB on the list, repeats it with updated distance.",
            "Citations cost money and affect your record. Serious violations can suspend your CDL and remove driving work. Fleeing is a major offense.",
            "The dash overspeed chime is a free warning. Tickets already paid on the road are not charged again at settlement.",
            "At an open scale, slow into its lane and double tap to signal. Stop on the scale, then double tap again to check in.",
        ],
    ),
    (
        "Hours, fatigue, and rest stops",
        "Hours, fatigue, and rest stops",
        &[
            "The hours clock permits eleven driving hours after ten hours off, inside a fourteen-hour duty window. A thirty-minute break is due after eight cumulative driving hours.",
            "Any thirty minutes in a row without driving count as that break, including loading or inspection. They do not necessarily reset the duty window.",
            "Realistic and Relaxed hours use the same limits. Relaxed brings lighter fines, fewer inspections, and rare road hazards. A warning comes before each limit: at one hour left on Relaxed pacing and two hours on Standard. Real time warns at two hours, one hour and thirty minutes. No warning comes for a limit you will not reach before the trip ends.",
            "Time at the wheel, When the break is due, and What ends this shift are under Hours and clock on the three-finger tap list. Check again as traffic or weather changes.",
            "Ten-hour sleep is the simplest reset. Sleeper parking also offers 2, 3, 7 or 8 hours. Pair 8 with 2, or 7 with 3, for a split rest. The long part pauses the duty window. The short part does not, until the pair is done.",
            "A nap that is not part of a split leaves the duty window running.",
            "Fatigue rises faster at night and reduces reaction time. On-duty time at a stop, such as fueling, a dock or an inspection, builds it at a third of the driving rate. Respond to nod-off warnings by steering or braking. Repeated misses can force you off the road.",
            "Job details and the departure line say how many hours of a run fall at night, between 9 PM and 5 AM.",
            "Fatigue slows work by hand. Fueling, the walk-around, chain work and pulling into a stop take longer from worn up, and the menus read the longer minutes.",
            "An inspection or traffic stop while you are severe ends in a ten-hour fatigue out-of-service order. On Relaxed pacing the first one is a warning.",
            "Drowsy or severe on a load, Call dispatch offers Too tired to drive safely once per load. The appointment moves for a night's sleep and the load pays its base rate, with no on-time bonus. A third call in a week costs dispatch trust.",
            "Meals ease fatigue, and a half-hour meal counts as a break. The lift from coffee or an energy drink comes back when it wears off, and a second one before then adds no lift. Proper sleep takes off 8 points of fatigue an hour, so ten hours clears 80. A legal reset with poor rest can leave you tired.",
            "A stop offers only its listed services. A full parking lot can still sell diesel. A motel may offer full rest at your expense.",
            "Sleeping 10 hours in the lot gives poor rest: 5 points an hour, never below 30. Ten hours in sleeper parking is proper sleep.",
            "Away from route points and fully stopped, Rest stop under Road and stops, or the pause menu, offers emergency sleep on the shoulder. Ten hours pass with poor rest, and you may get a parking ticket or minor damage.",
            "The deadline keeps running during all rest. Logbook at the terminal and the ELD app show your duty time and limits.",
        ],
    ),
    (
        "Driver apps",
        "Driver apps",
        &[
            "Open driving status with a three-finger tap, then a double tap, and choose Driver apps.",
            "Flick right or left to choose an app, and double tap to open it. Flick down with two fingers to return.",
            "In a report, flick right or left to move through the information, and double tap to repeat what you selected. Flick down with two fingers to return to Driver apps, then to driving status.",
            "The Radio app turns the radio on or off, tunes, and keeps favorites. You can also suggest a station there. Search by part of a name, call sign, or format, then choose a result. Out-of-range stations cannot be tuned here.",
            "Navigation reads your next guidance, progress, and next listed exit. Map under driving status gives a broader view of the route and stops.",
            "Weather gives conditions, suggested safe speed, how old the report is, and any active live weather alerts. Simulated weather includes a forecast. A live report may not.",
            "Traffic reports traffic ahead, including a reported vehicle's distance and speed. No slowdowns reported in the next twenty miles does not guarantee an empty road.",
            "Truck stops lists up to three route stops in the next hundred miles, with distance and services. Check for fuel, repairs, and parking before choosing a stop.",
            "Road chatter gives informal driver reports that may be stale. With hours enforcement off, there are no enforcement reports. Repeat the last CB chatter, under Radio and CB on the three-finger tap list, repeats the last chatter with its distance updated.",
            "ELD gives your hours and guidance about reachable legal rest. Check again if traffic or weather slows your trip.",
        ],
    ),
    (
        "Settings and accessibility",
        "Settings and accessibility",
        &[
            "Settings save as you change them. Flick right or left to choose a setting, then flick up for its next value or down for the previous one.",
            "Gameplay holds driving assistance, difficulty and hours, world and traffic, and controls. Audio, Speech, and Problem reports have separate choices.",
            "Flick up with two fingers on a setting to hear what it does.",
            "Realistic leaves lane keeping off. Balanced adds partial lane keeping and facility stops. All assists adds full lane keeping and a fifty-five mile per hour descent ceiling. Adjusting assistance yourself changes the preset name to Custom.",
            "Presets leave driving pace, legal hours, transmission, world sources, latching brake, and cue volumes as they are.",
            "Relaxed pacing passes about ten minutes of road and clock each real minute, with more response time and gentler consequences. Standard moves twice as fast. Real time follows the clock at real speed.",
            "Change pacing mid-drive from the pause menu. It begins when the truck next stops. Legal hours and time remaining on a deadline stay intact.",
            "Choose simulated or available live weather, traffic, and parking separately from online sharing. Fuel prices can use the weekly national diesel average with regional differences or simulated regional prices.",
            "Adjust music, radio, engine, weather, and gameplay cues separately if one covers another. Lower game sounds during speech turns the engine, weather, and radio to half volume while the road voice speaks.",
            "Driving speech chooses Standard, Quiet, or Urgent only announcements. Readouts still answer when asked. Place names and roadside chatter have separate controls.",
            "You can choose a separate driving event voice.",
        ],
    ),
    (
        "Online sharing and backups",
        "Online sharing and backups",
        &[
            "In Online you can browse driver profiles and account achievements, connect an account, choose what you share, and manage private backups. Viewing drivers shares nothing about you.",
            "Drivers on duty refreshes about once a minute. Double tap a driver for their public profile. Driver directory includes drivers who are not currently playing; Check again refreshes it.",
            "Use an activation code in your browser to connect an existing account and enable profile sharing and backup. You can turn either off afterward.",
            "Profile sharing includes your activity, achievements, and road journal. Public profiles omit current cash, full saves, coordinates, and precise location. Career statistics need an accepted private backup.",
            "Cloud backup keeps the last ten backups of each career, for up to ten careers. Restoring checks the backup first and keeps your replaced save as a spare copy. Back up this career now uploads immediately.",
            "Online services switches sharing services off together while remembering your choices. Live weather, traffic, and parking keep their separate settings.",
        ],
    ),
    (
        "Common questions",
        "Common questions",
        &[
            "Do I have to buy a truck? No. The carrier assigns company drivers a truck and pays its fuel, repairs, and insurance. Buying trucks opens with the owner-operator buy-in from level 18. Business status lists what else it needs.",
            "What are the fleets on Career stats? Each is a set of carrier trucks that dispatch assigns by level: the yard standard, then the regional fleet at level 4, the long-haul fleet at 9, the premium fleet at 13, and first pick of the yard at 17. Moving up costs you nothing.",
            "Why am I in a different truck on some loads? Until level 9, dispatch matches a spare truck to each load. From level 9 you keep one truck for every load until dispatch moves you up to the next fleet.",
            "Why am I not in the truck my level earns? Low dispatch trust, a suspended CDL, debt, or your driving record can hold equipment back. Career stats names the reason and what brings the truck back.",
            "What is the difference between reputation and dispatch trust? Reputation is a score from your deliveries, lowered by your driving record: Outstanding, Well regarded, Established, Unproven, Shaky, Damaged, or Ruined. New drivers start at 50, Unproven. A trust line shows it as, for example, Dispatch trust: full. Reputation 85 out of 100. Dispatch trust is what the carrier lets you do right now. Your CDL, your record, and what you owe count toward it too. Settings, Speech has Score display: both, percentage only, or description only.",
            "Why can I not accept a load? It may need a certificate, endorsement, or trailer you do not have. Flick up with two fingers on the load for its requirements, and Licenses and training offers the courses.",
            "Why did I get a citation at a weigh station? When an open weigh station is called, every truck must pull in. Double tap to signal, stop on the scale, then double tap again to check in.",
            "Why is the limit 55 on a fast highway? Some states hold trucks below the car limit. Posted speed limit, under Speed and cruise on the three-finger tap list, names the state when it does.",
            "I missed my exit. What now? Keep driving and follow the turnaround guidance. {{do:route}} for the distance left.",
            "The truck will not move. What should I check? Fuel, the engine, air pressure, and the parking brake. Damage, worn parts, or a suspended CDL can also stop you.",
            "How do I know when a hill ends? Grade, under Road and stops on the list, reads the grade under the wheels and the next one worth planning for.",
            "How do I change stations? With the engine running, a three-finger flick right or left tunes. Radio on or off is under Radio and CB on the list.",
            "Can I hear a weather warning again? With live weather on, the Weather app under Driver apps lists active alerts. A two-finger flick left also steps back through recent messages.",
            "Can I steer myself? Yes. Choose the Realistic preset, or set lane keeping to off, under Settings, Gameplay, Driving assistance.",
        ],
    ),
];

/// The touch version of the page titled `title`, if it has one.
pub(super) fn touch_page(title: &str) -> Option<(&'static str, &'static [&'static str])> {
    TOUCH_PAGES
        .iter()
        .find(|(desktop, ..)| *desktop == title)
        .map(|(_, touch, lines)| (*touch, *lines))
}
