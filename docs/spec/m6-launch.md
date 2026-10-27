# Milestone 6: Launch

**Goal:** the app becomes the household's real budget, running on the home server: data is backed up and restorable, the second person has a login, the review reminder arrives on phones, and the data can always leave as plain files.

**Done when:** everything below works, the app runs on the server from a released image, the move from the PC to the server was done by restoring a backup file (the first restore drill), and the old budgeting tool is retired.

## Backups

- **On by default**, nightly at 02:30 household time *(agent)*, to `BUDGET_BACKUP_DIR` (default `<data dir>/backups`).
- Each run: a consistent SQLite snapshot (`VACUUM INTO` or the backup API) → `PRAGMA integrity_check` on the copy → encrypt with `age` to the public key in Settings → write `budget-YYYY-MM-DD.sqlite.age` *(agent: name)*. Keep **30 daily + 12 monthly**; prune the rest.
- **No key yet:** backups are written unencrypted, with a warning in Settings and on Overview. When a key is set, later backups are encrypted and the old unencrypted files are deleted.
- **Alerts:** a failed run, or no successful run in the last 26 hours *(agent)*, raises an alert (ntfy if configured, always an Overview banner).
- **Settings › Backups:** on/off, `age` public key, retention, last runs, and **Download backup** (takes a fresh backup, encrypted if a key is set, and downloads it).
- **Restore** = stop the app, put the decrypted file in the data dir as the database file, start the app. Documented in `docs/deploy.md`. The private key never touches the server.
- **Restore drill:** at launch, then every 6 months on a reminder: decrypt the latest backup on another machine and open it in a throwaway instance. Settings records when the last drill was done ("Mark drill done") *(agent)*.

## Notifications

- ntfy is **off until configured** in Settings (server URL, access token). Without it, the reminder and alerts are in-app only (Review badge, Overview banner).
- ntfy is the only push channel in v1, behind a small notifier interface so Web Push or email can be added later.
- Messages carry **no amounts, merchants or categories**, and link back into the app (`BUDGET_BASE_URL`).

## Review reminder

- **Per login, opt-in, on by default** for every login. One shared inbox, so whoever reviews first clears it for both.
- Each login has its own ntfy topic, so either can mute independently.
- **Content:** count only, e.g. "12 transactions to review". Tapping opens Review at the oldest transaction.
- **Skipped** when the inbox is empty.
- **Schedule:** default Sunday 19:00, configurable per login (day, time; weekly, every 2 weeks, monthly), in the household timezone.
- **Nudge:** one extra push when the inbox exceeds a threshold (default 25), at most once between scheduled reminders.
- **Settings › Reminder** holds these per login.

## Export

**Settings › Export** downloads a zip of plain CSVs: transactions (with split lines and tags), accounts, categories, budgets, balance snapshots. Restoring an install uses a backup file, not the export. No JSON or API export in v1.

## Logins, invites and recovery

- **Invite links:** any login creates a one-time link that expires in **7 days**; the new person sets their own display name, username and password. In trusted-header mode, the invite records which header username to accept.
- **Settings › Logins:** list logins; remove one (not the last); issue a one-time password reset link for another login.
- **Trusted-header mode** ([Foundations › Auth](foundations.md#auth)): in this mode the setup wizard links the current header username to the first login, and invites accept a named header username.

## PWA

Installable (manifest, icons, service worker). The last-loaded screens' data is cached **read-only**: offline, the app shows what it last saw with an "Offline, read-only" notice; editing is disabled. Requires HTTPS from the server (and so the reverse proxy).

## Release images

GitHub Actions builds tagged images and pushes them to a **private** container registry (GitHub's), staying within the free plan's limits; old images are pruned. The server only pulls. Upgrading or rolling back = change the image tag + `docker compose up -d`.

## Moving to the server

The home-server setup itself (VM, disk encryption, reverse proxy, DNS) is the owner's and documented outside this repo. The app's side: run the released image with Compose behind an HTTPS reverse proxy, in built-in login mode at first. The data moves from the PC by a backup file (not the CSV export), restored on the server: that counts as the first restore drill.

After launch, with no new app code: VPN access for phones away from home, an off-site copy of the backup folder, then switching to trusted-header mode once the owner's SSO exists.
