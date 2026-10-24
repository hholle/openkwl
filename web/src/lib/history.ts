// Chart data for the History page (pure; history.test.ts): the in-browser live buffer, the last
// 24 hours from two day records, and the bands (bypass, frost) and axis ticks of the charts.
import { useSyncExternalStore } from 'react';
import type { Status } from '../api/client';
import { type DayRecord, QUARTERS, type Quarter } from './dayRecord';

/** One point of a chart. `x` is minutes from the window start (day views) or ms (live). */
export interface ChartPoint {
  x: number;
  outdoor: number | null;
  supply: number | null;
  extract: number | null;
  exhaust: number | null;
  power: number | null;
  level: number | null;
  bypass: boolean;
  frost: boolean;
}

export const QUARTER_MIN = 15;
const DAY_MIN = 24 * 60;

const EMPTY: Quarter = { outdoor: null, supply: null, extract: null, exhaust: null, powerW: null, level: null, bypass: false, frost: false };

const pointOf = (q: Quarter, x: number): ChartPoint => ({
  x,
  outdoor: q.outdoor,
  supply: q.supply,
  extract: q.extract,
  exhaust: q.exhaust,
  power: q.powerW,
  level: q.level,
  bypass: q.bypass,
  frost: q.frost,
});

/** A day window of quarter-hours: `startMin` is the clock time (minute of day) of x = 0. */
export interface QuarterWindow {
  startMin: number;
  points: ChartPoint[];
}

/** One day's 96 quarters, x = 0 at midnight. */
export function dayWindow(r: DayRecord): QuarterWindow {
  return { startMin: 0, points: r.quarters.map((q, i) => pointOf(q, i * QUARTER_MIN)) };
}

/** The quarter (0–95) a local time `…THH:MM…` falls in; null without a time. */
export function quarterOfTime(time: string | null | undefined): number | null {
  const m = time?.match(/T(\d\d):(\d\d)/);
  return m ? Math.floor((Number(m[1]) * 60 + Number(m[2])) / QUARTER_MIN) : null;
}

/**
 * The last 24 hours: yesterday's stored record and today's (either may be missing) as one
 * timeline, the 96 quarters that end with the last finished one. `nowQuarter` is the quarter
 * the unit's clock is in (still running, so not shown); without it the window ends with
 * today's last recorded quarter.
 */
export function last24h(yesterday: DayRecord | null, today: DayRecord | null, nowQuarter: number | null): QuarterWindow {
  const both = [...(yesterday?.quarters ?? Array<Quarter>(QUARTERS).fill(EMPTY)), ...(today?.quarters ?? Array<Quarter>(QUARTERS).fill(EMPTY))];
  let end: number;
  if (nowQuarter !== null) end = QUARTERS + nowQuarter - 1;
  else {
    let last = -1;
    today?.quarters.forEach((q, i) => {
      if (q.level !== null) last = i;
    });
    end = last >= 0 ? QUARTERS + last : QUARTERS - 1;
  }
  const start = end - QUARTERS + 1;
  return {
    startMin: (start % QUARTERS) * QUARTER_MIN,
    points: both.slice(start, end + 1).map((q, i) => pointOf(q, i * QUARTER_MIN)),
  };
}

/** True when no point of the window has a recorded level or any reading. */
export function isEmpty(points: ChartPoint[]): boolean {
  return points.every((p) => p.level === null && p.outdoor === null && p.supply === null && p.extract === null && p.exhaust === null);
}

/** Clock time (minute of day) of `x` in a quarter window → "HH:MM". */
export function hhmm(startMin: number, x: number): string {
  const m = (((startMin + x) % DAY_MIN) + DAY_MIN) % DAY_MIN;
  return `${String(Math.floor(m / 60)).padStart(2, '0')}:${String(m % 60).padStart(2, '0')}`;
}

/** The x values in a quarter window that fall on a full `every` minutes of the clock. */
export function clockTicks(startMin: number, every = 180, span = DAY_MIN): number[] {
  const first = (every - (startMin % every)) % every;
  const out: number[] = [];
  for (let x = first; x <= span; x += every) out.push(x);
  return out;
}

/** Spans (x1–x2) where `key` holds, each point lasting `step`. */
export function bands(points: ChartPoint[], key: 'bypass' | 'frost', step: number): { x1: number; x2: number }[] {
  const out: { x1: number; x2: number }[] = [];
  for (const p of points) {
    if (!p[key]) continue;
    const last = out[out.length - 1];
    if (last && p.x <= last.x2 + step / 2) last.x2 = p.x + step;
    else out.push({ x1: p.x, x2: p.x + step });
  }
  return out;
}

/** Round axis ticks (steps of 1, 2, 2.5 or 5 × 10^n) covering `values`, about `target` of them;
 *  [0, 1] without values. The first and last tick are the axis domain. */
export function niceTicks(values: (number | null | undefined)[], target = 4, includeZero = false): number[] {
  const xs = values.filter((v): v is number => typeof v === 'number' && Number.isFinite(v));
  if (includeZero) xs.push(0);
  if (!xs.length) return [0, 1];
  let lo = Math.min(...xs);
  let hi = Math.max(...xs);
  if (hi - lo < 1e-9) {
    lo -= 1;
    hi += 1;
  }
  const raw = (hi - lo) / target;
  const mag = 10 ** Math.floor(Math.log10(raw));
  const step = ([1, 2, 2.5, 5, 10].find((m) => m * mag >= raw) ?? 10) * mag;
  const out: number[] = [];
  for (let t = Math.floor(lo / step) * step; t < hi + step - 1e-9; t += step) out.push(Math.round(t / step) * step);
  return out;
}

// ---- live (in the browser, while the History page is open) ----

/** One point per 10 s at most, for 6 hours at most. */
export const LIVE_STEP_MS = 10_000;
export const LIVE_SPAN_MS = 6 * 3600_000;
/** Two points further apart than this have a gap between them (the page was closed). */
export const LIVE_GAP_MS = 3 * LIVE_STEP_MS;

/** A /status answer as a live point at `now` (ms). */
export function livePoint(s: Status, now: number): ChartPoint {
  const t = s.temperatures;
  return {
    x: now,
    outdoor: t.outdoor,
    supply: t.supply,
    extract: t.extract,
    exhaust: t.exhaust,
    power: s.heat_recovery.power_w,
    level: s.control.level,
    bypass: s.bypass.open,
    frost: s.frost.active,
  };
}

/**
 * `buf` with `p` added, unless the last point is in the same 10 s step (or later); points older
 * than 6 hours before `p` are dropped. Returns `buf` itself when nothing changed.
 */
export function appendLive(buf: ChartPoint[], p: ChartPoint): ChartPoint[] {
  const last = buf[buf.length - 1];
  if (last && Math.floor(p.x / LIVE_STEP_MS) <= Math.floor(last.x / LIVE_STEP_MS)) return buf;
  const from = p.x - LIVE_SPAN_MS;
  let i = 0;
  while (i < buf.length && buf[i]!.x < from) i++;
  return [...buf.slice(i), p];
}

/** The points with an empty point after each one followed by a gap, so the lines break there. */
export function withGaps(points: ChartPoint[], gap = LIVE_GAP_MS, step = LIVE_STEP_MS): ChartPoint[] {
  const out: ChartPoint[] = [];
  points.forEach((p, i) => {
    out.push(p);
    const next = points[i + 1];
    if (next && next.x - p.x > gap) out.push({ ...pointOf(EMPTY, p.x + step) });
  });
  return out;
}

/** Ticks on full clock steps (local time) for a ms range: about `target` of them. */
export function timeTicks(start: number, end: number, target = 5): number[] {
  const steps = [1, 2, 5, 10, 15, 30, 60, 120, 180, 360].map((m) => m * 60_000);
  const step = steps.find((s) => (end - start) / s <= target) ?? steps[steps.length - 1]!;
  const offset = new Date(start).getTimezoneOffset() * 60_000;
  const out: number[] = [];
  for (let t = Math.ceil((start - offset) / step) * step + offset; t <= end; t += step) out.push(t);
  return out;
}

let live: ChartPoint[] = [];
const subs = new Set<() => void>();

/** Adds a /status answer to the live buffer (kept until the page reloads). */
export function recordLive(s: Status, now = Date.now()) {
  const next = appendLive(live, livePoint(s, now));
  if (next === live) return;
  live = next;
  subs.forEach((f) => f());
}

export function useLive(): ChartPoint[] {
  return useSyncExternalStore(
    (f) => {
      subs.add(f);
      return () => subs.delete(f);
    },
    () => live,
  );
}
