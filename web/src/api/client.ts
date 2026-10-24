// Typed access to /api/v1 (types generated from api/openapi.yaml: `pnpm gen:api`).
import type { components } from './schema';
import { t } from '../i18n';

export type Schemas = components['schemas'];
export type Status = Schemas['Status'];
export type Control = Schemas['Control'];
export type Mode = Schemas['Mode'];
export type ApiErrorBody = Schemas['ApiError'];

const TOKEN_KEY = 'openkwl.token';

// A normal session lives in sessionStorage (ends with the tab); "stay logged in" puts the
// token in localStorage so it survives closing the browser.
function readToken(): string | null {
  try {
    return localStorage.getItem(TOKEN_KEY) ?? sessionStorage.getItem(TOKEN_KEY);
  } catch {
    return null;
  }
}

let token: string | null = readToken();
const listeners = new Set<() => void>();

export function setToken(t: string | null, remember = false) {
  token = t;
  try {
    localStorage.removeItem(TOKEN_KEY);
    sessionStorage.removeItem(TOKEN_KEY);
    if (t) (remember ? localStorage : sessionStorage).setItem(TOKEN_KEY, t);
  } catch {
    // Storage blocked (private mode): the session lives in memory until the page closes.
  }
  listeners.forEach((l) => l());
}

export const hasToken = () => token !== null;

// While the unit restarts on purpose its sessions are gone, so a 401 means "not back yet", not
// "log in again": the page stays until the restart dialog decides (reload or log in).
let restarting = false;
export function expectRestart(on: boolean) {
  restarting = on;
}

export function onAuthChange(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly body: ApiErrorBody | null,
  ) {
    super(body?.message ?? `HTTP ${status}`);
  }

  /** Short sentence for the UI. */
  describe(): string {
    if (this.status === 0) return t('error.unreachable');
    const b = this.body;
    if (!b) return t('error.httpStatus', { status: this.status });
    switch (b.error) {
      case 'invalid':
        return b.field ? `${b.field.replaceAll('_', ' ')}: ${b.message}.` : `${b.message}.`;
      case 'too_many_requests':
        return b.retry_after_s != null ? t('error.tooMany', { s: b.retry_after_s }) : t('error.tooManySoon');
      case 'unauthorized':
        return b.message === 'wrong password' ? t('error.wrongPassword') : t('error.sessionEnded');
      case 'unavailable':
        return t('error.sdCard');
      default:
        return `${b.message}.`;
    }
  }
}

/** No answer within this counts as "not reachable": a unit that loses power mid-request would
 *  otherwise leave the fetch hanging for minutes (the TCP timeout), with stale values shown as
 *  live. Longer than the slowest normal request (a password change hashes twice, about 2 s). */
const REQUEST_TIMEOUT_MS = 10_000;

function timeout(ms: number): AbortSignal {
  if (typeof AbortSignal.timeout === 'function') return AbortSignal.timeout(ms);
  const c = new AbortController();
  setTimeout(() => c.abort(), ms);
  return c.signal;
}

/** How an answer is read: JSON, plain text, or raw bytes (the history day records). */
type Body = 'json' | 'text' | 'bytes';

async function request<T>(method: string, path: string, body?: unknown, text = false, as: Body = text ? 'text' : 'json'): Promise<T> {
  let res: Response;
  try {
    res = await fetch(`/api/v1${path}`, {
      method,
      signal: timeout(REQUEST_TIMEOUT_MS),
      headers: {
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
        ...(body !== undefined ? { 'Content-Type': text ? 'text/plain' : 'application/json' } : {}),
      },
      body: body === undefined ? undefined : text ? String(body) : JSON.stringify(body),
    });
  } catch {
    throw new ApiError(0, null);
  }
  if (!res.ok) {
    const err = (await res.json().catch(() => null)) as ApiErrorBody | null;
    if (res.status === 401 && !restarting && path !== '/auth/login' && path !== '/auth/password') setToken(null);
    throw new ApiError(res.status, err);
  }
  if (res.status === 204 || res.status === 202) return undefined as T;
  try {
    return (await (as === 'bytes' ? res.arrayBuffer() : as === 'text' ? res.text() : res.json())) as T;
  } catch {
    // Cut off (the timeout) or garbled: as if the unit hadn't answered.
    throw new ApiError(0, null);
  }
}

/** `PUT /system/update` with upload progress (0–1), which `fetch` can't report. */
function uploadUpdate(file: Blob, onProgress: (share: number) => void): Promise<void> {
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    xhr.open('PUT', '/api/v1/system/update');
    if (token) xhr.setRequestHeader('Authorization', `Bearer ${token}`);
    xhr.setRequestHeader('Content-Type', 'application/octet-stream');
    xhr.upload.onprogress = (e) => {
      if (e.lengthComputable) onProgress(e.loaded / e.total);
    };
    xhr.onerror = () => reject(new ApiError(0, null));
    xhr.onload = () => {
      if (xhr.status >= 200 && xhr.status < 300) return resolve();
      let body: ApiErrorBody | null = null;
      try {
        body = JSON.parse(xhr.responseText) as ApiErrorBody;
      } catch {
        // No JSON: the status alone.
      }
      if (xhr.status === 401) setToken(null);
      reject(new ApiError(xhr.status, body));
    };
    xhr.send(file);
  });
}

export const api = {
  setupStatus: () => request<Schemas['SetupStatus']>('GET', '/setup'),
  setup: (password: string) => request<Schemas['LoginResponse']>('POST', '/setup', { password }),
  login: (password: string, remember: boolean) => request<Schemas['LoginResponse']>('POST', '/auth/login', { password, remember }),
  logout: () => request<void>('POST', '/auth/logout'),
  changePassword: (current_password: string, new_password: string) =>
    request<void>('POST', '/auth/password', { current_password, new_password }),
  status: () => request<Status>('GET', '/status'),
  control: (r: Schemas['ControlRequest']) => request<Control>('PATCH', '/control', r),
  get: <K extends ConfigKey>(key: K) => request<ConfigTypes[K]>('GET', `/config/${key}`),
  put: <K extends ConfigKey>(key: K, value: ConfigTypes[K]) => request<ConfigTypes[K]>('PUT', `/config/${key}`, value),
  mqtt: () => request<Schemas['MqttConfig']>('GET', '/config/mqtt'),
  putMqtt: (u: Schemas['MqttConfigUpdate']) => request<Schemas['MqttConfig']>('PUT', '/config/mqtt', u),
  profile: (n: number) => request<Schemas['ScheduleProfile']>('GET', `/config/schedule/${n}`),
  putProfile: (n: number, p: Schemas['ScheduleProfile']) => request<Schemas['ScheduleProfile']>('PUT', `/config/schedule/${n}`, p),
  exportConfig: () => request<string>('GET', '/config/export', undefined, true),
  importConfig: (ini: string) => request<void>('PUT', '/config/import', ini, true),
  resetFilter: () => request<void>('POST', '/filter/reset'),
  log: () => request<string>('GET', '/log', undefined, true),
  system: () => request<Schemas['SystemInfo']>('GET', '/system'),
  systemDetails: () => request<Schemas['SystemDetails']>('GET', '/system/details'),
  /** Today so far, one day record (`lib/dayRecord.ts`); kept in RAM, lost on restart. */
  historyToday: () => request<ArrayBuffer>('GET', '/history/today', undefined, false, 'bytes'),
  /** One stored day (`YYYY-MM-DD`) or all stored days of a month (`YYYY-MM`). The unit answers
   *  an empty body when there is none; for a single day that becomes a 404 here. */
  historyDays: async (dateOrMonth: string) => {
    const b = await request<ArrayBuffer>('GET', `/history/days/${dateOrMonth}`, undefined, false, 'bytes');
    if (b.byteLength === 0 && dateOrMonth.length === 10) throw new ApiError(404, null);
    return b;
  },
  setTime: (unix_s: number) => request<void>('POST', '/system/time', { unix_s }),
  reboot: () => request<void>('POST', '/system/reboot'),
  uploadUpdate,
};

export interface ConfigTypes {
  bypass: Schemas['BypassConfig'];
  frost: Schemas['FrostConfig'];
  fans: Schemas['FanConfig'];
  programs: Schemas['ProgramConfig'];
  holiday: Schemas['HolidayConfig'];
  time: Schemas['TimeConfig'];
  filter: Schemas['FilterConfig'];
  network: Schemas['NetworkConfig'];
  schedule: Schemas['ScheduleConfig'];
}
export type ConfigKey = keyof ConfigTypes;
