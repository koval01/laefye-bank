/** Opaque scrambled opcodes — must match server `secure/ops.rs`. */

export const OP = {
  BANK_ME: 0x4a21,
  BANK_PROFILE: 0x4a22,
  ACCOUNTS: 0x5b01,
  ACCOUNTS_OPEN: 0x5b02,
  ACCOUNT_TXNS: 0x5b10,
  TXN_DETAIL: 0x5b20,
  TRANSFER: 0x6c01,
  FX_QUOTE: 0x6c10,
  FX_CONVERT: 0x6c11,
  MARKET: 0x6c20,
  DIRECTORY_LOOKUP: 0x6c30,
  KYC_GET: 0x7d01,
  KYC_POST: 0x7d02,
  KYC_UPLOAD: 0x7d03,
  CRYPTO_SETTINGS: 0x8e01,
  CRYPTO_DEPOSIT_ADDR: 0x8e02,
  CRYPTO_QUOTE_DEPOSIT: 0x8e03,
  CRYPTO_QUOTE_WITHDRAW: 0x8e04,
  CRYPTO_WITHDRAW: 0x8e05,
  CRYPTO_DEPOSITS: 0x8e06,
  CRYPTO_WITHDRAWALS: 0x8e07,
  ACCOUNT: 0x9f01,
  ACCOUNT_PROFILE: 0x9f02,
  ACCOUNT_SESSIONS: 0x9f10,
  ACCOUNT_SESSION_REVOKE: 0x9f11,
  ACCOUNT_REVOKE_OTHERS: 0x9f12,
  ACCOUNT_EVENTS: 0x9f20,
  ACCOUNT_DATA_REQ_LIST: 0x9f30,
  ACCOUNT_DATA_REQ_CREATE: 0x9f31,
  NOTIFICATIONS: 0xa011,
  NOTIFICATIONS_READ_ALL: 0xa012,
  NOTIFICATION_READ: 0xa013,
  HEARTBEAT: 0xa020,
  REGISTER_BEGIN: 0xb101,
  REGISTER_FINISH: 0xb102,
  LOGIN_BEGIN: 0xb111,
  LOGIN_FINISH: 0xb112,
  STEP_UP_BEGIN: 0xb121,
  STEP_UP_FINISH: 0xb122,
  LOGOUT: 0xb130,
  RP: 0xb140,
  ME: 0xb150,
  DOC_STATEMENT: 0xc101,
  DOC_RECEIPT: 0xc102,
  DSM_INFO: 0xc110,
  DSM_VERIFY: 0xc111,
} as const

export type ResolvedOp = { op: number; args: Record<string, string> }

/** Map logical method+URL onto opaque op + short args (never sent as HTTP path). */
export function resolveOp(method: string, url: string): ResolvedOp | null {
  const u = new URL(url, 'https://local.invalid')
  const path = u.pathname
  const q = u.searchParams
  const m = method.toUpperCase()
  const args: Record<string, string> = {}
  q.forEach((v, k) => {
    const short =
      k === 'handle'
        ? 'h'
        : k === 'from'
          ? 'f'
          : k === 'to'
            ? 't'
            : k === 'limit'
              ? 'l'
              : k === 'usdt_minor'
                ? 'u'
                : k === 'rub_minor'
                  ? 'r'
                  : k === 'account_id'
                    ? 'c'
                    : k
    args[short] = v
  })

  const txn = path.match(/^\/api\/accounts\/([^/]+)\/transactions$/)
  if (m === 'GET' && txn) return { op: OP.ACCOUNT_TXNS, args: { ...args, a: txn[1]! } }

  const detail = path.match(/^\/api\/transactions\/([^/]+)$/)
  if (m === 'GET' && detail) return { op: OP.TXN_DETAIL, args: { ...args, i: detail[1]! } }

  const receipt = path.match(/^\/api\/documents\/receipt\/([^/]+)$/)
  if (m === 'GET' && receipt) return { op: OP.DOC_RECEIPT, args: { ...args, i: receipt[1]! } }

  const sess = path.match(/^\/api\/account\/sessions\/([^/]+)$/)
  if (m === 'POST' && sess && sess[1] !== 'revoke-others') {
    return { op: OP.ACCOUNT_SESSION_REVOKE, args: { ...args, i: decodeURIComponent(sess[1]!) } }
  }

  const nread = path.match(/^\/api\/notifications\/([^/]+)\/read$/)
  if (m === 'POST' && nread) return { op: OP.NOTIFICATION_READ, args: { ...args, i: nread[1]! } }

  const table: Record<string, number> = {
    'GET /api/bank/me': OP.BANK_ME,
    'POST /api/bank/profile': OP.BANK_PROFILE,
    'GET /api/accounts': OP.ACCOUNTS,
    'POST /api/accounts/open': OP.ACCOUNTS_OPEN,
    'POST /api/transfers': OP.TRANSFER,
    'GET /api/fx/quote': OP.FX_QUOTE,
    'POST /api/fx/convert': OP.FX_CONVERT,
    'GET /api/market': OP.MARKET,
    'GET /api/directory/lookup': OP.DIRECTORY_LOOKUP,
    'GET /api/kyc': OP.KYC_GET,
    'POST /api/kyc': OP.KYC_POST,
    'POST /api/kyc/documents/b64': OP.KYC_UPLOAD,
    'GET /api/crypto/settings': OP.CRYPTO_SETTINGS,
    'GET /api/crypto/deposit-address': OP.CRYPTO_DEPOSIT_ADDR,
    'GET /api/crypto/quote/deposit': OP.CRYPTO_QUOTE_DEPOSIT,
    'GET /api/crypto/quote/withdraw': OP.CRYPTO_QUOTE_WITHDRAW,
    'POST /api/crypto/withdraw': OP.CRYPTO_WITHDRAW,
    'GET /api/crypto/deposits': OP.CRYPTO_DEPOSITS,
    'GET /api/crypto/withdrawals': OP.CRYPTO_WITHDRAWALS,
    'GET /api/account': OP.ACCOUNT,
    'POST /api/account/profile': OP.ACCOUNT_PROFILE,
    'GET /api/account/sessions': OP.ACCOUNT_SESSIONS,
    'POST /api/account/sessions/revoke-others': OP.ACCOUNT_REVOKE_OTHERS,
    'GET /api/account/events': OP.ACCOUNT_EVENTS,
    'GET /api/account/data-requests': OP.ACCOUNT_DATA_REQ_LIST,
    'POST /api/account/data-requests': OP.ACCOUNT_DATA_REQ_CREATE,
    'GET /api/notifications': OP.NOTIFICATIONS,
    'POST /api/notifications/read-all': OP.NOTIFICATIONS_READ_ALL,
    'GET /api/session/heartbeat': OP.HEARTBEAT,
    'POST /api/register/begin': OP.REGISTER_BEGIN,
    'POST /api/register/finish': OP.REGISTER_FINISH,
    'POST /api/login/begin': OP.LOGIN_BEGIN,
    'POST /api/login/finish': OP.LOGIN_FINISH,
    'POST /api/step-up/begin': OP.STEP_UP_BEGIN,
    'POST /api/step-up/finish': OP.STEP_UP_FINISH,
    'POST /api/logout': OP.LOGOUT,
    'GET /api/rp': OP.RP,
    'GET /api/me': OP.ME,
    'GET /api/documents/statement': OP.DOC_STATEMENT,
    'GET /api/documents/dsm': OP.DSM_INFO,
    'POST /api/documents/dsm/verify': OP.DSM_VERIFY,
  }
  const op = table[`${m} ${path}`]
  if (op == null) return null
  return { op, args }
}
