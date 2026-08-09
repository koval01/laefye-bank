// Customer SPA helpers only — never import admin-web or /api/admin paths here.
import { secureRpc, shouldUseSecureChannel } from '@/secure/channel'
import { deviceHeaders } from '@/lib/device'

function b64urlFromBuffer(buf: ArrayBuffer): string {
  const bytes = new Uint8Array(buf)
  let str = ''
  for (const b of bytes) str += String.fromCharCode(b)
  return btoa(str).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/g, '')
}

function bufferFromB64url(value: string): ArrayBuffer {
  const padded = value.replace(/-/g, '+').replace(/_/g, '/')
  const pad = '='.repeat((4 - (padded.length % 4)) % 4)
  const raw = atob(padded + pad)
  const out = new Uint8Array(raw.length)
  for (let i = 0; i < raw.length; i++) out[i] = raw.charCodeAt(i)
  return out.buffer
}

export class ApiError extends Error {
  status: number
  code?: string
  constructor(status: number, message: string, code?: string) {
    super(message)
    this.status = status
    this.code = code
  }
}

export async function api<T = unknown>(
  method: string,
  url: string,
  body?: unknown,
): Promise<T> {
  if (shouldUseSecureChannel(url)) {
    try {
      return await secureRpc<T>(method, url, body)
    } catch (err) {
      if (err instanceof ApiError) throw err
      if (err && typeof err === 'object' && 'status' in err) {
        const e = err as { status?: number; message?: string; code?: string }
        throw new ApiError(e.status || 500, e.message || 'secure rpc failed', e.code)
      }
      throw err
    }
  }

  const extra = await deviceHeaders()
  const res = await fetch(url, {
    method,
    credentials: 'same-origin',
    headers: {
      ...extra,
      ...(body !== undefined ? { 'content-type': 'application/json' } : {}),
    },
    body: body !== undefined ? JSON.stringify(body) : undefined,
  })
  const text = await res.text()
  let data: unknown = null
  try {
    data = text ? JSON.parse(text) : null
  } catch {
    data = { raw: text }
  }
  if (!res.ok) {
    const msg =
      (data as { error?: string; message?: string } | null)?.error ||
      (data as { message?: string } | null)?.message ||
      `HTTP ${res.status}`
    throw new ApiError(
      res.status,
      msg,
      (data as { code?: string } | null)?.code,
    )
  }
  return data as T
}

type CreationOptionsJSON = {
  challenge: string
  rp?: { id?: string; name?: string }
  user: { id: string; name: string; displayName: string }
  excludeCredentials?: { id: string; type: string; transports?: string[] }[]
  [key: string]: unknown
}

type RequestOptionsJSON = {
  challenge: string
  rpId?: string
  allowCredentials?: { id: string; type: string; transports?: string[] }[]
  [key: string]: unknown
}

function prepareCreation(options: CreationOptionsJSON): PublicKeyCredentialCreationOptions {
  return {
    ...options,
    challenge: bufferFromB64url(options.challenge),
    user: { ...options.user, id: bufferFromB64url(options.user.id) },
    excludeCredentials: (options.excludeCredentials || []).map((c) => ({
      ...c,
      id: bufferFromB64url(c.id),
      type: 'public-key' as const,
    })),
  } as PublicKeyCredentialCreationOptions
}

function prepareRequest(options: RequestOptionsJSON): PublicKeyCredentialRequestOptions {
  return {
    ...options,
    challenge: bufferFromB64url(options.challenge),
    allowCredentials: (options.allowCredentials || []).map((c) => ({
      ...c,
      id: bufferFromB64url(c.id),
      type: 'public-key' as const,
    })),
  } as PublicKeyCredentialRequestOptions
}

function credentialToJson(cred: PublicKeyCredential) {
  const response = cred.response as AuthenticatorAttestationResponse & AuthenticatorAssertionResponse
  const base = {
    id: cred.id,
    rawId: b64urlFromBuffer(cred.rawId),
    type: cred.type,
    clientExtensionResults: cred.getClientExtensionResults(),
    authenticatorAttachment: cred.authenticatorAttachment || null,
  }
  if ('attestationObject' in response && response.attestationObject) {
    return {
      ...base,
      response: {
        clientDataJSON: b64urlFromBuffer(response.clientDataJSON),
        attestationObject: b64urlFromBuffer(response.attestationObject),
        transports: response.getTransports?.() ?? [],
      },
    }
  }
  return {
    ...base,
    response: {
      clientDataJSON: b64urlFromBuffer(response.clientDataJSON),
      authenticatorData: b64urlFromBuffer(response.authenticatorData),
      signature: b64urlFromBuffer(response.signature),
      userHandle: response.userHandle ? b64urlFromBuffer(response.userHandle) : null,
    },
  }
}

export async function registerPasskey(username: string, displayName?: string) {
  const begin = await api<CreationOptionsJSON>('POST', '/api/register/begin', {
    username,
    display_name: displayName,
  })
  const cred = (await navigator.credentials.create({
    publicKey: prepareCreation(begin),
  })) as PublicKeyCredential | null
  if (!cred) throw new Error('Passkey creation cancelled')
  return api('POST', '/api/register/finish', credentialToJson(cred))
}

export async function loginPasskey(username?: string) {
  const begin = await api<RequestOptionsJSON>(
    'POST',
    '/api/login/begin',
    username ? { username } : {},
  )
  const cred = (await navigator.credentials.get({
    publicKey: prepareRequest(begin),
  })) as PublicKeyCredential | null
  if (!cred) throw new Error('Passkey login cancelled')
  return api('POST', '/api/login/finish', credentialToJson(cred))
}

/** In-session WebAuthn re-auth for transfers / profile / long statements. */
export async function stepUpPasskey() {
  const begin = await api<RequestOptionsJSON>('POST', '/api/step-up/begin', {})
  const cred = (await navigator.credentials.get({
    publicKey: prepareRequest(begin),
  })) as PublicKeyCredential | null
  if (!cred) throw new Error('Подтверждение passkey отменено')
  return api<{ ok: boolean; step_up_until?: string }>(
    'POST',
    '/api/step-up/finish',
    credentialToJson(cred),
  )
}

/** Run `fn`; on `step_up_required` prompt passkey once and retry. */
export async function withStepUp<T>(fn: () => Promise<T>): Promise<T> {
  try {
    return await fn()
  } catch (err) {
    if (err instanceof ApiError && err.code === 'step_up_required') {
      await stepUpPasskey()
      return await fn()
    }
    throw err
  }
}

/** Always ask for a fresh passkey, then run the sensitive action. */
export async function requirePasskeyThen<T>(fn: () => Promise<T>): Promise<T> {
  await stepUpPasskey()
  return withStepUp(fn)
}

export type BankMe = {
  user_id: string
  username: string
  profile: {
    user_id: string
    handle: string
    display_name: string
    email?: string
    phone?: string
    full_name?: string
    birth_date?: string | null
  } | null
  accounts: {
    id: string
    kind: string
    currency: string
    status: string
    balance_minor: number
  }[]
  currency: string
}

export type Txn = {
  entry_id: string
  created_at: string
  memo: string
  amount_minor: number
  currency: string
  counterparty_account_id: string | null
}

export function dedupeTxns(rows: Txn[]): Txn[] {
  const seen = new Set<string>()
  const out: Txn[] = []
  for (const t of rows) {
    if (seen.has(t.entry_id)) continue
    seen.add(t.entry_id)
    out.push(t)
  }
  return out
}
