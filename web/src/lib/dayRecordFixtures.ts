// Day records built in the browser, for tests and the /design page (never in the build).
import { dayOfDate, QUARTERS, type Quarter, RECORD_LEN } from './dayRecord';

const NONE = -32768;

export interface RecordInit {
  date: string;
  recordedS?: number;
  levelS?: number[];
  bypassS?: number;
  frostS?: number;
  heatingWh?: number;
  coolingWh?: number;
  supplyRpm?: number;
  exhaustRpm?: number;
  /** Quarter i (0–95); missing quarters are not recorded. */
  quarters?: (Partial<Quarter> | null)[];
}

/** The 1216 bytes of one record, as the unit writes them. */
export function encodeRecord(r: RecordInit): ArrayBuffer {
  const buf = new ArrayBuffer(RECORD_LEN);
  const v = new DataView(buf);
  v.setUint8(0, 0x4b);
  v.setUint8(1, 0x4c);
  v.setUint8(2, 1);
  v.setUint8(3, 15);
  v.setUint16(4, dayOfDate(r.date), true);
  v.setUint32(12, r.recordedS ?? 86_400, true);
  (r.levelS ?? [0, 0, 86_400, 0, 0]).forEach((s, l) => v.setUint32(16 + l * 4, s, true));
  v.setUint32(36, r.bypassS ?? 0, true);
  v.setUint32(40, r.frostS ?? 0, true);
  v.setUint32(44, r.heatingWh ?? 0, true);
  v.setUint32(48, r.coolingWh ?? 0, true);
  v.setUint16(52, r.supplyRpm ?? 0, true);
  v.setUint16(54, r.exhaustRpm ?? 0, true);
  const t = (x: number | null | undefined) => (x === null || x === undefined ? NONE : Math.round(x * 10));
  for (let i = 0; i < QUARTERS; i++) {
    const q = r.quarters?.[i];
    const o = 64 + i * 12;
    v.setInt16(o, t(q?.outdoor), true);
    v.setInt16(o + 2, t(q?.supply), true);
    v.setInt16(o + 4, t(q?.extract), true);
    v.setInt16(o + 6, t(q?.exhaust), true);
    v.setInt16(o + 8, q?.powerW ?? NONE, true);
    v.setUint8(o + 10, q?.level ?? 255);
    v.setUint8(o + 11, (q?.bypass ? 1 : 0) | (q?.frost ? 2 : 0));
  }
  return buf;
}

/** Records back to back, like the month answer. */
export function concat(bufs: ArrayBuffer[]): ArrayBuffer {
  const out = new Uint8Array(bufs.length * RECORD_LEN);
  bufs.forEach((b, i) => out.set(new Uint8Array(b), i * RECORD_LEN));
  return out.buffer;
}

/**
 * A plausible autumn day: outdoor 4–12 °C over the day, about 80 % heat recovery, level 1 at
 * night and 2–3 by day, bypass closed. `upTo` leaves the later quarters unrecorded (today).
 */
export function sampleDay(date: string, { upTo = QUARTERS, shift = 0 }: { upTo?: number; shift?: number } = {}): RecordInit {
  const quarters: Quarter[] = [];
  const levelS = [0, 0, 0, 0, 0];
  let heatingWh = 0;
  for (let i = 0; i < upTo; i++) {
    const h = i / 4;
    const outdoor = 8 + shift + 4 * Math.sin(((h - 9) / 24) * 2 * Math.PI);
    const extract = 21.5 + 0.8 * Math.sin(((h - 12) / 24) * 2 * Math.PI);
    const supply = outdoor + 0.8 * (extract - outdoor);
    const level = h < 6 || h >= 22 ? 1 : h >= 16 && h < 20 ? 3 : 2;
    const powerW = Math.round((supply - outdoor) * [0, 25, 40, 60, 80][level]! * 0.34);
    levelS[level] = (levelS[level] ?? 0) + 900;
    heatingWh += powerW / 4;
    quarters.push({ outdoor, supply, extract, exhaust: extract - 0.8 * (extract - outdoor), powerW, level, bypass: false, frost: false });
  }
  return {
    date,
    recordedS: upTo * 900,
    levelS,
    heatingWh: Math.round(heatingWh),
    supplyRpm: 1400,
    exhaustRpm: 1380,
    quarters,
  };
}
