use super::*;
use crate::Money;

const V1: &str = include_str!("../../../../fixtures/bank-v1.qfx");
const V2: &str = include_str!("../../../../fixtures/bank-v2.ofx");

fn day(text: &str) -> Date {
    text.parse().unwrap()
}

fn one(content: &str) -> Statement {
    let mut statements = parse(content, 2).unwrap();
    assert_eq!(statements.len(), 1);
    statements.remove(0)
}

type Summary = (
    usize,
    Date,
    Option<Date>,
    i64,
    String,
    Option<String>,
    String,
);

fn summary(statement: &Statement) -> Vec<Summary> {
    statement
        .read
        .rows
        .iter()
        .map(|row| {
            (
                row.row,
                row.date,
                row.post_date,
                row.amount.minor(),
                row.description.clone(),
                row.memo.clone(),
                row.import_ref.clone(),
            )
        })
        .collect()
}

#[test]
fn the_sgml_fixture_reads_with_its_amounts_dates_and_descriptions() {
    let statement = one(V1);
    assert_eq!(statement.account_id, "4000000000000042");
    assert_eq!(statement.read.unreadable, vec![]);
    assert_eq!(
        summary(&statement),
        vec![
            (
                1,
                day("2026-09-03"),
                Some(day("2026-09-02")),
                -1234,
                "DEMO GROCER & DELI".into(),
                None,
                "DEMO-V1-0001".into()
            ),
            (
                2,
                day("2026-09-10"),
                None,
                -4500,
                "DEMO FUEL STOP".into(),
                Some("PUMP 4".into()),
                "DEMO-V1-0002".into()
            ),
            (
                3,
                day("2026-09-20"),
                None,
                20000,
                "PAYMENT THANK YOU".into(),
                None,
                "DEMO-V1-0003".into()
            ),
        ]
    );
}

#[test]
fn the_xml_fixture_reads_with_payees_and_ignores_the_account_a_transfer_goes_to() {
    let statement = one(V2);
    assert_eq!(statement.account_id, "000000000077");
    let rows = summary(&statement);
    assert_eq!(rows.len(), 4);
    assert_eq!(
        rows[0],
        (
            1,
            day("2026-09-01"),
            None,
            150_000,
            "DEMO PAYROLL".into(),
            None,
            "DEMO-V2-0001".into()
        )
    );
    // A check's payee names it; its memo stays the memo.
    assert_eq!(rows[1].4, "DEMO LANDSCAPING");
    assert_eq!(rows[1].5.as_deref(), Some("CHECK 101"));
    assert_eq!(rows[1].3, -8025);
    assert_eq!(rows[2].4, "TRANSFER TO SAVINGS");
    assert_eq!(rows[3].3, -450);
}

#[test]
fn a_file_without_an_ofx_element_is_refused() {
    assert_eq!(parse("Date,Amount\n2026-01-01,1", 2), Err(NotOfx));
    assert_eq!(parse("OFXHEADER:100\nDATA:OFXSGML\n", 2), Err(NotOfx));
    assert_eq!(parse("<ofx></ofx>", 2), Ok(vec![]));
}

#[test]
fn unreadable_transactions_are_listed_and_a_missing_fitid_gets_a_steady_hash() {
    let file = "<OFX><BANKMSGSRSV1><STMTTRNRS><STMTRS><BANKTRANLIST>
        <STMTTRN><DTPOSTED>2026-09-01<TRNAMT>1.00<FITID>A</STMTTRN>
        <STMTTRN><DTPOSTED>20260901<TRNAMT>lots<FITID>B</STMTTRN>
        <STMTTRN><DTPOSTED>20260902<TRNAMT>-3,50<MEMO>ONLY A MEMO</STMTTRN>
        </BANKTRANLIST></STMTRS></STMTTRNRS></BANKMSGSRSV1></OFX>";
    let statement = one(file);
    assert_eq!(statement.account_id, "");
    assert_eq!(
        statement.read.unreadable,
        vec![
            Unreadable {
                row: 1,
                why: "its date doesn't read"
            },
            Unreadable {
                row: 2,
                why: "its amount doesn't read"
            },
        ]
    );
    let [row] = statement.read.rows.as_slice() else {
        panic!("one row")
    };
    assert_eq!(row.row, 3);
    assert_eq!(row.amount, Money::from_minor(-350));
    assert_eq!(row.description, "ONLY A MEMO");
    assert_eq!(row.memo, None);
    assert_eq!(row.import_ref.len(), 64);
    assert_eq!(one(file).read.rows[0].import_ref, row.import_ref);
}

#[test]
fn several_statements_come_back_one_each() {
    let file = "<OFX><STMTRS><BANKACCTFROM><ACCTID>11110001</BANKACCTFROM>
        <STMTTRN><DTPOSTED>20260901<TRNAMT>1<FITID>A</STMTTRN></STMTRS>
        <CCSTMTRS><CCACCTFROM><ACCTID>22220002</CCACCTFROM></CCSTMTRS></OFX>";
    let statements = parse(file, 2).unwrap();
    assert_eq!(statements.len(), 2);
    assert_eq!(statements[0].account_id, "11110001");
    assert_eq!(statements[0].read.rows.len(), 1);
    assert_eq!(statements[1].account_id, "22220002");
    assert_eq!(statements[1].read.rows, vec![]);
}

#[test]
fn character_references_decode_and_unknown_ones_stay() {
    assert_eq!(decode("A &amp; B &lt;&gt; &#39;x&#x27;"), "A & B <> 'x'");
    assert_eq!(decode("AT&T &bogus; &"), "AT&T &bogus; &");
}

#[test]
fn dates_take_the_day_and_ignore_the_time_and_zone() {
    assert_eq!(date("20260903120000.000[-5:EST]"), Some(day("2026-09-03")));
    assert_eq!(date("20260230"), None);
    assert_eq!(date("2026090"), None);
}

#[test]
fn the_last_four_come_from_the_last_long_run_of_digits() {
    assert_eq!(last_four("4000000000000042").as_deref(), Some("0042"));
    assert_eq!(last_four("Card ··0042").as_deref(), Some("0042"));
    assert_eq!(
        last_four("Checking 1234 (joint 2)").as_deref(),
        Some("1234")
    );
    assert_eq!(last_four("Savings 12"), None);
    assert_eq!(last_four(""), None);
}

#[test]
fn an_account_is_preselected_only_when_one_name_ends_in_the_digits() {
    let accounts = [
        (1, "Demo card 0042"),
        (2, "Demo checking 0077"),
        (3, "Cash"),
    ];
    assert_eq!(preselect("0042", accounts), Some(1));
    assert_eq!(preselect("9999", accounts), None);
    assert_eq!(
        preselect("0042", [(1, "Card 0042"), (2, "Old card 0042")]),
        None
    );
}

#[test]
fn comments_empty_tags_and_look_alike_tags_are_passed_over() {
    let file = "<OFXLIKE>\n<ofx>\n<!-- a comment --><STMTRS><BANKID/>
        <BANKACCTFROM><ACCTID>12345678</ACCTID></BANKACCTFROM></STMTRS></ofx>";
    let statement = one(file);
    assert_eq!(statement.account_id, "12345678");
    assert_eq!(statement.read.rows, vec![]);
    assert_eq!(parse("<OFXLIKE>", 2), Err(NotOfx));
}
