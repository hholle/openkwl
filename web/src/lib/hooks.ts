import { useCallback, useEffect, useRef, useState } from 'react';
import { ApiError } from '../api/client';

/**
 * Polls `load` every `ms` while the tab is visible, and at once when the tab becomes visible
 * again (unless `refreshOnVisible` is false). A poll is skipped while the previous one is still running (a busy unit must not pile
 * up requests). `refresh()` (e.g. after a command) always loads, and an older answer that
 * arrives late never overwrites a newer one.
 */
export function usePoll<T>(load: () => Promise<T>, ms: number, refreshOnVisible = true) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  /** `Date.now()` of the last successful load, for "Updated … ago". */
  const [updatedAt, setUpdatedAt] = useState<number | null>(null);
  const loadRef = useRef(load);
  loadRef.current = load;
  const inFlight = useRef(0);
  const issued = useRef(0);
  const applied = useRef(0);

  const refresh = useCallback(async () => {
    const seq = ++issued.current;
    inFlight.current += 1;
    try {
      const value = await loadRef.current();
      if (seq < applied.current) return;
      applied.current = seq;
      setData(value);
      setUpdatedAt(Date.now());
      setError(null);
    } catch (e) {
      if (seq < applied.current) return;
      applied.current = seq;
      setError(e instanceof ApiError ? e : new ApiError(0, null));
    } finally {
      inFlight.current -= 1;
    }
  }, []);

  useEffect(() => {
    void refresh();
    const tick = () => {
      if (document.visibilityState === 'visible' && inFlight.current === 0) void refresh();
    };
    const id = setInterval(tick, ms);
    if (refreshOnVisible) document.addEventListener('visibilitychange', tick);
    return () => {
      clearInterval(id);
      document.removeEventListener('visibilitychange', tick);
    };
  }, [refresh, ms, refreshOnVisible]);

  return { data, error, refresh, setData, updatedAt };
}

/** Loads once; `reload` re-fetches. */
export function useLoad<T>(load: () => Promise<T>) {
  // No reload on tab return: forms built on it would lose unsaved edits.
  const { data, error, refresh, setData } = usePoll(load, 24 * 3600 * 1000, false);
  return { data, error, reload: refresh, setData };
}

/** Red toast that stays until closed: "Couldn't change the level. <reason>" */

/** Success toast for 4 s: "Level 2", "Settings saved". */
