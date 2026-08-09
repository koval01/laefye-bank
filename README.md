# Laefye Bank

Самый дружелюбный банк России (демо): **passkeys** + dual Postgres + FX/KYC + React UI в вайбе Monobank — тёмный, розовый, мобильный, без пафоса.

## Стек

| Piece | Role |
|-------|------|
| `api` | Axum — WebAuthn, банк, FX, KYC, админ |
| `auth-db` / `ledger-db` | Профили/KYC · double-entry |
| `minio` | Шифрованные KYC-блобы |
| `web` | React SPA (nginx) |
| `envoy` | Один origin |
| `cloudflared` | Публичный HTTPS |

Админ-хендл по умолчанию: `laefye_ops` (`ADMIN_HANDLES`).

```bash
docker compose up --build
```
