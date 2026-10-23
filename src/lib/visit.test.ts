import { describe, expect, it } from 'vitest'
import { FALLBACK_MS, lastVisit, recordVisit } from './visit'

function memory(initial: Record<string, string> = {}) {
  const data = new Map(Object.entries(initial))
  return { getItem: (key: string) => data.get(key) ?? null, setItem: (key: string, value: string) => { data.set(key, value) } }
}

describe('last visit (FR-42)', () => {
  const now = Date.parse('2026-10-23T12:00:00Z')

  it('records per project on this machine and reads it back', () => {
    const storage = memory()
    recordVisit('/repo/a', now - 1000, storage)
    recordVisit('/repo/b', now - 2000, storage)
    expect(lastVisit('/repo/a', now, storage)).toEqual({ since: now - 1000, fallback: false })
    expect(lastVisit('/repo/b', now, storage)).toEqual({ since: now - 2000, fallback: false })
  })

  it('falls back to the last 7 days when the record is missing, unreadable or in the future', () => {
    const fallback = { since: now - FALLBACK_MS, fallback: true }
    expect(lastVisit('/repo/a', now, memory())).toEqual(fallback)
    expect(lastVisit('/repo/a', now, memory({ 'towermd.lastVisit': 'not json' }))).toEqual(fallback)
    expect(lastVisit('/repo/a', now, memory({ 'towermd.lastVisit': '{"/repo/a":"yesterday"}' }))).toEqual(fallback)
    expect(lastVisit('/repo/a', now, memory({ 'towermd.lastVisit': `{"/repo/a":${now + 1}}` }))).toEqual(fallback)
    expect(lastVisit('/repo/a', now, null)).toEqual(fallback)
  })

  it('never fails when the storage refuses a write', () => {
    const storage = { getItem: () => null, setItem: () => { throw new Error('quota') } }
    expect(() => recordVisit('/repo/a', now, storage)).not.toThrow()
    expect(lastVisit('/repo/a', now, storage).fallback).toBe(true)
  })
})
