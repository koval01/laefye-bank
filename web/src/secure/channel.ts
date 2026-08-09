/** In-browser ECDH/AES/HMAC secure channel + PoW gate. */

import {
  decodeApiFrame,
  decodeJsonCompat,
  decodeRpcResponse,
  encodeApiFrameChallengeReq,
  encodeApiFrameEnvelope,
  encodeApiFrameOpenReq,
  encodeJsonCompat,
  encodeOpenRequest,
  encodeRpcRequest,
  encodeSecureEnvelope,
} from './pb'
import { resolveOp } from './ops'
import { deviceHeaders } from '@/lib/device'

type Session = {
  channelId: string
  encKey: CryptoKey
  macKey: CryptoKey
  counter: number
}

let session: Session | null = null
let opening: Promise<Session> | null = null

function u8(buf: ArrayBuffer): Uint8Array {
  return new Uint8Array(buf)
}

async function sha256(data: Uint8Array): Promise<Uint8Array> {
  return u8(await crypto.subtle.digest('SHA-256', data as BufferSource))
}

function leadingZeroBits(dig: Uint8Array): number {
  let n = 0
  for (const b of dig) {
    if (b === 0) {
      n += 8
      continue
    }
    n += Math.clz32(b) - 24
    break
  }
  return n
}

async function solvePow(challenge: Uint8Array, difficulty: number): Promise<bigint> {
  const scratch = new Uint8Array(challenge.length + 8)
  scratch.set(challenge, 0)
  const view = new DataView(scratch.buffer)
  for (let nonce = 0n; nonce < 50_000_000n; nonce++) {
    view.setBigUint64(challenge.length, nonce, true)
    const dig = await sha256(scratch)
    if (leadingZeroBits(dig) >= difficulty) return nonce
  }
  throw new Error('PoW failed')
}

async function clientBinding(): Promise<Uint8Array> {
  const raw = [
    navigator.userAgent,
    navigator.language,
    String(screen.width),
    String(screen.height),
    Intl.DateTimeFormat().resolvedOptions().timeZone || '',
    location.origin,
  ].join('|')
  return sha256(new TextEncoder().encode(raw))
}

async function importAes(raw: Uint8Array): Promise<CryptoKey> {
  return crypto.subtle.importKey('raw', raw as BufferSource, 'AES-GCM', false, [
    'encrypt',
    'decrypt',
  ])
}

async function importHmac(raw: Uint8Array): Promise<CryptoKey> {
  return crypto.subtle.importKey('raw', raw as BufferSource, { name: 'HMAC', hash: 'SHA-256' }, false, [
    'sign',
    'verify',
  ])
}

async function deriveKeys(
  shared: ArrayBuffer,
  channelId: string,
  binding: Uint8Array,
): Promise<{ enc: CryptoKey; mac: CryptoKey }> {
  const hkdfKey = await crypto.subtle.importKey('raw', shared, 'HKDF', false, ['deriveBits'])
  const info = new Uint8Array(binding.length + 16)
  info.set(binding, 0)
  info.set(new TextEncoder().encode('laefye-secure-v1'), binding.length)
  const bits = await crypto.subtle.deriveBits(
    {
      name: 'HKDF',
      hash: 'SHA-256',
      salt: new TextEncoder().encode(channelId),
      info,
    },
    hkdfKey,
    512,
  )
  const okm = u8(bits)
  return {
    enc: await importAes(okm.subarray(0, 32)),
    mac: await importHmac(okm.subarray(32, 64)),
  }
}

async function makeProof(
  macKey: CryptoKey,
  channelId: string,
  counter: number,
  tsMs: number,
  nonce: Uint8Array,
  ciphertext: Uint8Array,
): Promise<Uint8Array> {
  const enc = new TextEncoder()
  const cBytes = new Uint8Array(8)
  new DataView(cBytes.buffer).setBigUint64(0, BigInt(counter), true)
  const tBytes = new Uint8Array(8)
  new DataView(tBytes.buffer).setBigInt64(0, BigInt(tsMs), true)
  const parts = [enc.encode(channelId), cBytes, tBytes, nonce, ciphertext]
  const len = parts.reduce((n, p) => n + p.length, 0)
  const msg = new Uint8Array(len)
  let o = 0
  for (const p of parts) {
    msg.set(p, o)
    o += p.length
  }
  return u8(await crypto.subtle.sign('HMAC', macKey, msg))
}

async function postApi(body: Uint8Array): Promise<Uint8Array> {
  const res = await fetch('/api', {
    method: 'POST',
    credentials: 'same-origin',
    headers: {
      'content-type': 'application/x-protobuf',
      origin: location.origin,
      ...(await deviceHeaders()),
    },
    body: body as BufferSource,
  })
  if (!res.ok) {
    resetSecureSession()
    throw new Error(`api ${res.status}`)
  }
  return u8(await res.arrayBuffer())
}

async function openSession(): Promise<Session> {
  const chalFrame = decodeApiFrame(await postApi(encodeApiFrameChallengeReq()))
  if (chalFrame.kind !== 'challenge_res') throw new Error('secure challenge failed')
  const chal = chalFrame.value
  const powNonce = await solvePow(chal.challenge, chal.difficulty)

  const kp = await crypto.subtle.generateKey({ name: 'ECDH', namedCurve: 'P-256' }, false, [
    'deriveBits',
  ])
  const rawPub = u8(await crypto.subtle.exportKey('raw', kp.publicKey))
  const binding = await clientBinding()

  const openBody = encodeOpenRequest({
    clientPublicKey: rawPub,
    challengeId: chal.challengeId,
    powNonce,
    clientBinding: binding,
  })
  const openFrame = decodeApiFrame(await postApi(encodeApiFrameOpenReq(openBody)))
  if (openFrame.kind !== 'open_res') throw new Error('secure open failed')
  const opened = openFrame.value

  const serverKey = await crypto.subtle.importKey(
    'raw',
    opened.serverPublicKey as BufferSource,
    { name: 'ECDH', namedCurve: 'P-256' },
    false,
    [],
  )
  const shared = await crypto.subtle.deriveBits(
    { name: 'ECDH', public: serverKey },
    kp.privateKey,
    256,
  )
  const keys = await deriveKeys(shared, opened.channelId, binding)
  return {
    channelId: opened.channelId,
    encKey: keys.enc,
    macKey: keys.mac,
    counter: 0,
  }
}

async function ensureSession(): Promise<Session> {
  if (session) return session
  if (!opening) {
    opening = openSession()
      .then((s) => {
        session = s
        opening = null
        return s
      })
      .catch((e) => {
        opening = null
        throw e
      })
  }
  return opening
}

export function resetSecureSession() {
  session = null
  opening = null
}

export async function secureRpc<T = unknown>(
  method: string,
  url: string,
  body?: unknown,
): Promise<T> {
  const resolved = resolveOp(method, url)
  if (!resolved) throw new Error('unknown op')

  const s = await ensureSession()
  const bodyBytes =
    body === undefined ? new Uint8Array() : encodeJsonCompat(JSON.stringify(body))

  const rpcPlain = encodeRpcRequest({
    op: resolved.op,
    body: bodyBytes,
    args: resolved.args,
  })

  s.counter += 1
  const counter = s.counter
  const tsMs = Date.now()
  const iv = crypto.getRandomValues(new Uint8Array(12))
  const ctBuf = await crypto.subtle.encrypt(
    { name: 'AES-GCM', iv },
    s.encKey,
    rpcPlain as BufferSource,
  )
  const ciphertext = u8(ctBuf)
  const proof = await makeProof(s.macKey, s.channelId, counter, tsMs, iv, ciphertext)
  const envelope = encodeSecureEnvelope({
    channelId: s.channelId,
    counter,
    tsMs,
    nonce: iv,
    proof,
    ciphertext,
  })

  const frameBuf = await postApi(encodeApiFrameEnvelope(envelope))
  const frame = decodeApiFrame(frameBuf)
  if (frame.kind !== 'envelope') {
    resetSecureSession()
    throw new Error('secure rpc failed')
  }
  const env = frame.value
  const pt = u8(
    await crypto.subtle.decrypt(
      { name: 'AES-GCM', iv: env.nonce as BufferSource },
      s.encKey,
      env.ciphertext as BufferSource,
    ),
  )
  const rpc = decodeRpcResponse(pt)
  if (rpc.status >= 400) {
    const err = new Error(rpc.error || `HTTP ${rpc.status}`) as Error & {
      status?: number
      code?: string
    }
    err.status = rpc.status
    err.code = rpc.code
    throw err
  }
  if (!rpc.body.length) return null as T
  const json = decodeJsonCompat(rpc.body)
  return json ? (JSON.parse(json) as T) : (null as T)
}

export function shouldUseSecureChannel(url: string): boolean {
  // Playwright / local UI harness: plain JSON mocks without ECDH envelope.
  if (import.meta.env.VITE_E2E_PLAIN_API === '1') return false
  // Everything under /api goes through the single POST /api protobuf surface
  // (except the transport itself, which posts to `/api`).
  if (url === '/api' || url.startsWith('/api?')) return false
  return url.startsWith('/api/')
}
