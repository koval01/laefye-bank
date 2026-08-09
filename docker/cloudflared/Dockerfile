# syntax=docker/dockerfile:1
FROM cloudflare/cloudflared:latest AS cf
FROM alpine:3.20
RUN apk add --no-cache curl
COPY --from=cf /usr/local/bin/cloudflared /usr/local/bin/cloudflared
COPY notify.sh /notify.sh
RUN chmod +x /notify.sh
ENTRYPOINT ["/notify.sh"]
