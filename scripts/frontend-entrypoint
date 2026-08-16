#!/bin/sh

set -eu

modules=/workspace/crates/escpost/frontend/node_modules
chown bun:bun "$modules"

exec runuser -u bun -- "$@"
