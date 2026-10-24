// FR-50: progress on the same object within a second shows as one visual
// change. Frames (FR-151) and canvas nodes share this one timer rule.

/** How long progress waits so that several changes show as one. */
export const MERGE_MS = 1000

/**
 * Shows the latest value at most once a second. `now` shows it at once and
 * drops what was waiting; otherwise the first change starts the wait and the
 * value shown when it ends is the latest one pushed.
 */
export function coalescer<T>(show: (value: T) => void, delay = MERGE_MS) {
  let timer: ReturnType<typeof setTimeout> | undefined
  let latest: T
  function stop() {
    clearTimeout(timer)
    timer = undefined
  }
  return {
    push(value: T, now = false) {
      latest = value
      if (now) {
        stop()
        show(value)
      } else if (timer === undefined) {
        timer = setTimeout(() => { timer = undefined; show(latest) }, delay)
      }
    },
    /** Drops what was waiting, for example when the value went back to what is shown. */
    stop,
  }
}
