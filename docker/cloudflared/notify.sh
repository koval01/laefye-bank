#!/bin/sh
# Quick Tunnel → app:8080, затем POST /debug/rp чтобы WebAuthn RP совпал с публичным URL.
set -eu

APP_URL="${APP_URL:-http://app:8080}"
TUNNEL_TARGET="${TUNNEL_TARGET:-http://app:8080}"
RP_ENDPOINT="${RP_ENDPOINT:-${APP_URL}/debug/rp}"

echo "cloudflared → ${TUNNEL_TARGET}"
echo "RP notify  → ${RP_ENDPOINT}"

# Ждём HTTP приложения.
i=0
while [ "$i" -lt 90 ]; do
  if curl -fsS -o /dev/null "${APP_URL}/api/rp" 2>/dev/null; then
    break
  fi
  i=$((i + 1))
  sleep 2
done

notified=0
cloudflared tunnel --no-autoupdate --url "${TUNNEL_TARGET}" 2>&1 | while IFS= read -r line; do
  echo "$line"
  case "$line" in
    *https://*.trycloudflare.com*)
      url=$(printf '%s\n' "$line" | sed -n 's/.*\(https:\/\/[a-zA-Z0-9-]*\.trycloudflare\.com\).*/\1/p' | head -n1)
      if [ -n "$url" ] && [ "$notified" -eq 0 ]; then
        echo "public URL → ${url}"
        # несколько ретраев: app мог ещё поднимать TLS/роуты
        n=0
        while [ "$n" -lt 15 ]; do
          if curl -fsS -X POST "${RP_ENDPOINT}" \
            -H 'content-type: application/json' \
            -d "{\"origin\":\"${url}\"}"; then
            echo
            echo "RP updated"
            notified=1
            break
          fi
          n=$((n + 1))
          sleep 2
        done
      fi
      ;;
  esac
done
