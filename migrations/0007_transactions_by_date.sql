-- Transactions newest first (docs/spec/m1-ledger-import.md#transactions): the list, searched or
-- filtered, reads rows in (date, id) order. Without this index SQLite sorts every matching row, and
-- works out each one's tags and split flag, before taking a page; with it, it walks the index
-- backwards and stops at the page's end *(agent: measured on a generated 50k-row ledger)*.
-- `id` is the rowid, which every index entry already ends with.

CREATE INDEX idx_txn_date ON transactions(date);
