#!/usr/bin/env bash
# Smoke: public API must accept register/begin for the live tunnel Origin,
# and reject the placeholder pending.tunnel origin.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

ORIGIN="${1:-}"
if [ -z "$ORIGIN" ]; then
  ORIGIN=$(docker compose exec -T web sh -c 'curl -fsS http://envoy:8080/api/rp' \
    | sed -n 's/.*"origin":"\([^"]*\)".*/\1/p')
fi
if [ -z "$ORIGIN" ] || echo "$ORIGIN" | grep -q pending.tunnel; then
  echo "FAIL: RP origin not ready ($ORIGIN)" >&2
  exit 1
fi

USER="smoke_$(date +%s)"
CODE=$(docker compose exec -T web sh -c \
  "curl -sS -o /tmp/rb.json -w '%{http_code}' -X POST http://envoy:8080/api/register/begin \
    -H 'content-type: application/json' \
    -H 'origin: ${ORIGIN}' \
    -d '{\"username\":\"${USER}\"}'")

if [ "$CODE" != "200" ]; then
  echo "FAIL: register/begin HTTP $CODE for origin=$ORIGIN" >&2
  docker compose exec -T web cat /tmp/rb.json >&2 || true
  exit 1
fi

STALE=$(docker compose exec -T web sh -c \
  "curl -sS -o /tmp/stale.json -w '%{http_code}' -X POST http://envoy:8080/api/register/begin \
    -H 'content-type: application/json' \
    -H 'origin: https://pending.tunnel' \
    -d '{\"username\":\"${USER}x\"}'")

if [ "$STALE" = "200" ]; then
  echo "FAIL: pending.tunnel origin was accepted" >&2
  exit 1
fi

echo "OK register/begin for $ORIGIN (stale pending rejected with HTTP $STALE)"
