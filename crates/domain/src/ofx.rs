//! OFX import (docs/spec/m2-bank-data.md#csv-and-ofxqfx-import): one small reader for OFX 1.x
//! (SGML, where a value's closing tag is left out), OFX 2.x (XML), and the QFX and QBO files that
//! are OFX under another name *(agent: written here rather than taken from a crate, since the
//! importer only needs the statement's account id and its transactions)*.
//!
//! Pure: the rows it reads are matched by [`crate::csv_import::plan`] like a CSV file's.

use sha2::{Digest, Sha256};

use crate::Date;
use crate::csv_import::{FileRow, Read, Unreadable, hex, parse_amount};

/// A file with no `<OFX>` element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("This isn't an OFX, QFX or QBO file.")]
pub struct NotOfx;

/// One account's statement in a file: a bank account's (`STMTRS`) or a card's (`CCSTMTRS`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    /// The bank's id for the account (`ACCTID`), empty when the file leaves it out.
    pub account_id: String,
    /// Its transactions (`STMTTRN`), in the file's order: `row` is the place from 1, the FITID
    /// the `import_ref`.
    pub read: Read,
}

/// The statements in `content`, read for an account of `places` decimal places.
///
/// Each transaction's `date` is its posting date (`DTPOSTED`) and its `post_date` the day the
/// household made it (`DTUSER`), when the file has one and it differs: matching takes either
/// *(agent)*. The description is `NAME` (or the bill payee's), else `MEMO`; `MEMO` is the memo
/// when it says something more. A transaction without a FITID gets a hash of its fields instead
/// *(agent)*, so the same file still adds nothing the second time.
///
/// # Errors
/// [`NotOfx`] when there's no `<OFX>` element.
pub fn parse(content: &str, places: u8) -> Result<Vec<Statement>, NotOfx> {
    let body = find_ofx(content).ok_or(NotOfx)?;
    let mut reader = Reader::default();
    for token in tokens(body) {
        reader.take(token);
    }
    Ok(reader
        .statements
        .into_iter()
        .map(|statement| Statement {
            account_id: statement.account_id,
            read: read(statement.transactions, places),
        })
        .collect())
}

/// The last four digits of an account id or name: of its last run of at least four digits.
#[must_use]
pub fn last_four(text: &str) -> Option<String> {
    let run = text
        .split(|c: char| !c.is_ascii_digit())
        .rfind(|run| run.len() >= 4)?;
    run.get(run.len() - 4..).map(str::to_owned)
}

/// The one account of `accounts` (id, name) whose name ends its digits in `last_four`, if exactly
/// one does *(agent: the name, where "Checking 1234" or "Card ··1234" carries them)*.
#[must_use]
pub fn preselect<'a>(
    last: &str,
    accounts: impl IntoIterator<Item = (i64, &'a str)>,
) -> Option<i64> {
    let mut found = accounts
        .into_iter()
        .filter(|(_, name)| last_four(name).as_deref() == Some(last))
        .map(|(id, _)| id);
    let first = found.next()?;
    found.next().is_none().then_some(first)
}

fn find_ofx(content: &str) -> Option<&str> {
    let upper = content.to_ascii_uppercase();
    let mut from = 0;
    while let Some(at) = upper.get(from..)?.find("<OFX") {
        let at = from + at;
        // `<OFX>`, not `<OFXHEADER …?>` or the like.
        if matches!(
            upper.as_bytes().get(at + 4),
            Some(b'>' | b' ' | b'\t' | b'\r' | b'\n')
        ) {
            return content.get(at..);
        }
        from = at + 4;
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Open(String),
    Close(String),
    Text(String),
}

/// The tags and texts of an OFX body; processing instructions and comments are passed over.
fn tokens(body: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut rest = body;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('<') {
            let Some(end) = after.find('>') else { break };
            let tag = after.get(..end).unwrap_or_default().trim();
            rest = after.get(end + 1..).unwrap_or_default();
            if tag.starts_with(['?', '!']) {
                continue;
            }
            // `<TAG/>` holds nothing: as good as no tag.
            if tag.ends_with('/') {
                continue;
            }
            let name = |text: &str| {
                text.split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .to_ascii_uppercase()
            };
            out.push(match tag.strip_prefix('/') {
                Some(closed) => Token::Close(name(closed)),
                None => Token::Open(name(tag)),
            });
        } else {
            let end = rest.find('<').unwrap_or(rest.len());
            let text = decode(rest.get(..end).unwrap_or_default().trim());
            if !text.is_empty() {
                out.push(Token::Text(text));
            }
            rest = rest.get(end..).unwrap_or_default();
        }
    }
    out
}

/// `text` with its character references decoded; one it doesn't know stays as written.
fn decode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(rest.get(..at).unwrap_or_default());
        rest = rest.get(at..).unwrap_or_default();
        let entity = rest
            .find(';')
            .filter(|end| *end <= 10)
            .and_then(|end| Some((end, character(rest.get(1..end)?)?)));
        let (c, skip) = entity.map_or(('&', 1), |(end, c)| (c, end + 1));
        out.push(c);
        rest = rest.get(skip..).unwrap_or_default();
    }
    out.push_str(rest);
    out
}

fn character(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some(' '),
        _ => {
            let number = name.strip_prefix('#')?;
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse().ok()?,
            };
            char::from_u32(code)
        }
    }
}

/// A transaction's fields as the file wrote them.
#[derive(Debug, Default)]
struct Raw {
    fitid: Option<String>,
    posted: Option<String>,
    user: Option<String>,
    amount: Option<String>,
    name: Option<String>,
    payee: Option<String>,
    memo: Option<String>,
}

#[derive(Debug, Default)]
struct RawStatement {
    account_id: String,
    transactions: Vec<Raw>,
}

/// Walks the tokens. Aggregates (`STMTRS`, `STMTTRN`, `PAYEE`, `BANKACCTFROM`…) close in both
/// versions; a value's text belongs to the tag just opened, closed or not.
#[derive(Debug, Default)]
struct Reader {
    statements: Vec<RawStatement>,
    /// Inside a statement, a transaction, its payee, and the statement's own account.
    statement: bool,
    transaction: Option<Raw>,
    payee: bool,
    account: bool,
    /// The tag whose value the next text is.
    leaf: Option<String>,
}

impl Reader {
    fn take(&mut self, token: Token) {
        match token {
            Token::Open(name) => {
                match name.as_str() {
                    "STMTRS" | "CCSTMTRS" => {
                        self.statement = true;
                        self.statements.push(RawStatement::default());
                    }
                    "STMTTRN" if self.statement => self.transaction = Some(Raw::default()),
                    "PAYEE" => self.payee = true,
                    "BANKACCTFROM" | "CCACCTFROM" if self.transaction.is_none() => {
                        self.account = true;
                    }
                    _ => {}
                }
                self.leaf = Some(name);
            }
            Token::Close(name) => {
                match name.as_str() {
                    "STMTRS" | "CCSTMTRS" => self.statement = false,
                    "STMTTRN" => {
                        if let (Some(raw), Some(statement)) =
                            (self.transaction.take(), self.statements.last_mut())
                        {
                            statement.transactions.push(raw);
                        }
                    }
                    "PAYEE" => self.payee = false,
                    "BANKACCTFROM" | "CCACCTFROM" => self.account = false,
                    _ => {}
                }
                self.leaf = None;
            }
            Token::Text(text) => {
                if let Some(leaf) = self.leaf.take() {
                    self.value(&leaf, text);
                }
            }
        }
    }

    fn value(&mut self, leaf: &str, text: String) {
        if let Some(raw) = self.transaction.as_mut() {
            let field = match (leaf, self.payee) {
                ("FITID", _) => &mut raw.fitid,
                ("DTPOSTED", _) => &mut raw.posted,
                ("DTUSER", _) => &mut raw.user,
                ("TRNAMT", _) => &mut raw.amount,
                ("NAME", false) => &mut raw.name,
                ("NAME", true) => &mut raw.payee,
                ("MEMO", _) => &mut raw.memo,
                _ => return,
            };
            *field = Some(text);
        } else if self.account
            && leaf == "ACCTID"
            && let Some(statement) = self.statements.last_mut()
        {
            statement.account_id = text;
        }
    }
}

/// An OFX date-time's day: `YYYYMMDD`, then any time and zone, which are ignored *(agent: the day
/// the bank wrote, as a statement shows it)*.
fn date(text: &str) -> Option<Date> {
    let digits = text.get(..8)?;
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let part = |range: std::ops::Range<usize>| digits.get(range)?.parse::<i16>().ok();
    Date::from_ymd(
        part(0..4)?,
        i8::try_from(part(4..6)?).ok()?,
        i8::try_from(part(6..8)?).ok()?,
    )
    .ok()
}

fn read(transactions: Vec<Raw>, places: u8) -> Read {
    let mut read = Read {
        rows: Vec::new(),
        unreadable: Vec::new(),
    };
    for (at, raw) in transactions.into_iter().enumerate() {
        let row = at + 1;
        let Some(posted) = raw.posted.as_deref().and_then(date) else {
            read.unreadable.push(Unreadable {
                row,
                why: "its date doesn't read",
            });
            continue;
        };
        let Some(amount) = raw.amount.as_deref().and_then(|t| parse_amount(t, places)) else {
            read.unreadable.push(Unreadable {
                row,
                why: "its amount doesn't read",
            });
            continue;
        };
        let import_ref = raw.fitid.clone().unwrap_or_else(|| {
            let fields = [&raw.posted, &raw.amount, &raw.name, &raw.payee, &raw.memo]
                .map(|field| field.as_deref().unwrap_or_default());
            hex(&Sha256::digest(
                format!("ofx\u{1f}{}", fields.join("\u{1f}")).as_bytes(),
            ))
        });
        let name = raw.name.or(raw.payee);
        let (description, memo) = match (name, raw.memo) {
            (Some(name), Some(memo)) if memo != name => (name, Some(memo)),
            (Some(name), _) => (name, None),
            (None, memo) => (memo.unwrap_or_default(), None),
        };
        read.rows.push(FileRow {
            row,
            date: posted,
            post_date: raw
                .user
                .as_deref()
                .and_then(date)
                .filter(|user| *user != posted),
            description,
            amount,
            category: None,
            memo,
            import_ref,
        });
    }
    read
}

#[cfg(test)]
mod tests;
