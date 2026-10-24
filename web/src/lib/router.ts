// Path routes for the four pages: `/`, `/schedule/:section`, `/history/:view`, `/settings/:section`. The unit
// serves index.html for every path without a file extension (firmware/src/http.rs), so these
// survive a reload. Old links (`#/dashboard`, `/maintenance`, moved Settings sections) redirect.
import { type MouseEvent, useEffect, useState } from 'react';

export type Page = 'overview' | 'schedule' | 'history' | 'settings';

export interface Route {
  page: Page;
  /** Section of Schedule or Settings, or the History view; null for the list (phones), the default view or Overview. */
  section: string | null;
}

/** Settings sections that moved to Schedule, and old page names. */
const REDIRECTS: Record<string, string> = {
  '/dashboard': '/',
  '/overview': '/',
  '/maintenance': '/settings/filters',
  '/device': '/settings/technical-details',
  '/settings/holiday': '/schedule/holiday',
  '/settings/party-quiet': '/schedule/party-quiet',
  '/settings/bypass': '/schedule/summer-cooling',
};

/** Hash routes of the previous UI (`#/schedule`) → their new paths. */
const HASH_REDIRECTS: Record<string, string> = {
  dashboard: '/',
  overview: '/',
  schedule: '/schedule/week',
  history: '/history',
  maintenance: '/settings/filters',
  device: '/settings/technical-details',
  settings: '/settings',
};

/** Where an old URL now lives, or null when `path` (and `hash`) is current. */
export function redirectFor(path: string, hash = ''): string | null {
  const old = hash.replace(/^#\/?/, '');
  if (old) return HASH_REDIRECTS[old] ?? '/';
  const clean = path.length > 1 ? path.replace(/\/+$/, '') : path;
  if (REDIRECTS[clean]) return REDIRECTS[clean];
  return clean !== path ? clean : null;
}

export function parseRoute(path: string): Route {
  const [, page, section] = path.split('/');
  if (page === 'schedule' || page === 'settings' || page === 'history') return { page, section: section || null };
  return { page: 'overview', section: null };
}

export function pathOf(page: Page, section?: string | null): string {
  return page === 'overview' ? '/' : section ? `/${page}/${section}` : `/${page}`;
}

/**
 * Asked before every navigation while a section has unsaved changes. It calls `go` to leave,
 * or does nothing to stay.
 */
type Guard = (go: () => void) => void;
let guard: Guard | null = null;
export function setLeaveGuard(g: Guard | null) {
  guard = g;
}

/** Runs `action` after the unsaved-changes question (at once without unsaved changes; never if
 *  the user stays). For ways out that aren't a navigation, such as logging out. */
export function afterLeaveGuard(action: () => void) {
  if (guard) guard(action);
  else action();
}

const listeners = new Set<() => void>();
const notify = () => listeners.forEach((l) => l());

export function navigate(to: string, { replace = false, force = false } = {}) {
  const go = () => {
    if (to !== window.location.pathname || window.location.hash) {
      window.history[replace ? 'replaceState' : 'pushState'](null, '', to);
    }
    window.scrollTo(0, 0);
    notify();
  };
  if (guard && !force) guard(go);
  else go();
}

/** The current path, with old URLs replaced by their new place. */
export function usePath(): string {
  const read = () => {
    const to = redirectFor(window.location.pathname, window.location.hash);
    if (to) window.history.replaceState(null, '', to);
    return window.location.pathname;
  };
  const [path, setPath] = useState(read);
  useEffect(() => {
    const on = () => setPath(read());
    listeners.add(on);
    window.addEventListener('popstate', on);
    return () => {
      listeners.delete(on);
      window.removeEventListener('popstate', on);
    };
  }, []);
  return path;
}

/** `href` + `onClick` for an in-app link; modified clicks (new tab) keep the browser default. */
export function link(to: string) {
  return {
    href: to,
    onClick: (e: MouseEvent) => {
      if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
      e.preventDefault();
      navigate(to);
    },
  };
}
