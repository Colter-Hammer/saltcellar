//! The Actual import's commit (docs/spec/m1-ledger-import.md#step-4-done): the choices of the
//! importer's steps 2 and 3, applied to an export's live rows, as the plan of what the import
//! writes (docs/spec/m1-ledger-import.md#turning-actual-rows-into-transactions).
//!
//! In this version splits and linked transfers take the spec's fallback paths *(agent)*: a split
//! comes in unsplit and uncategorized with the Actual split in its memo, and every transfer leg is
//! treated like a row whose category goes to Transfer.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use sha2::{Digest, Sha256};

use crate::accounts::{AccountType, Role};
use crate::actual::{Export, Payee, Transaction};
use crate::{Date, Money};

/// What happens to one Actual account (step 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountChoice {
    /// Create new, with this name, type and role; balances-only brings the balance, no rows.
    Create {
        name: String,
        kind: AccountType,
        role: Role,
        balances_only: bool,
    },
    /// Same as this app account: its rows go there.
    SameAs(i64),
    /// None of its rows are imported.
    Skip,
}

/// Where one Actual category's rows go (step 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoesTo {
    Category(i64),
    /// Transfer (not a category).
    Transfer,
    /// Decide later.
    Later,
}

/// The choices of steps 2 and 3, by Actual id. Every Actual account needs one; a category without
/// one goes to Decide later.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Choices {
    pub accounts: HashMap<String, AccountChoice>,
    pub categories: HashMap<String, GoesTo>,
}

/// What the app has already, that the choices name.
#[derive(Debug, Clone, Copy)]
pub struct Existing<'a> {
    /// Every app account's id and name.
    pub accounts: &'a [(i64, String)],
    /// Every app category's id.
    pub categories: &'a HashSet<i64>,
}

/// Why the choices can't be committed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChoiceError {
    #[error("Choose what happens to {0}.")]
    NoChoice(String),
    #[error("There's no account {0:?} in this export.")]
    UnknownAccount(String),
    #[error("Enter a name for the new account from {0}.")]
    NoName(String),
    #[error("Another account is already called {0}.")]
    NameTaken(String),
    #[error("{0} is the same as an account that isn't there any more.")]
    NoSuchAccount(String),
    #[error("{0} goes to a category that isn't there any more.")]
    NoSuchCategory(String),
}

/// The import's plan: one entry per Actual account that isn't skipped, in the export's order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub accounts: Vec<PlannedAccount>,
    /// Actual rows left out: starting balances, and the rows of skipped and balances-only accounts.
    pub skipped: u64,
}

/// Where an Actual account's rows and balance go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    New(NewAccount),
    Existing(i64),
}

/// An account the import creates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAccount {
    pub name: String,
    pub kind: AccountType,
    pub role: Role,
    pub balances_only: bool,
    /// Closed in Actual: created closed.
    pub closed: bool,
    /// Actual's bank-sync account id, kept as `external_account_id`.
    pub bank_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedAccount {
    /// Actual's name for it.
    pub actual_name: String,
    pub target: Target,
    /// Its balance in Actual, written as a balance snapshot.
    pub balance: Money,
    /// The snapshot's date: the last imported row's; `None` (the import day) when there are none.
    pub as_of: Option<Date>,
    pub rows: Vec<Row>,
}

/// One row the import inserts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Actual's transaction id.
    pub import_ref: String,
    pub date: Date,
    pub amount: Money,
    pub raw_description: String,
    /// The bank's id for it; dropped for a second row in the same account with the same one
    /// *(agent)*, as an account's ids are unique.
    pub external_id: Option<String>,
    /// The payee's exact name; `None` for no payee or a transfer.
    pub payee: Option<String>,
    /// The normalized description, to become the payee's alias if not taken; with a payee only.
    pub alias: Option<String>,
    pub memo: Option<String>,
    /// Each `#word` in the notes, without the `#`.
    pub tags: Vec<String>,
    pub disposition: Disposition,
    /// The row's Actual category name; `None` for none, and for a split.
    pub source_category: Option<String>,
}

/// What the category mapping did with a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disposition {
    /// Took this app category, for this reason ("From Actual: Groceries").
    Mapped { category_id: i64, reason: String },
    /// Decide later: uncategorized, listed in From Actual, undecided. Also a split, which comes in
    /// unsplit *(agent)*.
    Undecided,
    /// Transfer (not a category), or a transfer leg: uncategorized, transfer-like.
    Transfer,
    /// No category in Actual.
    Uncategorized,
}

impl Disposition {
    /// As `import_batch_rows.source_disposition` stores it.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Disposition::Mapped { .. } => "mapped",
            Disposition::Undecided => "undecided",
            Disposition::Transfer => "transfer",
            Disposition::Uncategorized => "uncategorized",
        }
    }
}

/// How many rows each disposition got, for the Done summary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub mapped: u64,
    pub undecided: u64,
    pub transfer: u64,
    pub uncategorized: u64,
}

impl Plan {
    /// The choices applied to the export.
    ///
    /// # Errors
    /// [`ChoiceError`] when an account has no choice or a bad one, or a category goes to one that
    /// isn't there.
    pub fn new(
        export: &Export,
        choices: &Choices,
        existing: Existing<'_>,
    ) -> Result<Plan, ChoiceError> {
        check(export, choices, existing)?;
        let account_names: HashMap<&str, &str> = export
            .accounts
            .iter()
            .map(|a| (a.id.as_str(), a.name.as_str()))
            .collect();
        let categories: HashMap<&str, &str> = export
            .categories
            .iter()
            .map(|c| (c.id.as_str(), c.name.as_str()))
            .collect();
        let rows = Rows {
            account_names: &account_names,
            categories: &categories,
            choices: &choices.categories,
        };

        let mut planned = Vec::new();
        let mut skipped = 0;
        for account in &export.accounts {
            let actual: Vec<&Transaction> = export
                .transactions
                .iter()
                .filter(|t| t.account == account.id)
                .collect();
            let (target, imports_rows) = match &choices.accounts[&account.id] {
                AccountChoice::Skip => {
                    skipped += actual.len() as u64;
                    continue;
                }
                AccountChoice::SameAs(id) => (Target::Existing(*id), true),
                AccountChoice::Create {
                    name,
                    kind,
                    role,
                    balances_only,
                } => (
                    Target::New(NewAccount {
                        name: name.trim().to_owned(),
                        kind: *kind,
                        role: *role,
                        balances_only: *balances_only,
                        closed: account.closed,
                        bank_id: account.bank_id.clone(),
                    }),
                    !balances_only,
                ),
            };
            let mut seen_ids = HashSet::new();
            let mut imported = Vec::new();
            for transaction in &actual {
                if transaction.starting_balance || !imports_rows {
                    skipped += 1;
                    continue;
                }
                let mut row = rows.row(transaction);
                if let Some(id) = &row.external_id
                    && !seen_ids.insert(id.clone())
                {
                    row.external_id = None;
                }
                imported.push(row);
            }
            let as_of = if imports_rows {
                imported.iter().map(|row| row.date).max()
            } else {
                actual.iter().map(|t| t.date).max()
            };
            planned.push(PlannedAccount {
                actual_name: account.name.clone(),
                target,
                balance: account.balance,
                as_of,
                rows: imported,
            });
        }
        Ok(Plan {
            accounts: planned,
            skipped,
        })
    }

    /// How many rows each disposition got.
    #[must_use]
    pub fn counts(&self) -> Counts {
        let mut counts = Counts::default();
        for row in self.accounts.iter().flat_map(|a| &a.rows) {
            *match row.disposition {
                Disposition::Mapped { .. } => &mut counts.mapped,
                Disposition::Undecided => &mut counts.undecided,
                Disposition::Transfer => &mut counts.transfer,
                Disposition::Uncategorized => &mut counts.uncategorized,
            } += 1;
        }
        counts
    }
}

/// Every account has a choice that can go ahead, and every category goes somewhere there is.
fn check(export: &Export, choices: &Choices, existing: Existing<'_>) -> Result<(), ChoiceError> {
    let known: HashSet<&str> = export.accounts.iter().map(|a| a.id.as_str()).collect();
    if let Some(unknown) = choices
        .accounts
        .keys()
        .find(|id| !known.contains(id.as_str()))
    {
        return Err(ChoiceError::UnknownAccount(unknown.clone()));
    }
    let mut names: Vec<String> = existing
        .accounts
        .iter()
        .map(|(_, name)| name.trim().to_lowercase())
        .collect();
    for account in &export.accounts {
        match choices.accounts.get(&account.id) {
            None => return Err(ChoiceError::NoChoice(account.name.clone())),
            Some(AccountChoice::Create { name, .. }) => {
                let name = name.trim();
                if name.is_empty() {
                    return Err(ChoiceError::NoName(account.name.clone()));
                }
                let folded = name.to_lowercase();
                if names.contains(&folded) {
                    return Err(ChoiceError::NameTaken(name.to_owned()));
                }
                names.push(folded);
            }
            Some(AccountChoice::SameAs(id)) => {
                if !existing.accounts.iter().any(|(app, _)| app == id) {
                    return Err(ChoiceError::NoSuchAccount(account.name.clone()));
                }
            }
            Some(AccountChoice::Skip) => {}
        }
    }
    for (id, goes_to) in &choices.categories {
        if let GoesTo::Category(category) = goes_to
            && !existing.categories.contains(category)
        {
            let name = export
                .categories
                .iter()
                .find(|c| &c.id == id)
                .map_or(id.as_str(), |c| c.name.as_str());
            return Err(ChoiceError::NoSuchCategory(name.to_owned()));
        }
    }
    Ok(())
}

/// What turning a row needs: Actual's names for accounts and categories, and where categories go.
struct Rows<'a> {
    account_names: &'a HashMap<&'a str, &'a str>,
    categories: &'a HashMap<&'a str, &'a str>,
    choices: &'a HashMap<String, GoesTo>,
}

impl Rows<'_> {
    fn row(&self, t: &Transaction) -> Row {
        let payee = match &t.payee {
            Some(Payee::Named(name)) => Some(name.clone()),
            _ => None,
        };
        // A transfer payee's name in Actual is the account's.
        let payee_text = match &t.payee {
            Some(Payee::Named(name)) => Some(name.as_str()),
            Some(Payee::Transfer(account)) => self.account_names.get(account.as_str()).copied(),
            None => None,
        };
        let raw_description = t
            .imported_description
            .as_deref()
            .or(payee_text)
            .unwrap_or_default()
            .to_owned();
        let alias = payee
            .as_ref()
            .map(|_| normalize(&raw_description))
            .filter(|text| !text.is_empty());
        let mut notes: Vec<&str> = t.notes.as_deref().into_iter().collect();
        notes.extend(t.lines.iter().filter_map(|line| line.notes.as_deref()));
        let tags = tags(&notes);
        let category_name = |id: &Option<String>| {
            id.as_deref()
                .and_then(|id| self.categories.get(id))
                .map(|name| (*name).to_owned())
        };

        let (disposition, source_category, memo) = if t.lines.is_empty() {
            let source_category = category_name(&t.category);
            let disposition = if matches!(t.payee, Some(Payee::Transfer(_))) {
                Disposition::Transfer
            } else {
                match (&t.category, &source_category) {
                    (Some(id), Some(name)) => match self.choices.get(id) {
                        Some(GoesTo::Category(category_id)) => Disposition::Mapped {
                            category_id: *category_id,
                            reason: format!("From Actual: {name}"),
                        },
                        Some(GoesTo::Transfer) => Disposition::Transfer,
                        Some(GoesTo::Later) | None => Disposition::Undecided,
                    },
                    _ => Disposition::Uncategorized,
                }
            };
            (disposition, source_category, memo(t.notes.as_deref(), None))
        } else {
            let split = t
                .lines
                .iter()
                .map(|line| {
                    let mut text =
                        category_name(&line.category).unwrap_or_else(|| "Uncategorized".to_owned());
                    let _ = write!(text, " {}", cents(line.amount));
                    if let Some(notes) = line
                        .notes
                        .as_deref()
                        .map(str::trim)
                        .filter(|n| !n.is_empty())
                    {
                        let _ = write!(text, " ({notes})");
                    }
                    text
                })
                .collect::<Vec<_>>()
                .join("; ");
            (
                Disposition::Undecided,
                None,
                memo(
                    t.notes.as_deref(),
                    Some(&format!("Split in Actual: {split}")),
                ),
            )
        };

        Row {
            import_ref: t.id.clone(),
            date: t.date,
            amount: t.amount,
            raw_description,
            external_id: t.financial_id.clone().filter(|id| !id.is_empty()),
            payee,
            alias,
            memo,
            tags,
            disposition,
            source_category,
        }
    }
}

/// The notes, then the split, on lines of their own; `None` when both are empty.
fn memo(notes: Option<&str>, split: Option<&str>) -> Option<String> {
    let parts: Vec<&str> = notes
        .map(str::trim)
        .into_iter()
        .chain(split)
        .filter(|part| !part.is_empty())
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n"))
}

/// Cents as `-12.34`: Actual's amounts are always in cents.
fn cents(amount: Money) -> String {
    let minor = amount.minor();
    let sign = if minor < 0 { "-" } else { "" };
    let abs = minor.unsigned_abs();
    format!("{sign}{}.{:02}", abs / 100, abs % 100)
}

/// Bank text, minimally normalized for milestone 1 *(agent)*: lowercase, punctuation stripped,
/// whitespace collapsed. The full normalizer arrives in milestone 3.
#[must_use]
pub fn normalize(text: &str) -> String {
    let kept: String = text
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Each `#word` in `notes`, without the `#`, once each, in order *(agent)*. A word is letters,
/// digits, `-` and `_`, after a `#` at the start or after a space.
#[must_use]
pub fn tags(notes: &[&str]) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    for word in notes.iter().flat_map(|notes| notes.split_whitespace()) {
        let Some(rest) = word.strip_prefix('#') else {
            continue;
        };
        let tag: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if !tag.is_empty() && !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    tags
}

/// The key that tells a row seen twice: until bank sync's shared dedupe function lands, a SHA-256
/// of the account, date, amount and normalized description *(agent)*, as lowercase hex.
#[must_use]
pub fn dedupe_key(account_id: i64, date: Date, amount: Money, raw_description: &str) -> String {
    let text = format!(
        "{account_id}\n{date}\n{}\n{}",
        amount.minor(),
        normalize(raw_description)
    );
    let digest = Sha256::digest(text.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

#[cfg(test)]
mod tests;
