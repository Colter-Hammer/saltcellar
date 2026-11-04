# SimpleFIN Bridge as the bank-data provider

Bank data arrives through SimpleFIN Bridge, a paid aggregator (about $15 a year per household) that relays MX's bank connections over a small read-only protocol: one setup token claimed once for an access URL, then `GET /accounts` returning accounts, balances and transactions. CSV and OFX/QFX file import and manual entry are built alongside from the start, for history older than the feed, banks it can't reach, and outages. The household's own banks all connected in a trial, and a daily feed watched for twelve days behaved well enough to design around (see the facts in the [milestone 2 spec](../spec/m2-bank-data.md#reading-the-feed)).

The app talks to SimpleFIN directly: there is no provider-adapter layer beyond `connections.provider`, because a second provider isn't wanted and an abstraction designed around one example would be wrong anyway.

## Considered Options

- **Plaid:** the broadest coverage, but a personal app needs a developer account, an approval process, and a trial tier limited to a handful of bank logins; investment-account coverage at the household's brokerage was doubtful.
- **Teller:** a clean API, but no investment accounts.
- **Direct OFX from each bank:** free and standard, but several large banks have dropped or broken their OFX servers.
- **Files only:** no third party at all, but someone has to download files every week, which is the chore the app exists to remove.

## Consequences

- At most 24 requests a day and 90 days per request, with a recommended 45; the app caps itself at 12 a day and 45-day windows, and the daily sync asks for 30 days.
- Ids are opaque provider UUIDs that aren't guaranteed stable, pending rows can change id or vanish, and some banks report a balance with the wrong sign. Sync matches by id first with content fallbacks, retires vanished pending rows, and tracks drift instead of trusting either side blindly.
- The access URL is a long-lived credential: stored encrypted with a key kept outside the database, never shown or logged. Losing the key costs a new setup token, not data.
- Investment accounts come through as balances only; holdings and trades are out of scope.
- Leaving SimpleFIN later means a new `connections.provider` value and its own fetch code; the matching, balance and file-import rules don't depend on the provider.
