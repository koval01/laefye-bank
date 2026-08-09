#!/bin/sh
# Quick Tunnel → envoy:8080, then keep POST /debug/rp so WebAuthn RP survives API restarts.
set -eu

APP_URL="${APP_URL:-http://envoy:8080}"
TUNNEL_TARGET="${TUNNEL_TARGET:-http://envoy:8080}"
RP_ENDPOINT="${RP_ENDPOINT:-${APP_URL}/debug/rp}"
RP_REFRESH_SECS="${RP_REFRESH_SECS:-15}"
URL_FILE="${URL_FILE:-/tmp/laefye_public_url}"

echo "cloudflared → ${TUNNEL_TARGET}"
echo "RP notify  → ${RP_ENDPOINT}"

i=0
while [ "$i" -lt 90 ]; do
  if curl -fsS -o /dev/null "${APP_URL}/api/rp" 2>/dev/null; then
    break
  fi
  i=$((i + 1))
  sleep 2
done

# Re-POST origin forever so a rebuilt/restarted public API picks up the tunnel RP.
rp_refresh_loop() {
  while true; do
    sleep "${RP_REFRESH_SECS}"
    if [ -f "${URL_FILE}" ]; then
      url=$(cat "${URL_FILE}")
      if [ -n "${url}" ]; then
        if curl -fsS -X POST "${RP_ENDPOINT}" \
          -H 'content-type: application/json' \
          -d "{\"origin\":\"${url}\"}" >/dev/null; then
          echo "RP refresh ok → ${url}"
        else
          echo "RP refresh failed (api not ready?) → ${url}"
        fi
      fi
    fi
  done
}

rp_refresh_loop &

cloudflared tunnel --no-autoupdate --url "${TUNNEL_TARGET}" 2>&1 | while IFS= read -r line; do
  echo "$line"
  case "$line" in
    *https://*.trycloudflare.com*)
      url=$(printf '%s\n' "$line" | sed -n 's/.*\(https:\/\/[a-zA-Z0-9-]*\.trycloudflare\.com\).*/\1/p' | head -n1)
      if [ -n "$url" ]; then
        echo "$url" > "${URL_FILE}"
        echo "public URL → ${url}"
        n=0
        while [ "$n" -lt 15 ]; do
          if curl -fsS -X POST "${RP_ENDPOINT}" \
            -H 'content-type: application/json' \
            -d "{\"origin\":\"${url}\"}"; then
            echo
            echo "RP updated"
            break
          fi
          n=$((n + 1))
          sleep 2
        done
      fi
      ;;
  esac
done
