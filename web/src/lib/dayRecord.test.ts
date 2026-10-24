import { describe, expect, it } from 'vitest';
import {
  addDays,
  addMonths,
  dateOfDay,
  dayOfDate,
  dayStats,
  daysInMonth,
  decodeRecord,
  decodeRecords,
  degreeDays,
  isPartial,
  monthTotals,
  quarterEfficiency,
  RECORD_LEN,
  RecordError,
} from './dayRecord';
import { concat, encodeRecord } from './dayRecordFixtures';

const q = { outdoor: 0, supply: 16, extract: 20, exhaust: 4, powerW: 500, level: 2, bypass: false, frost: false };

describe('day record', () => {
  it('converts dates and days since 1970', () => {
    expect(dayOfDate('1970-01-01')).toBe(0);
    expect(dateOfDay(dayOfDate('2026-10-24'))).toBe('2026-10-24');
    expect(addDays('2026-03-01', -1)).toBe('2026-02-28');
    expect(addDays('2026-12-31', 1)).toBe('2027-01-01');
    expect(addMonths('2026-01', -1)).toBe('2025-12');
    expect(addMonths('2026-12', 1)).toBe('2027-01');
    expect(daysInMonth('2026-10')).toBe(31);
    expect(daysInMonth('2028-02')).toBe(29);
    expect(daysInMonth('2026-02')).toBe(28);
  });

  it('decodes every field of a hand-built record', () => {
    // Built byte by byte here, independent of the fixture encoder.
    const buf = new ArrayBuffer(RECORD_LEN);
    const v = new DataView(buf);
    v.setUint8(0, 0x4b);
    v.setUint8(1, 0x4c);
    v.setUint8(2, 1);
    v.setUint8(3, 15);
    v.setUint16(4, 20_385, true); // 2025-10-24
    v.setUint32(12, 86_400, true);
    [600, 3600, 72_000, 7200, 3000].forEach((s, l) => v.setUint32(16 + l * 4, s, true));
    v.setUint32(36, 1800, true);
    v.setUint32(40, 900, true);
    v.setUint32(44, 12_345, true);
    v.setUint32(48, 67, true);
    v.setUint16(52, 1400, true);
    v.setUint16(54, 1380, true);
    for (let i = 0; i < 96; i++) {
      const o = 64 + i * 12;
      [-32768, -32768, -32768, -32768, -32768].forEach((x, k) => v.setInt16(o + k * 2, x, true));
      v.setUint8(o + 10, 255);
    }
    const o = 64 + 5 * 12;
    v.setInt16(o, -35, true);
    v.setInt16(o + 2, 181, true);
    v.setInt16(o + 4, 215, true);
    v.setInt16(o + 6, -32768, true);
    v.setInt16(o + 8, -120, true);
    v.setUint8(o + 10, 3);
    v.setUint8(o + 11, 3);

    const r = decodeRecord(buf);
    expect(r).toMatchObject({
      date: '2025-10-24',
      intervalMin: 15,
      recordedS: 86_400,
      levelS: [600, 3600, 72_000, 7200, 3000],
      bypassS: 1800,
      frostS: 900,
      heatingWh: 12_345,
      coolingWh: 67,
      supplyRpm: 1400,
      exhaustRpm: 1380,
    });
    expect(r.quarters).toHaveLength(96);
    expect(r.quarters[5]).toEqual({ outdoor: -3.5, supply: 18.1, extract: 21.5, exhaust: null, powerW: -120, level: 3, bypass: true, frost: true });
    expect(r.quarters[0]).toEqual({ outdoor: null, supply: null, extract: null, exhaust: null, powerW: null, level: null, bypass: false, frost: false });
  });

  it('rejects short buffers, a wrong magic and unknown versions', () => {
    expect(() => decodeRecord(new ArrayBuffer(100))).toThrow(RecordError);
    const bad = encodeRecord({ date: '2026-10-01' });
    new DataView(bad).setUint8(0, 0);
    expect(() => decodeRecord(bad)).toThrow(RecordError);
    const v2 = encodeRecord({ date: '2026-10-01' });
    new DataView(v2).setUint8(2, 2);
    expect(() => decodeRecord(v2)).toThrow(RecordError);
    expect(() => decodeRecords(new ArrayBuffer(RECORD_LEN + 1))).toThrow(RecordError);
  });

  it('decodes a month answer, oldest first, and an empty one', () => {
    const days = decodeRecords(concat([encodeRecord({ date: '2026-10-01' }), encodeRecord({ date: '2026-10-03', recordedS: 3600 })]));
    expect(days.map((d) => d.date)).toEqual(['2026-10-01', '2026-10-03']);
    expect(days.map(isPartial)).toEqual([false, true]);
    expect(decodeRecords(new ArrayBuffer(0))).toEqual([]);
  });
});

describe('statistics', () => {
  it('computes the efficiency only for quarters where it means something', () => {
    expect(quarterEfficiency(q)).toBeCloseTo(0.8);
    expect(quarterEfficiency({ ...q, level: 0 })).toBeNull();
    expect(quarterEfficiency({ ...q, level: null })).toBeNull();
    expect(quarterEfficiency({ ...q, bypass: true })).toBeNull();
    expect(quarterEfficiency({ ...q, frost: true })).toBeNull();
    expect(quarterEfficiency({ ...q, supply: null })).toBeNull();
    expect(quarterEfficiency({ ...q, outdoor: 16, supply: 19 })).toBeNull(); // 4 K apart
    expect(quarterEfficiency({ ...q, outdoor: 15, supply: 19 })).toBeCloseTo(0.8); // 5 K
    // Summer: outdoor warmer than extract air, the supply air cooled.
    expect(quarterEfficiency({ ...q, outdoor: 30, supply: 22, extract: 20 })).toBeCloseTo(0.8);
  });

  it('counts heating degree-days G20/15', () => {
    expect(degreeDays(5)).toBe(15);
    expect(degreeDays(14.9)).toBeCloseTo(5.1);
    expect(degreeDays(15)).toBe(0);
    expect(degreeDays(null)).toBeNull();
  });

  it('sums up a day and a month', () => {
    const quarters = Array.from({ length: 96 }, (_, i) => ({ ...q, outdoor: i < 48 ? 2 : 6, supply: i < 48 ? 16.4 : 17.2, level: i < 4 ? 0 : 2 }));
    const a = dayStats(decodeRecord(encodeRecord({ date: '2026-10-01', quarters, heatingWh: 4200, levelS: [3600, 0, 82_800, 0, 0], supplyRpm: 1400, exhaustRpm: 1300 })));
    expect(a.partial).toBe(false);
    expect(a.temps.outdoor).toEqual({ min: 2, max: 6, mean: 4 });
    expect(a.temps.extract).toEqual({ min: 20, max: 20, mean: 20 });
    expect(a.efficiency).toBeCloseTo(0.8);
    expect(a.efficiencyQuarters).toBe(92);
    expect(a.heatingKwh).toBeCloseTo(4.2);
    expect(a.degreeDays).toBeCloseTo(16);
    expect(a.levelH).toEqual([1, 0, 23, 0, 0]);

    const b = dayStats(decodeRecord(encodeRecord({ date: '2026-10-02', recordedS: 43_200, quarters: quarters.slice(0, 48).map((x) => ({ ...x, outdoor: 18, supply: 19.6, level: 2 })), heatingWh: 800, levelS: [0, 0, 43_200, 0, 0], supplyRpm: 1600, exhaustRpm: 1500 })));
    expect(b.partial).toBe(true);
    expect(b.degreeDays).toBe(0);
    expect(b.temps.outdoor).toEqual({ min: 18, max: 18, mean: 18 });
    expect(b.efficiency).toBeNull(); // 2 K apart

    const m = monthTotals([a, b]);
    expect(m.days).toBe(2);
    expect(m.partialDays).toBe(1);
    expect(m.heatingKwh).toBeCloseTo(5);
    expect(m.degreeDays).toBeCloseTo(16);
    expect(m.levelH).toEqual([1, 0, 35, 0, 0]);
    expect(m.temps.outdoor).toEqual({ min: 2, max: 18, mean: 11 });
    expect(m.efficiency).toBeCloseTo(0.8);
    // Weighted by running time: 23 h at 1400, 12 h at 1600.
    expect(m.supplyRpm).toBeCloseTo((1400 * 23 + 1600 * 12) / 35);
  });

  it('gives no figures for a month without days', () => {
    const m = monthTotals([]);
    expect(m.temps.outdoor).toBeNull();
    expect(m.efficiency).toBeNull();
    expect(m.supplyRpm).toBeNull();
    expect(m.degreeDays).toBe(0);
  });
});
