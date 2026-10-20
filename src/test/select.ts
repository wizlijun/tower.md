import { fireEvent } from '@testing-library/svelte'

/** happy-dom 20 does not match OPTION :checked, which Svelte's select binding
 * reads synchronously during change. Correct only this element for that event;
 * production bindings and the global DOM prototype remain untouched. */
export async function selectValue(element: HTMLElement, value: string) {
  const select = element as HTMLSelectElement
  const original = select.querySelector
  select.value = value
  select.querySelector = function <T extends Element = Element>(selector: string): T | null {
    if (selector === ':checked') return (Array.from(select.options).find((option) => option.selected) ?? null) as unknown as T | null
    return original.call(select, selector) as T | null
  }
  try { await fireEvent.change(select) }
  finally { select.querySelector = original }
}
