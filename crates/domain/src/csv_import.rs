//! CSV import (docs/spec/m2-bank-data.md#csv-and-ofxqfx-import): reading a file's records, the
//! column mapping that turns them into rows, and matching those rows against the ones an account
//! already has.
//!
//! Pure: the caller loads the stored rows and writes what comes back.

use std::collections::{BTreeSet, HashMap};
use std::fmt::Write;
use std::hash::BuildHasher;

use sha2::{Digest, Sha256};

use crate::import::normalize;
use crate::matching::similar_description;
use crate::{Date, Money};

/// Days either side of a file row's date that a stored row may sit and still be the same one.
pub const WINDOW_DAYS: i32 = 3;

/// Reads `content` as CSV: a comma, semicolon or tab between cells *(agent: whichever the first
/// ten lines use most)*, quotes around a cell that holds the delimiter, a quote or a line break
/// (`""` for a quote inside), and a leading byte-order mark ignored. Every record is returned,
/// blank ones too, so a record's place in the file stays its place here.
#[must_use]
pub fn parse(content: &str) -> Vec<Vec<String>> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let delimiter = sniff(content);
    let mut records: Vec<Vec<String>> = Vec::new();
    let mut cells: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = content.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                chars.next();
                cell.push('"');
            }
            '"' if quoted => quoted = false,
            '"' if cell.is_empty() => quoted = true,
            c if c == delimiter && !quoted => cells.push(std::mem::take(&mut cell)),
            '\r' if !quoted => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                cells.push(std::mem::take(&mut cell));
                records.push(std::mem::take(&mut cells));
            }
            '\n' if !quoted => {
                cells.push(std::mem::take(&mut cell));
                records.push(std::mem::take(&mut cells));
            }
            c => cell.push(c),
        }
    }
    if !cell.is_empty() || !cells.is_empty() {
        cells.push(cell);
        records.push(cells);
    }
    records
}

fn sniff(content: &str) -> char {
    let mut best = (',', 0);
    for candidate in [',', ';', '\t'] {
        let count: usize = content
            .lines()
            .take(10)
            .map(|line| line.matches(candidate).count())
            .sum();
        if count > best.1 {
            best = (candidate, count);
        }
    }
    best.0
}

/// How a file writes its dates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateFormat {
    /// `2026-03-04`.
    YearMonthDay,
    /// `03/04/2026`.
    MonthDayYear,
    /// `04/03/2026`.
    DayMonthYear,
}

impl DateFormat {
    /// `text` as a date in this format, with `-`, `/` or `.` between the parts, a year of 4
    /// digits or 2 *(agent: 2000 and on)*, and a time after the date ignored.
    #[must_use]
    pub fn parse(self, text: &str) -> Option<Date> {
        let day_part = text.trim().split(['T', ' ']).next()?;
        let parts: Vec<&str> = day_part.split(['-', '/', '.']).collect();
        let [first, second, third] = parts.as_slice() else {
            return None;
        };
        let (year, month, day) = match self {
            DateFormat::YearMonthDay => (*first, *second, *third),
            DateFormat::MonthDayYear => (*third, *first, *second),
            DateFormat::DayMonthYear => (*third, *second, *first),
        };
        let year = match (year.len(), self) {
            (4, _) => number(year)?,
            (2, DateFormat::MonthDayYear | DateFormat::DayMonthYear) => 2000 + number(year)?,
            _ => return None,
        };
        Date::from_ymd(
            i16::try_from(year).ok()?,
            i8::try_from(number(month)?).ok()?,
            i8::try_from(number(day)?).ok()?,
        )
        .ok()
    }

    /// The format every one of `samples` reads in, the year-first one before month-first before
    /// day-first; month-first when none fits, for the household to confirm.
    #[must_use]
    pub fn guess<'a>(samples: impl IntoIterator<Item = &'a str>) -> DateFormat {
        let samples: Vec<&str> = samples
            .into_iter()
            .filter(|sample| !sample.trim().is_empty())
            .collect();
        [
            DateFormat::YearMonthDay,
            DateFormat::MonthDayYear,
            DateFormat::DayMonthYear,
        ]
        .into_iter()
        .find(|format| {
            !samples.is_empty() && samples.iter().all(|sample| format.parse(sample).is_some())
        })
        .unwrap_or(DateFormat::MonthDayYear)
    }
}

fn number(text: &str) -> Option<i32> {
    if text.is_empty() || !text.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// `text` as an amount in an account of `places` decimal places: a leading or trailing `-` or
/// parentheses make it negative; currency signs, spaces and thousands separators are ignored. A
/// lone `.` or `,` is a decimal point when at most `places` digits follow it, else a thousands
/// separator; with both, the later one is the decimal point *(agent)*.
#[must_use]
pub fn parse_amount(text: &str, places: u8) -> Option<Money> {
    let text = text.trim();
    let negative = text.starts_with(['-', '\u{2212}'])
        || text.ends_with(['-', '\u{2212}'])
        || (text.starts_with('(') && text.ends_with(')'));
    let mut digits = String::new();
    for c in text.chars() {
        match c {
            '0'..='9' | '.' | ',' => digits.push(c),
            '-' | '\u{2212}' | '+' | '(' | ')' | ' ' | '\u{a0}' | '$' | '\u{20ac}' | '\u{a3}'
            | '\u{a5}' => {}
            _ => return None,
        }
    }
    if !digits.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    let decimal_at = match (digits.rfind('.'), digits.rfind(',')) {
        (Some(dot), Some(comma)) => Some(dot.max(comma)),
        (Some(at), None) => lone_decimal(&digits, '.', at, places),
        (None, Some(at)) => lone_decimal(&digits, ',', at, places),
        (None, None) => None,
    };
    let (whole, fraction) = match decimal_at {
        Some(at) => (digits.get(..at)?, digits.get(at + 1..)?),
        None => (digits.as_str(), ""),
    };
    let whole: String = whole.chars().filter(char::is_ascii_digit).collect();
    if fraction.len() > usize::from(places) || !fraction.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let scale = 10_i64.checked_pow(u32::from(places))?;
    let whole: i64 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    let fraction_scale = 10_i64.pow(u32::from(places) - u32::try_from(fraction.len()).ok()?);
    let fraction: i64 = if fraction.is_empty() {
        0
    } else {
        fraction.parse::<i64>().ok()?.checked_mul(fraction_scale)?
    };
    let minor = whole.checked_mul(scale)?.checked_add(fraction)?;
    Some(Money::from_minor(if negative { -minor } else { minor }))
}

/// The position of `separator` in `digits` when it's the only one and a decimal point.
fn lone_decimal(digits: &str, separator: char, at: usize, places: u8) -> Option<usize> {
    let after = digits.len() - at - 1;
    (digits.matches(separator).count() == 1 && after <= usize::from(places)).then_some(at)
}

/// Where an amount is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmountColumns {
    /// One column, signed from the account's point of view.
    Signed(usize),
    /// Money out in one, money in the other, both written without a sign.
    DebitCredit { debit: usize, credit: usize },
}

/// Which column holds what, saved per account by header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mapping {
    pub date: usize,
    pub post_date: Option<usize>,
    pub description: usize,
    pub amount: AmountColumns,
    /// The file writes money out as positive.
    pub flip_sign: bool,
    pub date_format: DateFormat,
    pub category: Option<usize>,
    pub memo: Option<usize>,
    /// Records before the first row, the header among them *(agent: 1 is a file with one header
    /// line, 0 a file with none)*.
    pub skip_rows: usize,
}

/// A hash of the header record `skip_rows` records in, so a file with the same header finds the
/// mapping saved for it; without a header (`skip_rows` 0) the width of the first record stands in.
#[must_use]
pub fn header_signature(records: &[Vec<String>], skip_rows: usize) -> String {
    let header = skip_rows.checked_sub(1).and_then(|at| records.get(at));
    let text = match header {
        Some(cells) => cells
            .iter()
            .map(|cell| cell.trim().to_lowercase())
            .collect::<Vec<_>>()
            .join("\u{1f}"),
        None => format!("no header\u{1f}{}", records.first().map_or(0, Vec::len)),
    };
    hex(&Sha256::digest(text.as_bytes()))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// The mapping a file's header suggests: columns by their names, the date format from the rows.
#[must_use]
pub fn guess_mapping(records: &[Vec<String>]) -> Mapping {
    // The header is the first record that names a date, within the first ten: lines of title
    // above it are skipped *(agent)*.
    let at = records
        .iter()
        .take(10)
        .position(|cells| {
            cells
                .iter()
                .any(|cell| cell.to_lowercase().contains("date"))
        })
        .unwrap_or(0);
    let header: Vec<String> = records
        .get(at)
        .map(|cells| {
            cells
                .iter()
                .map(|cell| cell.trim().to_lowercase())
                .collect()
        })
        .unwrap_or_default();
    let mut taken: BTreeSet<usize> = BTreeSet::new();
    let mut find = |wanted: &dyn Fn(&str) -> bool| {
        let at = header
            .iter()
            .enumerate()
            .find(|(at, name)| !taken.contains(at) && wanted(name))
            .map(|(at, _)| at);
        taken.extend(at);
        at
    };
    let post_date = find(&|name| name.contains("post") && name.contains("date"));
    let date = find(&|name| name.contains("date"));
    let description = find(&|name| {
        ["description", "payee", "details", "narrative", "name"]
            .iter()
            .any(|word| name.contains(word))
    });
    let debit = find(&|name| name.contains("debit") || name.contains("withdrawal"));
    let credit = find(&|name| name.contains("credit") || name.contains("deposit"));
    let signed = find(&|name| name.contains("amount"));
    let category = find(&|name| name.contains("category"));
    let memo = find(&|name| name.contains("memo") || name.contains("note"));
    let amount = match (signed, debit, credit) {
        (Some(column), _, _) => AmountColumns::Signed(column),
        (None, Some(debit), Some(credit)) => AmountColumns::DebitCredit { debit, credit },
        _ => AmountColumns::Signed(2),
    };
    let date = date.or(post_date).unwrap_or(0);
    let date_format = DateFormat::guess(
        records
            .iter()
            .skip(at + 1)
            .take(50)
            .filter_map(|cells| cells.get(date).map(String::as_str)),
    );
    Mapping {
        date,
        post_date: post_date.filter(|post| *post != date),
        description: description.unwrap_or(1),
        amount,
        flip_sign: false,
        date_format,
        category,
        memo,
        skip_rows: at + 1,
    }
}

/// A file row, read through a mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRow {
    /// Its record's place in the file, from 1.
    pub row: usize,
    pub date: Date,
    pub post_date: Option<Date>,
    pub description: String,
    pub amount: Money,
    pub category: Option<String>,
    pub memo: Option<String>,
    /// A hash of the raw cells: the same line is the same row on a later import.
    pub import_ref: String,
}

/// A record the mapping can't read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreadable {
    pub row: usize,
    pub why: &'static str,
}

/// The records of a file past its skipped ones, read through a mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Read {
    pub rows: Vec<FileRow>,
    /// Records with a date or amount that doesn't read: skipped.
    pub unreadable: Vec<Unreadable>,
}

/// The widest record: the columns a mapping may point at.
#[must_use]
pub fn width(records: &[Vec<String>]) -> usize {
    records.iter().map(Vec::len).max().unwrap_or(0)
}

/// Reads `records` through `mapping` for an account of `places` decimal places. Blank records are
/// passed over; a record whose date or amount doesn't read is listed as unreadable.
#[must_use]
pub fn read_rows(records: &[Vec<String>], mapping: &Mapping, places: u8) -> Read {
    let mut read = Read {
        rows: Vec::new(),
        unreadable: Vec::new(),
    };
    for (at, cells) in records.iter().enumerate().skip(mapping.skip_rows) {
        if cells.iter().all(|cell| cell.trim().is_empty()) {
            continue;
        }
        let row = at + 1;
        let cell = |column: usize| cells.get(column).map_or("", |text| text.trim());
        let Some(date) = mapping.date_format.parse(cell(mapping.date)) else {
            read.unreadable.push(Unreadable {
                row,
                why: "its date doesn't read",
            });
            continue;
        };
        let Some(amount) = amount(mapping, &cell, places) else {
            read.unreadable.push(Unreadable {
                row,
                why: "its amount doesn't read",
            });
            continue;
        };
        let text = |column: Option<usize>| {
            column
                .map(cell)
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        };
        read.rows.push(FileRow {
            row,
            date,
            post_date: mapping
                .post_date
                .and_then(|column| mapping.date_format.parse(cell(column))),
            description: cell(mapping.description).to_owned(),
            amount,
            category: text(mapping.category),
            memo: text(mapping.memo),
            import_ref: hex(&Sha256::digest(cells.join("\u{1f}").as_bytes())),
        });
    }
    read
}

fn amount<'a>(mapping: &Mapping, cell: &impl Fn(usize) -> &'a str, places: u8) -> Option<Money> {
    let amount = match mapping.amount {
        AmountColumns::Signed(column) => parse_amount(cell(column), places)?,
        AmountColumns::DebitCredit { debit, credit } => {
            let (debit, credit) = (
                parse_amount(cell(debit), places),
                parse_amount(cell(credit), places),
            );
            if debit.is_none() && credit.is_none() {
                return None;
            }
            let size = |amount: Option<Money>| amount.map_or(0, |amount| amount.minor().abs());
            Money::from_minor(size(credit).checked_sub(size(debit))?)
        }
    };
    if mapping.flip_sign {
        amount.checked_neg()
    } else {
        Some(amount)
    }
}

/// A row an account already has, as matching sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    pub id: i64,
    pub date: Date,
    pub amount: Money,
    pub raw_description: String,
}

/// The stored rows that can matter to `rows`, as a range of dates to load; `None` for no rows.
#[must_use]
pub fn window(rows: &[FileRow]) -> Option<(Date, Date)> {
    let dates = || {
        rows.iter()
            .flat_map(|row| std::iter::once(row.date).chain(row.post_date))
    };
    let (first, last) = (dates().min()?, dates().max()?);
    Some((
        first.plus_days(-WINDOW_DAYS).unwrap_or(first),
        last.plus_days(WINDOW_DAYS).unwrap_or(last),
    ))
}

/// What the import does with a file row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// An earlier import of this account has this very line: already there.
    Seen,
    /// The stored row with this id is it: already there. It can only gain the bank category.
    Matched(i64),
    /// It could be this stored row, or another as good: the household decides.
    Check(i64),
    /// Nothing there is it.
    New,
}

/// A file row and what becomes of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub row: FileRow,
    pub outcome: Outcome,
}

/// How many rows of each outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    pub new: u64,
    pub already: u64,
    pub check: u64,
}

/// Counts `entries` for "N new, M already there, K to check".
#[must_use]
pub fn counts(entries: &[Entry]) -> Counts {
    let mut counts = Counts::default();
    for entry in entries {
        match entry.outcome {
            Outcome::New => counts.new += 1,
            Outcome::Seen | Outcome::Matched(_) => counts.already += 1,
            Outcome::Check(_) => counts.check += 1,
        }
    }
    counts
}

/// Matches `rows` against the account's `stored` rows (synced, imported or manual, pending too).
///
/// - `seen`: for each import ref, how many rows of earlier imports hold it. A line seen as often
///   as it appears is already there; one that appears more often has the rest matched as usual
///   *(agent)*.
/// - Otherwise a stored row is the same when the amount is exact and its date is within
///   [`WINDOW_DAYS`] of the file row's date or post date. The nearest date wins, then a similar
///   description; copies of one stored row (same date and text) are taken one each, and a real
///   tie is a [`Outcome::Check`]. A stored row is taken at most once, so repeats are counted.
///
/// The entries come back in the file's order.
#[must_use]
pub fn plan<S: BuildHasher>(
    rows: Vec<FileRow>,
    seen: &HashMap<String, u64, S>,
    stored: &[Stored],
) -> Vec<Entry> {
    let mut left: HashMap<&str, u64> = seen.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    let mut outcomes: Vec<Option<Outcome>> = rows
        .iter()
        .map(|row| {
            let count = left
                .get_mut(row.import_ref.as_str())
                .filter(|count| **count > 0)?;
            *count -= 1;
            Some(Outcome::Seen)
        })
        .collect();
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by_key(|at| (rows[*at].date, rows[*at].row));
    let mut taken: BTreeSet<i64> = BTreeSet::new();
    for at in order {
        let (row, slot) = (&rows[at], &mut outcomes[at]);
        if slot.is_none() {
            let outcome = find(row, stored, &taken);
            if let Outcome::Matched(id) | Outcome::Check(id) = outcome {
                taken.insert(id);
            }
            *slot = Some(outcome);
        }
    }
    rows.into_iter()
        .zip(outcomes)
        .map(|(row, outcome)| Entry {
            row,
            outcome: outcome.unwrap_or(Outcome::New),
        })
        .collect()
}

fn find(row: &FileRow, stored: &[Stored], taken: &BTreeSet<i64>) -> Outcome {
    let distance = |s: &Stored| {
        let from = |date: Date| s.date.days_until(date).abs();
        row.post_date
            .map_or(from(row.date), |post| from(row.date).min(from(post)))
    };
    let rank = |s: &Stored| {
        (
            distance(s),
            !similar_description(&s.raw_description, &row.description),
        )
    };
    let mut candidates: Vec<&Stored> = stored
        .iter()
        .filter(|s| !taken.contains(&s.id) && s.amount == row.amount && distance(s) <= WINDOW_DAYS)
        .collect();
    candidates.sort_by_key(|s| (rank(s), s.id));
    let Some(best) = candidates.first() else {
        return Outcome::New;
    };
    let rival = |s: &&Stored| {
        rank(s) == rank(best)
            && (s.date != best.date
                || normalize(&s.raw_description) != normalize(&best.raw_description))
    };
    if candidates.iter().any(rival) {
        Outcome::Check(best.id)
    } else {
        Outcome::Matched(best.id)
    }
}

#[cfg(test)]
mod tests;
