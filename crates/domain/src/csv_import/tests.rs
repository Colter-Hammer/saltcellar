use super::*;

fn date(text: &str) -> Date {
    text.parse().unwrap()
}

fn records(text: &str) -> Vec<Vec<String>> {
    parse(text)
}

#[test]
fn parse_reads_quotes_line_breaks_and_the_delimiter() {
    assert_eq!(
        parse("\u{feff}a,\"b,1\",\"say \"\"hi\"\"\"\r\n1,\"two\nlines\",3\n\n4,5,6"),
        vec![
            vec!["a", "b,1", "say \"hi\""],
            vec!["1", "two\nlines", "3"],
            vec![""],
            vec!["4", "5", "6"],
        ]
    );
    assert_eq!(
        parse("a;b\n1;2\r3;4"),
        vec![vec!["a", "b"], vec!["1", "2"], vec!["3", "4"]]
    );
    assert_eq!(parse("a\tb\n1\t2\n"), vec![vec!["a", "b"], vec!["1", "2"]]);
    assert_eq!(parse("one\ntwo"), vec![vec!["one"], vec!["two"]]);
    assert_eq!(parse("a,b,"), vec![vec!["a", "b", ""]]);
    assert_eq!(parse("").len(), 0);
}

#[test]
fn dates_read_in_each_format() {
    use DateFormat::{DayMonthYear, MonthDayYear, YearMonthDay};
    assert_eq!(YearMonthDay.parse("2026-03-04"), Some(date("2026-03-04")));
    assert_eq!(
        YearMonthDay.parse(" 2026/3/4 12:30"),
        Some(date("2026-03-04"))
    );
    assert_eq!(
        YearMonthDay.parse("2026-03-04T10:00:00"),
        Some(date("2026-03-04"))
    );
    assert_eq!(MonthDayYear.parse("03/04/2026"), Some(date("2026-03-04")));
    assert_eq!(MonthDayYear.parse("3.4.26"), Some(date("2026-03-04")));
    assert_eq!(DayMonthYear.parse("04/03/2026"), Some(date("2026-03-04")));
    assert_eq!(DayMonthYear.parse("31/12/25"), Some(date("2025-12-31")));
    for (format, text) in [
        (YearMonthDay, "26-03-04"),
        (YearMonthDay, "2026-03"),
        (MonthDayYear, "13/01/2026"),
        (MonthDayYear, "03/04/202"),
        (MonthDayYear, "ab/04/2026"),
        (MonthDayYear, "-3/04/2026"),
        (MonthDayYear, "99999999999/04/2026"),
        (MonthDayYear, "03/99999/2026"),
        (MonthDayYear, "03/04/0000"),
        (MonthDayYear, ""),
        (MonthDayYear, "/04/2026"),
    ] {
        assert_eq!(format.parse(text), None, "{text}");
    }
}

#[test]
fn the_date_format_is_guessed_from_the_rows() {
    use DateFormat::{DayMonthYear, MonthDayYear, YearMonthDay};
    assert_eq!(
        DateFormat::guess(["2026-03-04", "", "2026-03-05"]),
        YearMonthDay
    );
    assert_eq!(
        DateFormat::guess(["03/04/2026", "03/25/2026"]),
        MonthDayYear
    );
    assert_eq!(
        DateFormat::guess(["04/03/2026", "25/03/2026"]),
        DayMonthYear
    );
    // Both fit: month first, for the household to confirm.
    assert_eq!(DateFormat::guess(["03/04/2026"]), MonthDayYear);
    // None fits, or there's nothing to go on.
    assert_eq!(DateFormat::guess(["soon"]), MonthDayYear);
    assert_eq!(DateFormat::guess([]), MonthDayYear);
}

#[test]
fn amounts_read_with_signs_symbols_and_separators() {
    let usd = |text: &str| parse_amount(text, 2).map(Money::minor);
    assert_eq!(usd("12.34"), Some(1234));
    assert_eq!(usd("-12.3"), Some(-1230));
    assert_eq!(usd("12.34-"), Some(-1234));
    assert_eq!(usd("\u{2212}5"), Some(-500));
    assert_eq!(usd("(5.00)"), Some(-500));
    assert_eq!(usd("+$1,234.50"), Some(123_450));
    assert_eq!(usd("1.234,50 \u{20ac}"), Some(123_450));
    assert_eq!(usd("12,34"), Some(1234));
    assert_eq!(usd("1,234"), Some(123_400));
    assert_eq!(usd("1,234,567"), Some(123_456_700));
    assert_eq!(usd(".5"), Some(50));
    assert_eq!(usd("5."), Some(500));
    for text in [
        "",
        "abc",
        "-",
        "1,2.345",
        "12abc",
        "9999999999999999999",
        "99999999999999999",
    ] {
        assert_eq!(usd(text), None, "{text}");
    }
    assert_eq!(parse_amount("1,234", 0).map(Money::minor), Some(1234));
    assert_eq!(parse_amount("1.234", 3).map(Money::minor), Some(1234));
    assert_eq!(parse_amount("5", 40), None);
}

fn file() -> Vec<Vec<String>> {
    records(
        "Date,Post Date,Description,Debit,Credit,Category,Memo\n\
         03/04/2026,03/05/2026,Coffee,4.50,,Dining,latte\n\
         03/06/2026,,Paycheck,,1000.00,Income,\n\
         \n\
         not a date,,Bad,1.00,,,\n\
         03/07/2026,,Bad amount,x,,,\n",
    )
}

#[test]
fn a_header_suggests_the_mapping() {
    let mapping = guess_mapping(&file());
    assert_eq!(
        mapping,
        Mapping {
            date: 0,
            post_date: Some(1),
            description: 2,
            amount: AmountColumns::DebitCredit {
                debit: 3,
                credit: 4
            },
            flip_sign: false,
            date_format: DateFormat::MonthDayYear,
            category: Some(5),
            memo: Some(6),
            skip_rows: 1,
        }
    );
    let signed = guess_mapping(&records("Posted,Payee,Amount\n2026-03-04,Shop,-1.00\n"));
    assert_eq!(signed.amount, AmountColumns::Signed(2));
    assert_eq!(signed.date, 0);
    assert_eq!(signed.post_date, None);
    assert_eq!(signed.date_format, DateFormat::YearMonthDay);
    // A header that says nothing falls back to the first three columns; no header at all too.
    let blind = guess_mapping(&records("a,b,c\n1,2,3\n"));
    assert_eq!(
        (blind.date, blind.description, blind.amount),
        (0, 1, AmountColumns::Signed(2))
    );
    let empty = guess_mapping(&[]);
    assert_eq!(empty.date, 0);
    // Lines of title above the header are skipped.
    let titled = guess_mapping(&records(
        "Demo export\n\nDate,Description,Amount\n2026-03-04,x,1\n",
    ));
    assert_eq!(
        (titled.skip_rows, titled.date, titled.date_format),
        (3, 0, DateFormat::YearMonthDay)
    );
    // Only a post date is the date.
    let post_only = guess_mapping(&records("Post date,Description,Amount\n2026-03-04,x,1\n"));
    assert_eq!((post_only.date, post_only.post_date), (0, None));
}

#[test]
fn the_header_signature_ignores_case_and_spacing_but_not_columns() {
    let a = records("Date, Amount\n1,2");
    let b = records("date,amount\n3,4");
    let c = records("date,amount,memo\n3,4,5");
    assert_eq!(header_signature(&a, 1), header_signature(&b, 1));
    assert_ne!(header_signature(&a, 1), header_signature(&c, 1));
    assert_ne!(header_signature(&a, 1), header_signature(&a, 2));
    assert_ne!(header_signature(&a, 0), header_signature(&c, 0));
    assert_eq!(header_signature(&[], 0), header_signature(&[], 3));
    assert_eq!(width(&c), 3);
    assert_eq!(width(&[]), 0);
}

#[test]
fn rows_are_read_through_the_mapping() {
    let records = file();
    let read = read_rows(&records, &guess_mapping(&records), 2);
    assert_eq!(read.rows.len(), 2);
    let coffee = &read.rows[0];
    assert_eq!(coffee.row, 2);
    assert_eq!(coffee.date, date("2026-03-04"));
    assert_eq!(coffee.post_date, Some(date("2026-03-05")));
    assert_eq!(coffee.description, "Coffee");
    assert_eq!(coffee.amount.minor(), -450);
    assert_eq!(coffee.category.as_deref(), Some("Dining"));
    assert_eq!(coffee.memo.as_deref(), Some("latte"));
    let pay = &read.rows[1];
    assert_eq!(
        (pay.amount.minor(), pay.post_date, pay.memo.clone()),
        (100_000, None, None)
    );
    assert_ne!(coffee.import_ref, pay.import_ref);
    assert_eq!(
        read.unreadable,
        vec![
            Unreadable {
                row: 5,
                why: "its date doesn't read"
            },
            Unreadable {
                row: 6,
                why: "its amount doesn't read"
            },
        ]
    );
}

#[test]
fn a_signed_column_flips_and_short_records_read_as_empty() {
    let records = records("2026-03-04,Shop,5.00\n2026-03-05,Short\n");
    let mut mapping = Mapping {
        date: 0,
        post_date: None,
        description: 1,
        amount: AmountColumns::Signed(2),
        flip_sign: true,
        date_format: DateFormat::YearMonthDay,
        category: None,
        memo: None,
        skip_rows: 0,
    };
    let read = read_rows(&records, &mapping, 2);
    assert_eq!(read.rows[0].amount.minor(), -500);
    assert_eq!(read.unreadable.len(), 1);
    mapping.flip_sign = false;
    assert_eq!(read_rows(&records, &mapping, 2).rows[0].amount.minor(), 500);
    // Debit and credit written with signs of their own are sizes.
    mapping.amount = AmountColumns::DebitCredit {
        debit: 2,
        credit: 3,
    };
    assert_eq!(
        read_rows(&records, &mapping, 2).rows[0].amount.minor(),
        -500
    );
}

fn row(n: usize, day: &str, amount: i64, text: &str) -> FileRow {
    FileRow {
        row: n,
        date: date(day),
        post_date: None,
        description: text.to_owned(),
        amount: Money::from_minor(amount),
        category: None,
        memo: None,
        import_ref: format!("ref{n}"),
    }
}

fn stored(id: i64, day: &str, amount: i64, text: &str) -> Stored {
    Stored {
        id,
        date: date(day),
        amount: Money::from_minor(amount),
        raw_description: text.to_owned(),
    }
}

fn outcomes(rows: Vec<FileRow>, seen: &HashMap<String, u64>, stored: &[Stored]) -> Vec<Outcome> {
    plan(rows, seen, stored)
        .into_iter()
        .map(|e| e.outcome)
        .collect()
}

#[test]
fn rows_match_on_amount_and_a_window_of_days() {
    let held = [stored(1, "2026-03-04", -450, "COFFEE SHOP #12")];
    let none = HashMap::new();
    let near = |day: &str, amount: i64| outcomes(vec![row(1, day, amount, "Coffee")], &none, &held);
    assert_eq!(near("2026-03-04", -450), [Outcome::Matched(1)]);
    assert_eq!(near("2026-03-07", -450), [Outcome::Matched(1)]);
    assert_eq!(near("2026-03-01", -450), [Outcome::Matched(1)]);
    assert_eq!(near("2026-03-08", -450), [Outcome::New]);
    assert_eq!(near("2026-03-04", -451), [Outcome::New]);
    // The post date counts too.
    let mut late = row(1, "2026-03-10", -450, "Coffee");
    late.post_date = Some(date("2026-03-05"));
    assert_eq!(outcomes(vec![late], &none, &held), [Outcome::Matched(1)]);
}

#[test]
fn repeats_are_counted_and_ties_are_left_to_the_household() {
    let none = HashMap::new();
    // Two identical rows in the file, one stored: one matches, one is new.
    let one = [stored(1, "2026-03-04", -450, "Coffee")];
    let twice = vec![
        row(1, "2026-03-04", -450, "Coffee"),
        row(2, "2026-03-04", -450, "Coffee"),
    ];
    assert_eq!(
        outcomes(twice.clone(), &none, &one),
        [Outcome::Matched(1), Outcome::New]
    );
    // Two identical stored copies take one each.
    let two = [
        stored(1, "2026-03-04", -450, "Coffee"),
        stored(2, "2026-03-04", -450, "Coffee"),
    ];
    assert_eq!(
        outcomes(twice, &none, &two),
        [Outcome::Matched(1), Outcome::Matched(2)]
    );
    // The nearest date breaks a tie; the description breaks a tie of dates.
    let near = [
        stored(1, "2026-03-02", -450, "Coffee"),
        stored(2, "2026-03-04", -450, "Coffee"),
    ];
    assert_eq!(
        outcomes(vec![row(1, "2026-03-04", -450, "x")], &none, &near),
        [Outcome::Matched(2)]
    );
    let same_day = [
        stored(1, "2026-03-04", -450, "Gas station"),
        stored(2, "2026-03-04", -450, "Coffee shop"),
    ];
    assert_eq!(
        outcomes(
            vec![row(1, "2026-03-04", -450, "Coffee shop")],
            &none,
            &same_day
        ),
        [Outcome::Matched(2)]
    );
    // Equally near, equally unlike: a possible duplicate of the oldest, which is taken; the next
    // row has only the other left.
    let tied = [
        stored(1, "2026-03-04", -450, "Gas"),
        stored(2, "2026-03-04", -450, "Food"),
    ];
    let rows = vec![
        row(1, "2026-03-04", -450, "Other"),
        row(2, "2026-03-04", -450, "Other"),
    ];
    assert_eq!(
        outcomes(rows, &none, &tied),
        [Outcome::Check(1), Outcome::Matched(2)]
    );
}

#[test]
fn a_line_seen_before_is_already_there() {
    let rows = vec![
        row(1, "2026-03-04", -450, "a"),
        row(2, "2026-03-05", -450, "a"),
        row(3, "2026-03-06", -1, "b"),
    ];
    let mut seen = HashMap::new();
    seen.insert("ref1".to_owned(), 1);
    seen.insert("ref2".to_owned(), 0);
    let held = [stored(9, "2026-03-05", -450, "a")];
    let entries = plan(rows, &seen, &held);
    assert_eq!(
        entries
            .iter()
            .map(|e| e.outcome.clone())
            .collect::<Vec<_>>(),
        [Outcome::Seen, Outcome::Matched(9), Outcome::New]
    );
    assert_eq!(
        counts(&entries),
        Counts {
            new: 1,
            already: 2,
            check: 0
        }
    );
    let checks = plan(
        vec![row(1, "2026-03-04", -450, "z")],
        &HashMap::new(),
        &[
            stored(1, "2026-03-04", -450, "a"),
            stored(2, "2026-03-04", -450, "b"),
        ],
    );
    assert_eq!(
        counts(&checks),
        Counts {
            new: 0,
            already: 0,
            check: 1
        }
    );
}

#[test]
fn the_window_to_load_surrounds_every_date() {
    assert_eq!(window(&[]), None);
    let mut late = row(2, "2026-03-20", 1, "x");
    late.post_date = Some(date("2026-03-25"));
    assert_eq!(
        window(&[row(1, "2026-03-10", 1, "x"), late]),
        Some((date("2026-03-07"), date("2026-03-28")))
    );
}
