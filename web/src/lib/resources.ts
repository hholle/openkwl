// Loaded-once data shared between pages: a Settings section, its row summary on the list and
// the nav dot all read the same copy, and a save updates all of them at once.
import { useEffect, useState } from 'react';
import { ApiError, api, type ConfigKey, type ConfigTypes, type Schemas } from '../api/client';

interface Entry {
  data: unknown;
  error: ApiError | null;
  loading: Promise<void> | null;
  loadedAt: number;
  /** When the last load failed (0: it didn't), for retrying while a page shows the entry. */
  failedAt: number;
  subs: Set<() => void>;
}

const entries = new Map<string, Entry>();
/** Counts `clearResources()` calls: mounted hooks move to the fresh entries and load them. */
let generation = 0;
/** A failed load is tried again this long after the failure while it is shown. */
const RETRY_MS = 10_000;

function entry(key: string): Entry {
  let e = entries.get(key);
  if (!e) {
    e = { data: undefined, error: null, loading: null, loadedAt: 0, failedAt: 0, subs: new Set() };
    entries.set(key, e);
  }
  return e;
}

const changed = (e: Entry) => e.subs.forEach((s) => s());

export function reload(key: string, load: () => Promise<unknown>): Promise<void> {
  const e = entry(key);
  if (e.loading) return e.loading;
  e.loading = load()
    .then((d) => {
      e.data = d;
      e.error = null;
      e.loadedAt = Date.now();
      e.failedAt = 0;
    })
    .catch((err) => {
      e.error = err instanceof ApiError ? err : new ApiError(0, null);
      e.failedAt = Date.now();
    })
    .finally(() => {
      e.loading = null;
      changed(e);
    });
  return e.loading;
}

/** Replaces the shared copy, e.g. with the answer of a save. */
export function setResource<T>(key: string, data: T) {
  const e = entry(key);
  e.data = data;
  e.error = null;
  e.loadedAt = Date.now();
  e.failedAt = 0;
  changed(e);
}

export function peekResource<T>(key: string): T | undefined {
  return entries.get(key)?.data as T | undefined;
}

/**
 * Forgets everything (after logging out, so the next login loads fresh; after restoring a
 * backup). Pages still showing an entry load it again at once; loads still running from before
 * land in the forgotten entries and change nothing.
 */
export function clearResources() {
  const subs = [...entries.values()].flatMap((e) => [...e.subs]);
  entries.clear();
  generation += 1;
  subs.forEach((s) => s());
}

/**
 * The shared copy of `key`, loaded on first use (and again after `clearResources()`). A failed
 * load is retried every 10 s while the page is visible. With `refreshMs`, it reloads that often
 * while the page is visible (for live summaries such as the Home Assistant connection).
 */
export function useResource<T>(key: string, load: () => Promise<T>, refreshMs?: number) {
  const gen = generation;
  const e = entry(key);
  const [, tick] = useState(0);
  useEffect(() => {
    const on = () => tick((n) => n + 1);
    e.subs.add(on);
    if ((e.data === undefined || e.error) && !e.loading) void reload(key, load);
    const id = setInterval(
      () => {
        if (document.visibilityState !== 'visible' || e.loading) return;
        const now = Date.now();
        if ((e.error && now - e.failedAt >= RETRY_MS) || (refreshMs && now - e.loadedAt >= refreshMs)) void reload(key, load);
      },
      Math.min(refreshMs ?? 5000, 5000),
    );
    return () => {
      e.subs.delete(on);
      clearInterval(id);
    };
    // `load` is a fresh closure each render; the key identifies it. `gen` moves the hook to the
    // fresh entry after `clearResources()`.
  }, [key, refreshMs, gen]);
  return {
    data: e.data as T | undefined,
    error: e.error,
    reload: () => reload(key, load),
    set: (d: T) => setResource(key, d),
  };
}

export const useConfig = <K extends ConfigKey>(key: K) => useResource<ConfigTypes[K]>(key, () => api.get(key));
export const useMqtt = () => useResource<Schemas['MqttConfig']>('mqtt', api.mqtt);
/** Device data; refreshed every 30 s for the Home Assistant and SD card summaries. */
export const useSystem = () => useResource<Schemas['SystemInfo']>('system', api.system, 30_000);
export const useProfile = (n: number | null) =>
  useResource<Schemas['ScheduleProfile'] | null>(`profile:${n}`, () => (n === null ? Promise.resolve(null) : api.profile(n)));
