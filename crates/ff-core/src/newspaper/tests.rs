use super::*;
use serde_json::json;

/// The site's fixture edition as the site's route answers it
/// (orinks-net `lib/freight-fate-paper-speech.ts`): made-up drivers.
pub const FIXTURE: &str = include_str!("fixture_issue.json");

fn fixture() -> PaperIssue {
    parse_issue(FIXTURE).expect("the fixture parses")
}

fn small(at: f64) -> PaperEvent {
    PaperEvent::Received {
        issue: fixture(),
        at,
    }
}

#[test]
fn the_fixture_reads_as_the_site_wrote_it() {
    let paper = fixture();
    assert_eq!(paper.issue, 25);
    assert_eq!(
        paper.title(),
        "The Mile Marker, issue 25. November 16 to 22, 2026."
    );
    let order: Vec<&str> = paper
        .reading_order()
        .iter()
        .map(|s| s.heading.as_str())
        .collect();
    assert_eq!(order[0], "Since Monday, November 23");
    assert_eq!(order[1], "Front page");
    assert_eq!(order.last(), Some(&"Back page: fiction"));
    assert!(paper
        .spoken_text()
        .contains(&"Most miles: Dana Rig, 4,210 miles.".to_string()));
}

/// The transition table: every state against every event.
#[test]
fn transitions() {
    let holding = PaperState::NoIssue.next(small(10.0));
    let rows: Vec<(PaperState, PaperEvent, PaperState)> = vec![
        (PaperState::NoIssue, small(10.0), holding.clone()),
        (
            PaperState::NoIssue,
            PaperEvent::Withdrawn,
            PaperState::NoIssue,
        ),
        (
            PaperState::NoIssue,
            PaperEvent::FetchFailed,
            PaperState::NoIssue,
        ),
        (
            holding.clone(),
            small(20.0),
            PaperState::Holding {
                issue: fixture(),
                held_at: 20.0,
            },
        ),
        (holding.clone(), PaperEvent::Withdrawn, PaperState::NoIssue),
        (holding.clone(), PaperEvent::FetchFailed, holding.clone()),
    ];
    for (from, event, to) in rows {
        let label = format!("{} + {event:?}", from.status());
        assert_eq!(from.next(event), to, "{label}");
    }
}

#[test]
fn a_refresh_is_due_with_nothing_held_or_after_five_minutes() {
    assert!(PaperState::NoIssue.refresh_due(0.0));
    assert_eq!(REFRESH_AFTER_S, 300.0);
    let held = PaperState::NoIssue.next(small(1000.0));
    assert!(!held.refresh_due(1000.0 + REFRESH_AFTER_S));
    assert!(held.refresh_due(1000.0 + REFRESH_AFTER_S + 1.0));
    assert_eq!(held.status(), "holding issue 25");
    assert_eq!(PaperState::NoIssue.status(), "no_issue");
}

fn body(sections: Value) -> Value {
    json!({
        "version": 1, "issue": 3, "masthead": "The Mile Marker",
        "dates": "November 16 to 22, 2026", "sections": sections,
    })
}

#[test]
fn bodies_it_refuses() {
    let good = body(json!([{ "heading": "Front page", "lines": ["One."] }]));
    assert!(parse_issue_value(&good).is_ok());

    let mut other = good.clone();
    other["version"] = json!(2);
    assert_eq!(parse_issue_value(&other), Err(PaperRefusal::Version));
    assert_eq!(parse_issue("not json"), Err(PaperRefusal::NotJson));

    let long = "a".repeat(MAX_LINE_CHARS + 1);
    let too_long = body(json!([{ "heading": "Front page", "lines": [long] }]));
    assert_eq!(parse_issue_value(&too_long), Err(PaperRefusal::LineTooLong));
    let fits = body(json!([{ "heading": "Front page", "lines": ["é".repeat(MAX_LINE_CHARS)] }]));
    assert!(
        parse_issue_value(&fits).is_ok(),
        "the limit is characters, not bytes"
    );

    let many: Vec<String> = (0..=MAX_ISSUE_LINES)
        .map(|n| format!("Line {n}."))
        .collect();
    let too_many = body(json!([{ "heading": "Front page", "lines": many }]));
    assert_eq!(
        parse_issue_value(&too_many),
        Err(PaperRefusal::TooManyLines)
    );

    for broken in [
        body(json!([])),
        body(json!([{ "heading": "Front page", "lines": [] }])),
        body(json!([{ "heading": "Front page", "lines": [""] }])),
        body(json!([{ "heading": "Front page", "lines": ["Two\nlines."] }])),
        body(json!([{ "heading": "Front page", "lines": [3] }])),
        body(json!([{ "lines": ["One."] }])),
        body(json!("Front page")),
    ] {
        assert!(
            matches!(parse_issue_value(&broken), Err(PaperRefusal::Shape(_))),
            "{broken}"
        );
    }
    let mut no_issue = good.clone();
    no_issue["issue"] = json!(0);
    assert!(parse_issue_value(&no_issue).is_err());
}
