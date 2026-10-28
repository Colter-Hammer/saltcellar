#!/usr/bin/env bash
# The checked-in sqlx query data (.sqlx/) against a fresh database built from migrations/.
#   scripts/sqlx-offline.sh prepare   regenerate .sqlx/ (run after adding or changing a query)
#   scripts/sqlx-offline.sh check     fail if .sqlx/ is out of date
set -euo pipefail

mode=${1:?usage: sqlx-offline.sh prepare|check}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

export DATABASE_URL="sqlite://$tmp/prepare.sqlite"
export SQLX_OFFLINE=false
cargo sqlx database create
cargo sqlx migrate run --source migrations >/dev/null

case $mode in
  prepare) cargo sqlx prepare --workspace -- --all-targets --locked ;;
  check) cargo sqlx prepare --workspace --check -- --all-targets --locked ;;
  *) echo "usage: sqlx-offline.sh prepare|check" >&2; exit 2 ;;
esac
