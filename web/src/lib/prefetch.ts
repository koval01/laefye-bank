/** Intent / idle prefetch for route chunks and warm API calls. */

type Loader = () => Promise<unknown>

const loaders = new Map<string, Loader>()
const warmed = new Set<string>()

export function registerPrefetch(path: string, loader: Loader) {
  loaders.set(path, loader)
}

function matchLoader(to: string): Loader | undefined {
  const path = to.split('?')[0] || '/'
  if (loaders.has(path)) return loaders.get(path)
  // Longest prefix match (e.g. /app/transfer → /app)
  let best: string | undefined
  for (const key of loaders.keys()) {
    if (path === key || path.startsWith(key.endsWith('/') ? key : `${key}/`)) {
      if (!best || key.length > best.length) best = key
    }
  }
  return best ? loaders.get(best) : undefined
}

export function prefetch(to: string) {
  const path = to.split('?')[0] || '/'
  if (warmed.has(path)) return
  const loader = matchLoader(path)
  if (!loader) return
  warmed.add(path)
  void loader().catch(() => {
    warmed.delete(path)
  })
}

/** Warm likely next routes when the browser is idle. */
export function prefetchIdle(paths: string[]) {
  const run = () => {
    for (const p of paths) prefetch(p)
  }
  if (typeof window !== 'undefined' && 'requestIdleCallback' in window) {
    window.requestIdleCallback(run, { timeout: 2500 })
  } else {
    setTimeout(run, 600)
  }
}
