import FingerprintJS from '@fingerprintjs/fingerprintjs'

const FP_HEADER = 'x-laefye-fingerprint'
const STORAGE_KEY = 'laefye_fp'

let cached: string | null = null
let loading: Promise<string> | null = null

function fallbackId(): string {
  try {
    const existing = localStorage.getItem(STORAGE_KEY)
    if (existing) return existing
  } catch {
    /* ignore */
  }
  const id =
    typeof crypto !== 'undefined' && crypto.randomUUID
      ? crypto.randomUUID().replace(/-/g, '')
      : `fb_${Date.now().toString(36)}_${Math.random().toString(36).slice(2)}`
  try {
    localStorage.setItem(STORAGE_KEY, id)
  } catch {
    /* ignore */
  }
  return id
}

/** Stable browser visitor id (FingerprintJS). Cached; falls back to local UUID. */
export async function getVisitorId(): Promise<string> {
  if (cached) return cached
  try {
    const fromSession = sessionStorage.getItem(STORAGE_KEY)
    if (fromSession) {
      cached = fromSession
      return fromSession
    }
  } catch {
    /* ignore */
  }
  if (!loading) {
    loading = (async () => {
      try {
        const agent = await FingerprintJS.load()
        const result = await agent.get()
        const id = result.visitorId
        cached = id
        try {
          sessionStorage.setItem(STORAGE_KEY, id)
          localStorage.setItem(STORAGE_KEY, id)
        } catch {
          /* ignore */
        }
        return id
      } catch {
        const id = fallbackId()
        cached = id
        return id
      }
    })()
  }
  return loading
}

/** Headers for session binding (fingerprint). UA is sent by the browser. */
export async function deviceHeaders(): Promise<Record<string, string>> {
  const fp = await getVisitorId()
  return { [FP_HEADER]: fp }
}

export { FP_HEADER }
