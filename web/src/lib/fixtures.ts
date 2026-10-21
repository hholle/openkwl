// A typical status answer (KWL EC 300 W R in autumn), for tests and the design preview page.
import type { Status } from '../api/client';

export const SAMPLE_STATUS: Status = {
  uptime_s: 12 * 86400,
  time: '2026-10-17T14:32:05',
  control: { mode: 'auto', level: 2, remaining_s: null, auto_return_s: null, base: 'auto', manual_level: 2 },
  schedule: {
    profile: 1,
    today: [
      { from: '00:00', level: 1 },
      { from: '06:30', level: 2 },
      { from: '08:00', level: 1 },
      { from: '16:00', level: 3 },
      { from: '22:00', level: 1 },
    ],
    next: { at: '16:00', level: 3, tomorrow: false },
  },
  holiday: null,
  temperatures: { outdoor: 9.2, supply: 19.6, extract: 21.9, exhaust: 12.5 },
  supply_fan: { volts: 6, duty_pct: 60, rpm: 1350, stalled: false, airflow_m3h: 120 },
  exhaust_fan: { volts: 6, duty_pct: 60, rpm: 1340, stalled: false, airflow_m3h: 118 },
  efficiency_pct: 83,
  bypass: { open: false, moving: false },
  frost: { state: 'idle', active: false, supply_stopped: false, exhaust_boost_v: 0, defrost: false },
  heat_recovery: { power_w: 640, heating_kwh: 1284, cooling_kwh: 12 },
  filter: { remaining_days: 50, due: false },
  operating_hours: 2210,
  faults: [],
};

type DeepPartial<T> = { [K in keyof T]?: T[K] extends object ? (T[K] extends unknown[] ? T[K] : DeepPartial<T[K]>) : T[K] };

/** SAMPLE_STATUS with some fields replaced. */
export function statusWith(patch: DeepPartial<Status>): Status {
  const s = structuredClone(SAMPLE_STATUS) as unknown as Record<string, unknown>;
  for (const [k, v] of Object.entries(patch)) {
    s[k] = v && typeof v === 'object' && !Array.isArray(v) ? { ...(s[k] as object), ...v } : v;
  }
  return s as unknown as Status;
}
