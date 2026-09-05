#!/bin/sh

set -eu

for path in /workspace/node_modules /workspace/dist /workspace/*.tsbuildinfo; do
  [ -e "$path" ] || continue
  chown -R bun:bun "$path"
done

exec runuser -u bun -- "$@"
