// The unit's history "day record": 1216 bytes, little-endian, one per local day (pure;
// dayRecord.test.ts). `/history/today` answers one record (today so far, in RAM),
// `/history/days/YYYY-MM-DD` one stored record and `/history/days/YYYY-MM` all stored records
// of a month back to back. The UI computes every statistic from the 96 quarter-hours itself.
//
//  0 "KL"            2 u8 version (1)     3 u8 interval (15 min)   4 u16 day (since 1970-01-01, local)
//  8 u32 crc32      12 u32 recorded_s    16 5 × u32 level_s       36 u32 bypass_s   40 u32 frost_s
// 44 u32 heating_wh 48 u32 cooling_wh    52 u16 supply rpm mean   54 u16 exhaust rpm mean
// 64 + 12 i, i = 0..95: i16 outdoor, supply, extract, exhaust (0.1 °C), i16 power_w,
//                       u8 level (255 = not recorded), u8 flags (bit 0 bypass, bit 1 frost)

export const RECORD_LEN = 1216;
export const QUARTERS = 96;
export const DAY_S = 86_400;
const MAGIC = [0x4b, 0x4c];
const NONE = -32768;
const QUARTER_AT = 64;
const QUARTER_LEN = 12;

export const SENSORS = ['outdoor', 'supply', 'extract', 'exhaust'] as const;
export type Sensor = (typeof SENSORS)[number];

export interface Quarter {
  /** Mean temperatures in °C; null without a reading. */
  outdoor: number | null;
  supply: number | null;
  extract: number | null;
  exhaust: number | null;
  /** Recovered heat power in W (positive = heating); null without a value. */
  powerW: number | null;
  /** Fan level 0–4; null when the quarter wasn't recorded. */
  level: number | null;
  bypass: boolean;
  frost: boolean;
}

export interface DayRecord {
  /** Local date, `YYYY-MM-DD`. */
  date: string;
  intervalMin: number;
  recordedS: number;
  /** Seconds at level 0 (off) to 4. */
  levelS: number[];
  bypassS: number;
  frostS: number;
  heatingWh: number;
  coolingWh: number;
  supplyRpm: number;
  exhaustRpm: number;
  quarters: Quarter[];
}

export class RecordError extends Error {}

/** Days since 1970-01-01 → `YYYY-MM-DD` (calendar arithmetic only, no time zone). */
export function dateOfDay(day: number): string {
  return new Date(day * DAY_S * 1000).toISOString().slice(0, 10);
}

/** `YYYY-MM-DD` → days since 1970-01-01. */
export function dayOfDate(date: string): number {
  return Math.round(Date.UTC(Number(date.slice(0, 4)), Number(date.slice(5, 7)) - 1, Number(date.slice(8, 10))) / (DAY_S * 1000));
}

/** `YYYY-MM-DD` moved by whole days. */
export const addDays = (date: string, n: number) => dateOfDay(dayOfDate(date) + n);

/** `YYYY-MM` moved by whole months. */
export function addMonths(month: string, n: number): string {
  const i = Number(month.slice(0, 4)) * 12 + Number(month.slice(5, 7)) - 1 + n;
  return `${Math.floor(i / 12)}-${String((i % 12) + 1).padStart(2, '0')}`;
}

const temp = (v: number) => (v === NONE ? null : v / 10);

/** Decodes the record at `offset`. Throws `RecordError` on a short buffer or wrong magic/version. */
export function decodeRecord(buf: ArrayBuffer, offset = 0): DayRecord {
  if (buf.byteLength - offset < RECORD_LEN) throw new RecordError('short record');
  const v = new DataView(buf, offset, RECORD_LEN);
  if (v.getUint8(0) !== MAGIC[0] || v.getUint8(1) !== MAGIC[1]) throw new RecordError('not a day record');
  if (v.getUint8(2) !== 1) throw new RecordError('unknown record version');
  const u32 = (o: number) => v.getUint32(o, true);
  const quarters: Quarter[] = [];
  for (let i = 0; i < QUARTERS; i++) {
    const o = QUARTER_AT + i * QUARTER_LEN;
    const level = v.getUint8(o + 10);
    const flags = v.getUint8(o + 11);
    const power = v.getInt16(o + 8, true);
    quarters.push({
      outdoor: temp(v.getInt16(o, true)),
      supply: temp(v.getInt16(o + 2, true)),
      extract: temp(v.getInt16(o + 4, true)),
      exhaust: temp(v.getInt16(o + 6, true)),
      powerW: power === NONE ? null : power,
      level: level > 4 ? null : level,
      bypass: (flags & 1) !== 0,
      frost: (flags & 2) !== 0,
    });
  }
  return {
    date: dateOfDay(v.getUint16(4, true)),
    intervalMin: v.getUint8(3),
    recordedS: u32(12),
    levelS: [0, 1, 2, 3, 4].map((l) => u32(16 + l * 4)),
    bypassS: u32(36),
    frostS: u32(40),
    heatingWh: u32(44),
    coolingWh: u32(48),
    supplyRpm: v.getUint16(52, true),
    exhaustRpm: v.getUint16(54, true),
    quarters,
  };
}

/** All records of a month answer (n × 1216 bytes); a trailing partial record is an error. */
export function decodeRecords(buf: ArrayBuffer): DayRecord[] {
  if (buf.byteLength % RECORD_LEN !== 0) throw new RecordError('length is not a multiple of a record');
  const out: DayRecord[] = [];
  for (let o = 0; o < buf.byteLength; o += RECORD_LEN) out.push(decodeRecord(buf, o));
  return out;
}

/** 28–31 for `YYYY-MM`. */
export function daysInMonth(month: string): number {
  return new Date(Date.UTC(Number(month.slice(0, 4)), Number(month.slice(5, 7)), 0)).getUTCDate();
}

/** Less than a whole day recorded: the unit started, restarted or got its clock that day. */
export const isPartial = (r: DayRecord) => r.recordedS < DAY_S;

// ---- statistics ----

export interface Range {
  min: number;
  max: number;
  mean: number;
}

/** Below this outdoor–extract difference the efficiency is too uncertain to count (K). */
export const EFFICIENCY_MIN_DELTA_K = 5;
/** Heating limit and room temperature of the degree-days G20/15 (VDI 4710, DIN V 4108-6). */
export const HEATING_LIMIT_C = 15;
export const ROOM_C = 20;

/** Min, max and mean of the quarter means; null without any reading. */
export function rangeOf(values: (number | null)[]): Range | null {
  let min = Infinity;
  let max = -Infinity;
  let sum = 0;
  let n = 0;
  for (const x of values) {
    if (x === null) continue;
    min = Math.min(min, x);
    max = Math.max(max, x);
    sum += x;
    n++;
  }
  return n ? { min, max, mean: sum / n } : null;
}

/**
 * Temperature efficiency of one quarter, η = (supply − outdoor) / (extract − outdoor) (supply
 * side, as EN 13141-7), or null when it doesn't apply: fans off or not recorded, bypass open,
 * frost protection active, a reading missing, or outdoor and extract air less than 5 K apart.
 */
export function quarterEfficiency(q: Quarter): number | null {
  if (q.level === null || q.level === 0 || q.bypass || q.frost) return null;
  if (q.outdoor === null || q.supply === null || q.extract === null) return null;
  const delta = q.extract - q.outdoor;
  if (Math.abs(delta) < EFFICIENCY_MIN_DELTA_K) return null;
  return (q.supply - q.outdoor) / delta;
}

/** Heating degree-days G20/15 of one day: 20 − mean outdoor temperature on a heating day (< 15 °C). */
export function degreeDays(meanOutdoor: number | null): number | null {
  if (meanOutdoor === null) return null;
  return meanOutdoor < HEATING_LIMIT_C ? ROOM_C - meanOutdoor : 0;
}

export interface DayStats {
  date: string;
  partial: boolean;
  recordedH: number;
  temps: Record<Sensor, Range | null>;
  /** Mean of the quarter efficiencies (0–1), null when no quarter qualifies. */
  efficiency: number | null;
  /** How many quarters the efficiency is based on (for the month's weighted mean). */
  efficiencyQuarters: number;
  heatingKwh: number;
  coolingKwh: number;
  degreeDays: number | null;
  /** Hours at level 0 (off) to 4. */
  levelH: number[];
  bypassH: number;
  frostH: number;
  supplyRpm: number;
  exhaustRpm: number;
  /** Seconds with the fans running (level > 0), the weight of the rpm means. */
  runningS: number;
}

const sum = (xs: number[]) => xs.reduce((a, b) => a + b, 0);

export function dayStats(r: DayRecord): DayStats {
  const temps = Object.fromEntries(SENSORS.map((s) => [s, rangeOf(r.quarters.map((q) => q[s]))])) as Record<Sensor, Range | null>;
  const etas = r.quarters.map(quarterEfficiency).filter((x): x is number => x !== null);
  return {
    date: r.date,
    partial: isPartial(r),
    recordedH: r.recordedS / 3600,
    temps,
    efficiency: etas.length ? sum(etas) / etas.length : null,
    efficiencyQuarters: etas.length,
    heatingKwh: r.heatingWh / 1000,
    coolingKwh: r.coolingWh / 1000,
    degreeDays: degreeDays(temps.outdoor?.mean ?? null),
    levelH: r.levelS.map((s) => s / 3600),
    bypassH: r.bypassS / 3600,
    frostH: r.frostS / 3600,
    supplyRpm: r.supplyRpm,
    exhaustRpm: r.exhaustRpm,
    runningS: sum(r.levelS.slice(1)),
  };
}

export interface MonthTotals {
  days: number;
  partialDays: number;
  /** Lowest and highest quarter mean of the month, mean of the daily means. */
  temps: Record<Sensor, Range | null>;
  efficiency: number | null;
  heatingKwh: number;
  coolingKwh: number;
  degreeDays: number;
  levelH: number[];
  bypassH: number;
  frostH: number;
  supplyRpm: number | null;
  exhaustRpm: number | null;
}

export function monthTotals(days: DayStats[]): MonthTotals {
  const temps = Object.fromEntries(
    SENSORS.map((s) => {
      const rs = days.map((d) => d.temps[s]).filter((x): x is Range => x !== null);
      return [s, rs.length ? { min: Math.min(...rs.map((r) => r.min)), max: Math.max(...rs.map((r) => r.max)), mean: sum(rs.map((r) => r.mean)) / rs.length } : null];
    }),
  ) as Record<Sensor, Range | null>;
  const etaQ = sum(days.map((d) => d.efficiencyQuarters));
  const running = sum(days.map((d) => d.runningS));
  const rpm = (k: 'supplyRpm' | 'exhaustRpm') => (running ? sum(days.map((d) => d[k] * d.runningS)) / running : null);
  return {
    days: days.length,
    partialDays: days.filter((d) => d.partial).length,
    temps,
    efficiency: etaQ ? sum(days.map((d) => (d.efficiency ?? 0) * d.efficiencyQuarters)) / etaQ : null,
    heatingKwh: sum(days.map((d) => d.heatingKwh)),
    coolingKwh: sum(days.map((d) => d.coolingKwh)),
    degreeDays: sum(days.map((d) => d.degreeDays ?? 0)),
    levelH: [0, 1, 2, 3, 4].map((l) => sum(days.map((d) => d.levelH[l] ?? 0))),
    bypassH: sum(days.map((d) => d.bypassH)),
    frostH: sum(days.map((d) => d.frostH)),
    supplyRpm: rpm('supplyRpm'),
    exhaustRpm: rpm('exhaustRpm'),
  };
}
