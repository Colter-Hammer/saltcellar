use super::*;
use crate::import::Disposition;

fn date(text: &str) -> Date {
    text.parse().unwrap()
}

fn row(id: &str, day: &str, minor: i64, text: &str, bank_id: Option<&str>) -> Row {
    Row {
        import_ref: id.to_owned(),
        date: date(day),
        amount: Money::from_minor(minor),
        raw_description: text.to_owned(),
        external_id: bank_id.map(str::to_owned),
        payee: None,
        alias: None,
        memo: None,
        tags: Vec::new(),
        disposition: Disposition::Uncategorized,
        source_category: None,
    }
}

fn stored(id: i64, day: &str, minor: i64, text: &str, bank_id: Option<&str>) -> StoredRow {
    StoredRow {
        id,
        external_id: bank_id.map(str::to_owned),
        actual_id: None,
        date: date(day),
        amount: Money::from_minor(minor),
        raw_description: text.to_owned(),
    }
}

#[test]
fn the_same_bank_id_is_the_same_row_whatever_its_date_or_amount() {
    let rows = [row("a1", "2026-01-02", -500, "CAFE", Some("TRN-1"))];
    let stored = [stored(10, "2026-10-20", -650, "DEMO CAFE", Some("TRN-1"))];
    assert_eq!(
        overlap(&rows, &stored, Some(date("2026-10-01"))),
        [Overlap::SameId(10)]
    );
    // Even with no synced period.
    assert_eq!(overlap(&rows, &stored, None), [Overlap::SameId(10)]);
}

#[test]
fn a_row_an_earlier_import_brought_is_seen() {
    let rows = [
        row("a1", "2025-03-01", -500, "CAFE", None),
        row("a2", "2025-03-02", -700, "DINER", Some("TRN-2")),
    ];
    let mut first = stored(10, "2025-03-01", -500, "CAFE", None);
    first.actual_id = Some("a1".to_owned());
    let mut second = stored(11, "2025-03-02", -700, "DINER", Some("TRN-2"));
    second.actual_id = Some("a2".to_owned());
    assert_eq!(
        overlap(&rows, &[first, second], None),
        [Overlap::Seen(10), Overlap::Seen(11)]
    );
}

#[test]
fn without_an_id_the_file_rule_applies_inside_the_synced_period() {
    let rows = [
        // Two days off, same amount: the same row.
        row("a1", "2026-10-10", -4_125, "FUEL STOP", None),
        // No stored row of its amount: new.
        row("a2", "2026-10-11", -999, "BOOKSHOP", None),
        // Before the synced period: inserted, though a stored row would match.
        row("a3", "2026-09-28", -1_000, "GYM", None),
    ];
    let stored = [
        stored(10, "2026-10-12", -4_125, "DEMO FUEL STOP", Some("TRN-1")),
        stored(11, "2026-09-29", -1_000, "GYM", Some("TRN-2")),
    ];
    assert_eq!(
        overlap(&rows, &stored, Some(date("2026-10-01"))),
        [Overlap::Matched(10), Overlap::New, Overlap::New]
    );
}

#[test]
fn repeats_are_counted_and_ties_are_checked() {
    let rows = [
        row("a1", "2026-10-10", -600, "PARKING", None),
        row("a2", "2026-10-10", -600, "PARKING", None),
        row("a3", "2026-10-10", -600, "PARKING", None),
        // Two stored rows as good as each other.
        row("a4", "2026-10-20", -300, "SNACK", None),
    ];
    let stored = [
        stored(10, "2026-10-10", -600, "PARKING", Some("TRN-1")),
        stored(11, "2026-10-10", -600, "PARKING", Some("TRN-2")),
        stored(12, "2026-10-19", -300, "VENDING", Some("TRN-3")),
        stored(13, "2026-10-21", -300, "KIOSK", Some("TRN-4")),
    ];
    assert_eq!(
        overlap(&rows, &stored, Some(date("2026-10-01"))),
        [
            Overlap::Matched(10),
            Overlap::Matched(11),
            Overlap::New,
            Overlap::Check(12),
        ]
    );
}

#[test]
fn a_stored_row_taken_by_id_is_not_matched_again() {
    let rows = [
        row("a1", "2026-10-10", -600, "PARKING", Some("TRN-1")),
        row("a2", "2026-10-10", -600, "PARKING", None),
    ];
    let stored = [stored(10, "2026-10-10", -600, "PARKING", Some("TRN-1"))];
    assert_eq!(
        overlap(&rows, &stored, Some(date("2026-10-01"))),
        [Overlap::SameId(10), Overlap::New]
    );
}
