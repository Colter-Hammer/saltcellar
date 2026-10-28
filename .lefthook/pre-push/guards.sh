#!/usr/bin/env bash
# lefthook runs this as a script, not a command: it skips pre-push commands when its own list of
# pushed files is empty, which it is for a commit pushed by id. The checks are in scripts/pre-push.
exec scripts/pre-push
