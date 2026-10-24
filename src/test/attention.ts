import type { AttentionListView } from '../lib/api'

/**
 * The daemon's ordered attention list for a test state, by ID. The order is
 * decided and tested in `towermd_core::attention_list`; App tests only check
 * that the App follows it and keeps its own fields on each item.
 */
export function daemonList(ids: string[], shown: number, quiet: string[] = [], now: string[] = []): AttentionListView {
  return { shown, asleep: quiet.length, budget: 3, items: ids.map((id) => ({ id, label: '告警', title: id, at: 0, objects: [], areas: [],
    object: null, detail: null, action: '', urgent: now.includes(id), delegation: null, signal: null, class: 3,
    timing: now.includes(id) ? 'now' : 'next_visit', quiet: quiet.includes(id), card: '' })) }
}
