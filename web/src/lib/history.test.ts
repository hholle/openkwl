import { describe, expect, it } from 'vitest';
import { decodeRecord } from './dayRecord';
import { encodeRecord } from './dayRecordFixtures';
import { statusWith } from './fixtures';
import {
  appendLive,
  bands,
  type ChartPoint,
  clockTicks,
  hhmm,
  isEmpty,
  last24h,
  LIVE_SPAN_MS,
  livePoint,
  niceTicks,
  quarterOfTime,
  timeTicks,
  withGaps,
} from './history';

const at = (x: number, extra: Partial<ChartPoint> = {}): ChartPoint => ({ ...livePoint(statusWith({}), x), ...extra });

/** A record whose quarter i has outdoor = i / 10 (so each quarter can be recognised). */
const day = (date: string, recorded = 96) =>
  decodeRecord(encodeRecord({ date, quarters: Array.from({ length: recorded }, (_, i) => ({ outdoor: i / 10, level: 1 })) }));

describe('live buffer', () => {
  it('keeps at most one point per 10 s', () => {
    let b: ChartPoint[] = [];
    for (let s = 0; s < 60; s++) b = appendLive(b, at(1_000_000_000 + s * 1000));
    expect(b.map((p) => p.x - 1_000_000_000)).toEqual([0, 10_000, 20_000, 30_000, 40_000, 50_000]);
    // Same step or older: nothing changes, not even the array.
    expect(appendLive(b, at(1_000_059_000))).toBe(b);
    expect(appendLive(b, at(999_000_000))).toBe(b);
  });

  it('drops points older than 6 hours', () => {
    let b: ChartPoint[] = [];
    const t0 = 1_000_000_000;
    for (let m = 0; m <= 7 * 60; m++) b = appendLive(b, at(t0 + m * 60_000));
    const last = b[b.length - 1]!;
    expect(last.x).toBe(t0 + 7 * 3600_000);
    expect(b[0]!.x).toBe(last.x - LIVE_SPAN_MS);
    expect(b).toHaveLength(6 * 60 + 1);
  });

  it('takes the readings from /status', () => {
    const p = livePoint(statusWith({ temperatures: { outdoor: null }, control: { level: 3 }, bypass: { open: true } }), 5);
    expect(p).toMatchObject({ x: 5, outdoor: null, supply: 19.6, level: 3, bypass: true, frost: false, power: 640 });
  });

  it('breaks the lines where the page was closed', () => {
    const pts = [at(0), at(10_000), at(600_000), at(610_000)];
    const g = withGaps(pts);
    expect(g.map((p) => p.x)).toEqual([0, 10_000, 20_000, 600_000, 610_000]);
    expect(g[2]).toMatchObject({ outdoor: null, level: null });
  });

  it('puts time ticks on full clock steps', () => {
    const start = new Date(2026, 9, 24, 10, 7, 13).getTime();
    const ticks = timeTicks(start, start + 3 * 3600_000);
    expect(ticks.length).toBeGreaterThan(1);
    expect(ticks.length).toBeLessThanOrEqual(5);
    for (const t of ticks) {
      expect(new Date(t).getMinutes()).toBe(0);
      expect(t).toBeGreaterThanOrEqual(start);
    }
    const short = timeTicks(start, start + 120_000);
    expect(short.every((t) => new Date(t).getSeconds() === 0)).toBe(true);
  });
});

describe('last 24 hours', () => {
  it('finds the quarter of a local time', () => {
    expect(quarterOfTime('2026-10-24T00:14:59')).toBe(0);
    expect(quarterOfTime('2026-10-24T14:32:05')).toBe(58);
    expect(quarterOfTime(null)).toBeNull();
  });

  it('joins yesterday and today and ends with the last finished quarter', () => {
    // 14:32: quarter 58 is running, so the window ends with 57 (14:15) and starts yesterday 14:30.
    const w = last24h(day('2026-10-23'), day('2026-10-24', 58), 58);
    expect(w.points).toHaveLength(96);
    expect(w.startMin).toBe(14 * 60 + 30);
    expect(w.points[0]).toMatchObject({ x: 0, outdoor: 5.8 });
    expect(w.points[37]).toMatchObject({ outdoor: 9.5 }); // yesterday 23:45
    expect(w.points[38]).toMatchObject({ x: 38 * 15, outdoor: 0 }); // today 00:00
    expect(w.points[95]).toMatchObject({ outdoor: 5.7 });
    expect(hhmm(w.startMin, 0)).toBe('14:30');
    expect(hhmm(w.startMin, 95 * 15)).toBe('14:15');
  });

  it('just after midnight shows yesterday as a whole', () => {
    const w = last24h(day('2026-10-23'), day('2026-10-24', 0), 0);
    expect(w.startMin).toBe(0);
    expect(w.points[0]).toMatchObject({ outdoor: 0 });
    expect(w.points[95]).toMatchObject({ outdoor: 9.5 });
  });

  it('leaves gaps for a missing yesterday, and without a clock ends with the last recorded quarter', () => {
    const w = last24h(null, day('2026-10-24', 10), null);
    expect(w.points[95]).toMatchObject({ outdoor: 0.9 });
    expect(w.points[0]).toMatchObject({ outdoor: null, level: null });
    expect(isEmpty(w.points)).toBe(false);
    expect(isEmpty(last24h(null, null, 40).points)).toBe(true);
  });

  it('puts clock ticks on full 3 hours', () => {
    expect(clockTicks(0)).toEqual([0, 180, 360, 540, 720, 900, 1080, 1260, 1440]);
    expect(clockTicks(14 * 60 + 30)[0]).toBe(30); // 15:00
  });

  it('merges bypass and frost quarters into bands', () => {
    const pts = [0, 1, 2, 3, 4, 5].map((i) => ({ ...at(i * 15), bypass: i === 1 || i === 2 || i === 4 }));
    expect(bands(pts, 'bypass', 15)).toEqual([
      { x1: 15, x2: 45 },
      { x1: 60, x2: 75 },
    ]);
    expect(bands(pts, 'frost', 15)).toEqual([]);
  });
});

describe('axis ticks', () => {
  it('rounds to steps of 1, 2, 2.5 or 5', () => {
    expect(niceTicks([-3.2, 23.4])).toEqual([-10, 0, 10, 20, 30]);
    expect(niceTicks([4.1, 22.9], 5)).toEqual([0, 5, 10, 15, 20, 25]);
    expect(niceTicks([85, 340], 4, true)).toEqual([0, 100, 200, 300, 400]);
    expect(niceTicks([0.4, 2.1], 4, true)).toEqual([0, 1, 2, 3]);
    expect(niceTicks([0.4, 1.9], 4, true)).toEqual([0, 0.5, 1, 1.5, 2]);
    expect(niceTicks([null, 21, 21])).toEqual([20, 20.5, 21, 21.5, 22]);
    expect(niceTicks([])).toEqual([0, 1]);
  });
});
