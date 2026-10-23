// When a person last closed a project's canvas (FR-42). This is this
// machine's derived state: it lives in the App's local storage, never in the
// repository or the hub. Lost or unreadable, the window falls back to the
// last 7 days, so nothing that happened is hidden.
const KEY = 'towermd.lastVisit'
export const FALLBACK_MS = 7 * 24 * 60 * 60 * 1000

type Store = Pick<Storage, 'getItem' | 'setItem'>

function store(): Store | null {
  try { return globalThis.localStorage ?? null } catch { return null }
}
function visits(storage: Store): Record<string, number> {
  try {
    const value: unknown = JSON.parse(storage.getItem(KEY) ?? '{}')
    return value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, number> : {}
  } catch { return {} }
}

/** The window's start for this project; a missing, invalid or future time falls back. */
export function lastVisit(project: string, now: number, storage: Store | null = store()): { since: number; fallback: boolean } {
  const at = storage ? visits(storage)[project] : undefined
  return typeof at === 'number' && Number.isFinite(at) && at > 0 && at <= now
    ? { since: at, fallback: false }
    : { since: now - FALLBACK_MS, fallback: true }
}

/** Records that the person closed this project's canvas now. Failing to write only widens the next window. */
export function recordVisit(project: string, at: number, storage: Store | null = store()) {
  if (!storage) return
  try { storage.setItem(KEY, JSON.stringify({ ...visits(storage), [project]: at })) } catch { /* the fallback covers it */ }
}
