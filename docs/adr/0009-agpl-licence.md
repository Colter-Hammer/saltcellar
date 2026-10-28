# AGPL-3.0 licence

The project is licensed under the GNU Affero General Public License v3.0 only (`AGPL-3.0-only`), added while the repo is still private so it is in place before the repo is made public. The owner wants the code shared, but not taken, changed and run or sold as a closed product. AGPL is the open-source licence that comes closest to this: anyone may use, change and even sell it, but anyone who distributes a changed version or runs one for others over a network has to publish their changes under the same licence.

## Considered Options

- **MIT (or Apache-2.0)**: simplest and most widely accepted, but lets anyone close a fork and sell it.
- **GPL-3.0**: doesn't cover a changed version that is only run as a hosted service, which is the usual way a self-hosted web app gets reused.
- **PolyForm Noncommercial**: bans commercial use outright, but isn't open source, is less proven, and some contributors and packagers avoid it.

## Consequences

- Dependencies must be licence-compatible with AGPL-3.0: permissive licences (MIT, Apache-2.0, BSD, ISC, Zlib, MPL-2.0) are fine; GPL-2.0-only and proprietary ones are not. `cargo-deny` and the npm licence check use that allowlist.
- The app shows a link to its source (Settings › About), which AGPL's network clause requires of anyone running a changed version for others.
- While the owner is the only author, the licence can still be changed. Once outside contributions are accepted, a change needs every contributor's agreement, so a contribution policy has to be chosen before the first outside PR is merged.
