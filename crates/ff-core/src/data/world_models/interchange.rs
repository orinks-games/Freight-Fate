//! Highway exits and the route-shield helpers their spoken phrases use (the
//! `Interchange` half of `world_models.py`).

use once_cell::sync::Lazy;
use regex::Regex;

/// A highway exit/junction along a leg, sourced from OpenStreetMap.
///
/// `ramp_control` is what governs the ramp terminal where the off-ramp meets
/// the surface road: `signal` (a traffic light on a ramp-link node), `stop`
/// (a stop sign), `yield` (a give-way at the terminal), `roundabout` (the
/// terminal node sits on a roundabout way), `none` (free-flow), or `""` when
/// OSM had no control tagged -- the runtime then falls back to a seeded
/// heuristic.
///
/// `ramp_far_end` is what the exit's ramp chains reach, walked from OSM link
/// topology: `motorway` (every chain merges onto another motorway; such exits
/// also carry `ramp_control: none`), `surface` (at least one chain ends off
/// the motorway network), or `""` when the walk could not judge. `surface`
/// tells the runtime NOT to guess free flow off the exit's `via` signage,
/// which points where the exit is signed toward, not at the road the ramp
/// lands on.
///
/// `ramp_length_ft_forward/backward` is the exit ramp's length for travel
/// along the leg A->B or B->A, derived from OSM geometry: summed along the
/// link way(s) from the gore to the node where the ramp ends. It starts where
/// the OSM ramp way leaves the motorway, at or near the gore, so a
/// deceleration lane before the gore is NOT included. `None` when no
/// gore matched in that direction or the bake's screen dropped the value.
///
/// `ramp_terminal_node_forward/backward` is the OSM node id where that same
/// walk ends on a surface road (read from link topology), per direction: the
/// crossroad the ramp hands the truck to, and the key of the facility street
/// chain that starts there (`World::facility_exit_route`). `None` for a ramp
/// ending in a merge, or where no length survived the screen.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Interchange {
    pub at_mi: f64,
    pub exit_ref: String,
    pub name: String,
    pub destinations: Vec<String>,
    pub via: String,
    pub highway: String,
    pub source: String,
    pub ramp_control: String,
    pub ramp_far_end: String,
    pub ramp_advisory_mph_forward: Option<f64>,
    pub ramp_advisory_mph_backward: Option<f64>,
    pub ramp_advisory_source: String,
    pub ramp_length_ft_forward: Option<f64>,
    pub ramp_length_ft_backward: Option<f64>,
    pub ramp_length_source: String,
    pub ramp_terminal_node_forward: Option<i64>,
    pub ramp_terminal_node_backward: Option<i64>,
    pub ramp_terminal_source: String,
}

impl Interchange {
    /// Lower-case lead phrase for GPS announcements.
    pub fn spoken_phrase(&self) -> String {
        self.phrase(Some(&self.via), &self.destinations)
    }

    /// True when `via` names the same route the leg runs on -- the bake merged
    /// a mainline entrance ramp's signage into this exit's record, so the via
    /// and any mainline destinations are the driver's own road, not the
    /// exit's. Requires a cardinal word (an unsigned "I 35" could still be the
    /// exit's real signage) and refuses route modifiers (a "Business" loop is
    /// a different road that happens to share the number).
    pub fn via_is_mainline(&self, leg_highway: &str) -> bool {
        let via_token = route_token(&self.via);
        if via_token.is_empty() || via_token != route_token(leg_highway) {
            return false;
        }
        if !has_cardinal(&self.via) {
            return false;
        }
        !has_modifier(&self.via) && !has_modifier(leg_highway)
    }

    /// `spoken_phrase` aware of the leg's own highway: when `via` is just the
    /// mainline (see `via_is_mainline`), the "for {via}" part is dropped and
    /// so is every destination that names the mainline's own signage: the
    /// leg's route token, any city on the driver's own route
    /// (`mainline_cities`), and any destination the leg's other exits also
    /// sign (`sibling_destinations`) -- a destination re-signed down the same
    /// road is the mainline's promise, not this exit's.
    pub fn spoken_phrase_on(
        &self,
        leg_highway: &str,
        mainline_cities: &[String],
        sibling_destinations: &[String],
    ) -> String {
        if !self.via_is_mainline(leg_highway) {
            return self.spoken_phrase();
        }
        let leg_token = route_token(leg_highway);
        let dests: Vec<String> = self
            .destinations
            .iter()
            .filter(|d| {
                route_token(d) != leg_token
                    && !mainline_cities.iter().any(|c| c == *d)
                    && !sibling_destinations.iter().any(|c| c == *d)
            })
            .cloned()
            .collect();
        self.phrase(None, &dests)
    }

    pub fn near_phrase(&self) -> String {
        near_from(&self.spoken_phrase())
    }

    pub fn near_phrase_on(
        &self,
        leg_highway: &str,
        mainline_cities: &[String],
        sibling_destinations: &[String],
    ) -> String {
        near_from(&self.spoken_phrase_on(leg_highway, mainline_cities, sibling_destinations))
    }

    fn phrase(&self, via: Option<&str>, destinations: &[String]) -> String {
        let head = if self.exit_ref.is_empty() {
            "exit".to_string()
        } else {
            format!("exit {}", self.exit_ref)
        };
        let mut parts = vec![head];
        if let Some(via) = via {
            let via = format_route_ref(via);
            if !via.is_empty() {
                parts.push(format!("for {via}"));
            }
        }
        let dest = join_destinations(&destinations_without_via(via.unwrap_or(""), destinations));
        if !dest.is_empty() {
            parts.push(format!("toward {dest}"));
        } else if !self.name.is_empty() && self.exit_ref.is_empty() {
            parts.push(format!("for {}", self.name));
        }
        parts.join(" ")
    }

    pub fn exit_label(&self) -> String {
        if self.exit_ref.is_empty() {
            String::new()
        } else {
            format!("exit {}", self.exit_ref)
        }
    }
}

fn near_from(phrase: &str) -> String {
    let mut chars = phrase.chars();
    let head: String = chars
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    format!("{head}{} now.", chars.as_str())
}

/// Whole-word, case-insensitive cardinal check: 'I 35 North' yes, 'Northern
/// Avenue' no.
fn has_cardinal(value: &str) -> bool {
    value.split_whitespace().any(|w| {
        matches!(
            w.to_ascii_lowercase().as_str(),
            "north" | "south" | "east" | "west"
        )
    })
}

/// Whole-word, case-insensitive route-modifier check ('Business', 'Spur',
/// ...). A modified route is a different road that shares the number, never
/// the mainline.
fn has_modifier(value: &str) -> bool {
    value.split_whitespace().any(|w| {
        matches!(
            w.to_ascii_lowercase().as_str(),
            "business" | "bypass" | "loop" | "spur" | "alt" | "alternate" | "truck"
        )
    })
}

/// "US 31 South;US 280" -> "US-31 South and US-280". Only a shield code
/// (all capitals: I, US, OK, SR) is joined to its number; a spelled-out word
/// stays a word ("Route 255", "Highway 104").
pub fn format_route_ref(value: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for chunk in value.split(';') {
        let reference = chunk.split_whitespace().collect::<Vec<_>>().join(" ");
        if reference.is_empty() {
            continue;
        }
        let mut parts: Vec<String> = reference.split(' ').map(str::to_string).collect();
        if parts.len() >= 2
            && parts[1].chars().next().is_some_and(|c| c.is_numeric())
            && parts[0].chars().all(|c| c.is_ascii_uppercase())
        {
            let joined = format!("{}-{}", parts[0], parts[1]);
            parts.splice(0..2, [joined]);
        }
        out.push(parts.join(" "));
    }
    out.join(" and ")
}

/// The province code ("NB") for a code or an English name ("New
/// Brunswick"), from the provincial diesel table, which lists every
/// province and territory the map reaches.
fn province_code(province: &str) -> Option<&'static str> {
    crate::models::economy::CANADA_DIESEL_CENTS_PER_L
        .iter()
        .find(|(code, name, _)| *code == province || *name == province)
        .map(|(code, _, _)| *code)
}

/// The word a province signs its numbered roads with, for a route number
/// OpenStreetMap gives bare ("255", "143 Sud") or behind the province's
/// code ("NS 102"): an exit line says "for Route 255" or "for Highway 102",
/// never "for 255" or "for N S 102". `province` is where the exit is, as a
/// code or the English name; a chunk's own province code wins over it.
/// Anywhere else gets `None`, and so does a via that does not start with a
/// route number.
pub fn provincial_route(province: &str, via: &str) -> Option<String> {
    let mut words = via.split_whitespace().peekable();
    let mut code = province_code(province);
    if let Some(own) = words.peek().and_then(|w| province_code(w)) {
        code = Some(own);
        words.next();
    }
    let code = code?;
    let number: u32 = words.next()?.parse().ok()?;
    let rest: Vec<&str> = words.collect();
    let word = match code {
        // Quebec: autoroutes carry one or two digits, or a 400-series spur
        // (A-410, A-640, A-955); the numbered routes run 100 to 399.
        "QC" => {
            if (100..400).contains(&number) {
                "Route "
            } else {
                "A-"
            }
        }
        "NB" | "PE" => "Route ",
        // Nova Scotia: the 100-series highways, the trunks below 100 and the
        // routes from 200.
        "NS" => match number {
            100..=199 => "Highway ",
            0..=99 => "Trunk ",
            _ => "Route ",
        },
        _ => "Highway ",
    };
    let mut out = format!("{word}{number}");
    for w in rest {
        out.push(' ');
        out.push_str(w);
    }
    Some(out)
}

/// `via` with every `;`-separated bare route number named by its province's
/// word (see [`provincial_route`]); anything else is kept as read.
pub fn name_provincial_routes(province: &str, via: &str) -> String {
    if via.trim().is_empty() {
        return via.to_string();
    }
    via.split(';')
        .map(|chunk| provincial_route(province, chunk).unwrap_or_else(|| chunk.trim().to_string()))
        .collect::<Vec<_>>()
        .join(";")
}

static ROUTE_TOKEN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*((?:I|US|[A-Za-z]{2})[-\s]?\d+)").expect("valid regex"));

/// Leading route shield of a string, normalized for comparison:
/// 'I 70 East' -> 'I70', 'US 1 North' -> 'US1', 'Trenton' -> ''.
pub fn route_token(value: &str) -> String {
    match ROUTE_TOKEN.captures(value.trim()) {
        Some(caps) => caps[1]
            .chars()
            .filter(|c| *c != '-' && !c.is_whitespace())
            .collect::<String>()
            .to_uppercase(),
        None => String::new(),
    }
}

/// Drop destinations that merely restate the via route (via 'I 70' with a
/// destination of 'I 70 East'), so the spoken phrase never says it twice. The
/// via itself still carries the route, so emptying the list reads cleanly
/// ('exit 101A for I-70').
pub fn destinations_without_via(via: &str, destinations: &[String]) -> Vec<String> {
    let token = route_token(via);
    if token.is_empty() {
        return destinations.to_vec();
    }
    destinations
        .iter()
        .filter(|d| route_token(d) != token)
        .cloned()
        .collect()
}

/// ['Trenton', 'New York'] -> 'Trenton and New York'; Oxford-comma 3+.
pub fn join_destinations(destinations: &[String]) -> String {
    let items: Vec<&String> = destinations.iter().filter(|d| !d.is_empty()).collect();
    match items.len() {
        0 => String::new(),
        1 => items[0].clone(),
        2 => format!("{} and {}", items[0], items[1]),
        n => format!(
            "{}, and {}",
            items[..n - 1]
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            items[n - 1]
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ix(exit_ref: &str, via: &str, destinations: &[&str]) -> Interchange {
        Interchange {
            exit_ref: exit_ref.to_string(),
            via: via.to_string(),
            destinations: destinations.iter().map(|d| d.to_string()).collect(),
            ..Interchange::default()
        }
    }

    #[test]
    fn mainline_via_is_dropped_from_the_exit_label() {
        // us/oklahoma: oklahoma_city_ok_us -> ardmore_ok_us at_mi 97.1 -- the
        // bake merged the I-35 North entrance ramp's signage into exit 31B.
        let record = ix("31B", "I 35 North", &["Oklahoma City", "Dallas", "Waurika"]);
        let cities: Vec<String> = ["Oklahoma City", "Ardmore", "Dallas"]
            .iter()
            .map(|c| c.to_string())
            .collect();
        assert!(record.via_is_mainline("I-35"));
        assert_eq!(
            record.spoken_phrase_on("I-35", &cities, &[]),
            "exit 31B toward Waurika"
        );
        // And when the route does not pass through a signed city, the
        // destinations the leg's other exits also sign still drop out
        // (Dallas is re-signed down I-35 on exits 108B and 86).
        let cities: Vec<String> = ["Oklahoma City", "Ardmore"]
            .iter()
            .map(|c| c.to_string())
            .collect();
        let siblings: Vec<String> = vec!["Dallas".to_string()];
        assert_eq!(
            record.spoken_phrase_on("I-35", &cities, &siblings),
            "exit 31B toward Waurika"
        );
        // The leg-agnostic phrase keeps the old wording.
        assert_eq!(
            record.spoken_phrase(),
            "exit 31B for I-35 North toward Oklahoma City, Dallas, and Waurika"
        );
    }

    #[test]
    fn a_real_exit_via_is_untouched() {
        let record = ix("42", "OK 53 West", &["Comanche"]);
        let cities: Vec<String> = Vec::new();
        assert!(!record.via_is_mainline("I-35"));
        assert_eq!(
            record.spoken_phrase_on("I-35", &cities, &[]),
            "exit 42 for OK-53 West toward Comanche"
        );
    }

    #[test]
    fn a_bare_canadian_route_number_takes_its_province_word() {
        assert_eq!(name_provincial_routes("Quebec", "255"), "Route 255");
        assert_eq!(name_provincial_routes("QC", "143 Sud"), "Route 143 Sud");
        assert_eq!(name_provincial_routes("Quebec", "20"), "A-20");
        assert_eq!(name_provincial_routes("Quebec", "55;20"), "A-55;A-20");
        assert_eq!(name_provincial_routes("New Brunswick", "2"), "Route 2");
        assert_eq!(name_provincial_routes("Nova Scotia", "102"), "Highway 102");
        assert_eq!(name_provincial_routes("Nova Scotia", "4"), "Trunk 4");
        assert_eq!(name_provincial_routes("Nova Scotia", "236"), "Route 236");
        assert_eq!(name_provincial_routes("Ontario", "401"), "Highway 401");
        // A province code on the sign gives way to the province's word.
        assert_eq!(
            name_provincial_routes("NB", "NS 102;2"),
            "Highway 102;Route 2"
        );
        assert_eq!(name_provincial_routes("Maine", "NB 1"), "Route 1");
        assert_eq!(name_provincial_routes("Maine", "ME 9"), "ME 9");
        // Read as written: a signed shield, a name, a US state.
        assert_eq!(name_provincial_routes("Ontario", "QEW"), "QEW");
        assert_eq!(name_provincial_routes("Maine", "1"), "1");
        let record = ix(
            "185",
            &name_provincial_routes("Quebec", "255"),
            &["Baie-du-Febvre"],
        );
        assert_eq!(
            record.spoken_phrase(),
            "exit 185 for Route 255 toward Baie-du-Febvre"
        );
        let record = ix("15", &name_provincial_routes("Nova Scotia", "102"), &[]);
        assert_eq!(record.spoken_phrase(), "exit 15 for Highway 102");
    }

    #[test]
    fn a_business_route_is_not_the_mainline() {
        let record = ix("10", "I 40 Business", &["Shamrock"]);
        assert!(!record.via_is_mainline("I-40"));
    }
}
