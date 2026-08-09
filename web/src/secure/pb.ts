/** Minimal protobuf wire codec for laefye.v1 secure messages. */

export class PbWriter {
  private chunks: Uint8Array[] = []

  bytes(): Uint8Array {
    const len = this.chunks.reduce((n, c) => n + c.length, 0)
    const out = new Uint8Array(len)
    let o = 0
    for (const c of this.chunks) {
      out.set(c, o)
      o += c.length
    }
    return out
  }

  private push(b: Uint8Array) {
    this.chunks.push(b)
  }

  private varint(v: number | bigint) {
    let n = typeof v === 'bigint' ? v : BigInt(v >>> 0)
    if (typeof v === 'number' && v < 0) n = BigInt(v)
    const parts: number[] = []
    while (n > 0x7fn) {
      parts.push(Number((n & 0x7fn) | 0x80n))
      n >>= 7n
    }
    parts.push(Number(n))
    this.push(Uint8Array.from(parts))
  }

  private tag(field: number, wire: number) {
    this.varint((field << 3) | wire)
  }

  uint32(field: number, v: number) {
    if (!v) return
    this.tag(field, 0)
    this.varint(v >>> 0)
  }

  uint64(field: number, v: number | bigint) {
    if (!v) return
    this.tag(field, 0)
    this.varint(typeof v === 'bigint' ? v : BigInt(v))
  }

  int64(field: number, v: number | bigint) {
    if (!v) return
    this.tag(field, 0)
    let n = typeof v === 'bigint' ? v : BigInt(v)
    // zigzag not used — protobuf int64 is sign-extended varint
    if (n < 0n) n = (1n << 64n) + n
    this.varint(n)
  }

  bool(field: number, v: boolean) {
    if (!v) return
    this.tag(field, 0)
    this.varint(1)
  }

  string(field: number, v: string) {
    if (!v) return
    const enc = new TextEncoder().encode(v)
    this.bytesField(field, enc)
  }

  bytesField(field: number, v: Uint8Array) {
    if (!v.length) return
    this.tag(field, 2)
    this.varint(v.length)
    this.push(v)
  }

  /** Always emit length-delimited field (incl. empty) — required for Empty oneof arms. */
  message(field: number, bytes: Uint8Array) {
    this.tag(field, 2)
    this.varint(bytes.length)
    if (bytes.length) this.push(bytes)
  }
}

export class PbReader {
  private i = 0
  private buf: Uint8Array
  constructor(buf: Uint8Array) {
    this.buf = buf
  }

  private readVarint(): bigint {
    let n = 0n
    let shift = 0n
    while (this.i < this.buf.length) {
      const b = this.buf[this.i++]!
      n |= BigInt(b & 0x7f) << shift
      if ((b & 0x80) === 0) return n
      shift += 7n
    }
    throw new Error('truncated varint')
  }

  next(): { field: number; wire: number } | null {
    if (this.i >= this.buf.length) return null
    const t = this.readVarint()
    return { field: Number(t >> 3n), wire: Number(t & 7n) }
  }

  skip(wire: number) {
    if (wire === 0) this.readVarint()
    else if (wire === 1) this.i += 8
    else if (wire === 5) this.i += 4
    else if (wire === 2) {
      const len = Number(this.readVarint())
      this.i += len
    } else throw new Error('bad wire')
  }

  bytes(): Uint8Array {
    const len = Number(this.readVarint())
    const slice = this.buf.subarray(this.i, this.i + len)
    this.i += len
    return slice
  }

  string(): string {
    return new TextDecoder().decode(this.bytes())
  }

  uint32(): number {
    return Number(this.readVarint())
  }

  uint64(): number {
    return Number(this.readVarint())
  }

  int64(): number {
    let n = this.readVarint()
    if (n >= 1n << 63n) n -= 1n << 64n
    return Number(n)
  }

  bool(): boolean {
    return this.readVarint() !== 0n
  }
}

export function encodeJsonCompat(json: string): Uint8Array {
  const w = new PbWriter()
  w.string(1, json)
  return w.bytes()
}

export function decodeJsonCompat(buf: Uint8Array): string {
  const r = new PbReader(buf)
  let json = ''
  for (;;) {
    const t = r.next()
    if (!t) break
    if (t.field === 1 && t.wire === 2) json = r.string()
    else r.skip(t.wire)
  }
  return json
}

export type ChallengeResponse = {
  challenge: Uint8Array
  difficulty: number
  challengeId: string
}

export function decodeChallengeResponse(buf: Uint8Array): ChallengeResponse {
  const r = new PbReader(buf)
  const out: ChallengeResponse = { challenge: new Uint8Array(), difficulty: 0, challengeId: '' }
  for (;;) {
    const t = r.next()
    if (!t) break
    if (t.field === 1 && t.wire === 2) out.challenge = r.bytes()
    else if (t.field === 2 && t.wire === 0) out.difficulty = r.uint32()
    else if (t.field === 3 && t.wire === 2) out.challengeId = r.string()
    else r.skip(t.wire)
  }
  return out
}

export function encodeOpenRequest(o: {
  clientPublicKey: Uint8Array
  challengeId: string
  powNonce: bigint
  clientBinding: Uint8Array
}): Uint8Array {
  const w = new PbWriter()
  w.bytesField(1, o.clientPublicKey)
  w.string(2, o.challengeId)
  w.uint64(3, o.powNonce)
  w.bytesField(4, o.clientBinding)
  return w.bytes()
}

export type OpenResponse = {
  channelId: string
  serverPublicKey: Uint8Array
  ttlSecs: number
}

export function decodeOpenResponse(buf: Uint8Array): OpenResponse {
  const r = new PbReader(buf)
  const out: OpenResponse = { channelId: '', serverPublicKey: new Uint8Array(), ttlSecs: 0 }
  for (;;) {
    const t = r.next()
    if (!t) break
    if (t.field === 1 && t.wire === 2) out.channelId = r.string()
    else if (t.field === 2 && t.wire === 2) out.serverPublicKey = r.bytes()
    else if (t.field === 3 && t.wire === 0) out.ttlSecs = r.uint32()
    else r.skip(t.wire)
  }
  return out
}

export function encodeSecureEnvelope(o: {
  channelId: string
  counter: number
  tsMs: number
  nonce: Uint8Array
  proof: Uint8Array
  ciphertext: Uint8Array
}): Uint8Array {
  const w = new PbWriter()
  w.string(1, o.channelId)
  w.uint64(2, o.counter)
  w.int64(3, o.tsMs)
  w.bytesField(4, o.nonce)
  w.bytesField(5, o.proof)
  w.bytesField(6, o.ciphertext)
  return w.bytes()
}

export type SecureEnvelope = {
  channelId: string
  counter: number
  tsMs: number
  nonce: Uint8Array
  proof: Uint8Array
  ciphertext: Uint8Array
}

export function decodeSecureEnvelope(buf: Uint8Array): SecureEnvelope {
  const r = new PbReader(buf)
  const out: SecureEnvelope = {
    channelId: '',
    counter: 0,
    tsMs: 0,
    nonce: new Uint8Array(),
    proof: new Uint8Array(),
    ciphertext: new Uint8Array(),
  }
  for (;;) {
    const t = r.next()
    if (!t) break
    if (t.field === 1 && t.wire === 2) out.channelId = r.string()
    else if (t.field === 2 && t.wire === 0) out.counter = r.uint64()
    else if (t.field === 3 && t.wire === 0) out.tsMs = r.int64()
    else if (t.field === 4 && t.wire === 2) out.nonce = r.bytes()
    else if (t.field === 5 && t.wire === 2) out.proof = r.bytes()
    else if (t.field === 6 && t.wire === 2) out.ciphertext = r.bytes()
    else r.skip(t.wire)
  }
  return out
}

export function encodeRpcRequest(o: {
  op: number
  body: Uint8Array
  args: Record<string, string>
}): Uint8Array {
  const w = new PbWriter()
  w.uint32(1, o.op >>> 0)
  w.bytesField(2, o.body)
  for (const [k, v] of Object.entries(o.args)) {
    const entry = new PbWriter()
    entry.string(1, k)
    entry.string(2, v)
    w.message(3, entry.bytes())
  }
  return w.bytes()
}

export type RpcResponse = {
  status: number
  body: Uint8Array
  error: string
  code: string
}

export function decodeRpcResponse(buf: Uint8Array): RpcResponse {
  const r = new PbReader(buf)
  const out: RpcResponse = { status: 0, body: new Uint8Array(), error: '', code: '' }
  for (;;) {
    const t = r.next()
    if (!t) break
    if (t.field === 1 && t.wire === 0) out.status = r.uint32()
    else if (t.field === 2 && t.wire === 2) out.body = r.bytes()
    else if (t.field === 3 && t.wire === 2) out.error = r.string()
    else if (t.field === 4 && t.wire === 2) out.code = r.string()
    else r.skip(t.wire)
  }
  return out
}

export function encodeApiFrameChallengeReq(): Uint8Array {
  const w = new PbWriter()
  // field 1 = challenge_req (Empty message)
  w.message(1, new Uint8Array())
  return w.bytes()
}

export function encodeApiFrameOpenReq(openBytes: Uint8Array): Uint8Array {
  const w = new PbWriter()
  w.message(3, openBytes)
  return w.bytes()
}

export function encodeApiFrameEnvelope(envBytes: Uint8Array): Uint8Array {
  const w = new PbWriter()
  w.message(5, envBytes)
  return w.bytes()
}

export type ApiFrameDecoded =
  | { kind: 'challenge_res'; value: ChallengeResponse }
  | { kind: 'open_res'; value: OpenResponse }
  | { kind: 'envelope'; value: SecureEnvelope }
  | { kind: 'unknown' }

export function decodeApiFrame(buf: Uint8Array): ApiFrameDecoded {
  const r = new PbReader(buf)
  for (;;) {
    const t = r.next()
    if (!t) break
    if (t.wire !== 2) {
      r.skip(t.wire)
      continue
    }
    const inner = r.bytes()
    if (t.field === 2) return { kind: 'challenge_res', value: decodeChallengeResponse(inner) }
    if (t.field === 4) return { kind: 'open_res', value: decodeOpenResponse(inner) }
    if (t.field === 5) return { kind: 'envelope', value: decodeSecureEnvelope(inner) }
  }
  return { kind: 'unknown' }
}
