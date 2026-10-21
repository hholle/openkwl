// Weekly-program editing as pure functions (covered by schedule.test.ts).
//
// The device stores per day up to 10 switching points {from, level}. The editor presents
// each day as blocks that start at 00:00 and end at 24:00 (like tado / Nest), so a day never
// "inherits" from the previous one while editing; on save the day always has a 00:00 point.

import { t } from '../i18n';
import { weekdayName } from '../i18n/format';

export interface Slot {
  from: string;
  level: number;
}
export type Day = Slot[];

export const MAX_SLOTS = 10;
export const STEP_MIN = 15;
export const DAY_MIN = 1440;

export const toMin = (t: string) => Number(t.slice(0, 2)) * 60 + Number(t.slice(3, 5));
export const fromMin = (m: number) => `${String(Math.floor(m / 60)).padStart(2, '0')}:${String(m % 60).padStart(2, '0')}`;

/** Sorted, de-duplicated, starting at 00:00 (carrying `carry` if the day had no 00:00 point), neighbours with equal levels merged. */
export function normalize(day: Day, carry = 0): Day {
  const sorted = [...day].sort((a, b) => toMin(a.from) - toMin(b.from));
  const out: Day = [];
  if (sorted.length === 0 || toMin(sorted[0]!.from) > 0) out.push({ from: '00:00', level: carry });
  for (const s of sorted) {
    const last = out.at(-1);
    if (last && toMin(last.from) === toMin(s.from)) last.level = s.level;
    else out.push({ ...s });
  }
  return out.filter((s, i) => i === 0 || s.level !== out[i - 1]!.level);
}

/** Days with their 00:00 block made explicit (taking over the previous day's last level). */
export function normalizeWeek(days: Day[]): Day[] {
  return days.map((d, i) => normalize(d, (days[(i + 6) % 7] ?? []).at(-1)?.level ?? 0));
}

export interface Block {
  start: number;
  end: number;
  level: number;
}

export function blocks(day: Day): Block[] {
  return day.map((s, i) => ({
    start: toMin(s.from),
    end: i + 1 < day.length ? toMin(day[i + 1]!.from) : DAY_MIN,
    level: s.level,
  }));
}

/** Moves the start of block `index` (≥ 1), keeping at least one step to its neighbours. */
export function moveBoundary(day: Day, index: number, minute: number): Day {
  if (index <= 0 || index >= day.length) return day;
  const lo = toMin(day[index - 1]!.from) + STEP_MIN;
  const hi = (index + 1 < day.length ? toMin(day[index + 1]!.from) : DAY_MIN) - STEP_MIN;
  const snapped = Math.round(minute / STEP_MIN) * STEP_MIN;
  const m = Math.min(hi, Math.max(lo, snapped));
  return day.map((s, i) => (i === index ? { ...s, from: fromMin(m) } : s));
}

/**
 * Sets an exact start time (from a time field), kept at least one step after the previous and
 * before the next block's start (so a block never passes or swallows a neighbour; not snapped).
 * Block 0 stays at 00:00.
 */
export function setStart(day: Day, index: number, time: string): Day {
  if (index <= 0 || index >= day.length || !/^([01]\d|2[0-3]):[0-5]\d$/.test(time)) return day;
  const lo = toMin(day[index - 1]!.from) + STEP_MIN;
  const hi = (index + 1 < day.length ? toMin(day[index + 1]!.from) : DAY_MIN) - STEP_MIN;
  const m = Math.min(hi, Math.max(lo, toMin(time)));
  return day.map((s, i) => (i === index ? { ...s, from: fromMin(m) } : s));
}

export function setLevel(day: Day, index: number, level: number): Day {
  return normalize(day.map((s, i) => (i === index ? { ...s, level } : s)));
}

/** Removes block `index`; the previous block grows over its time. */
export function removeBlock(day: Day, index: number): Day {
  if (index === 0) return day.length > 1 ? normalize([{ ...day[1]!, from: '00:00' }, ...day.slice(2)]) : day;
  return normalize(day.filter((_, i) => i !== index));
}

const snap = (m: number) => Math.round(m / STEP_MIN) * STEP_MIN;

/**
 * Adds a block in the middle third of the longest block (`[L][new][L]`), with a level
 * different from L. With only one free slot, splits the longest block in half instead and
 * picks a level that differs from both neighbours. `null` when full or no room.
 */
export function addBlock(day: Day): Day | null {
  if (day.length >= MAX_SLOTS) return null;
  const all = blocks(day);
  const i = all.reduce((best, b, k) => (b.end - b.start > all[best]!.end - all[best]!.start ? k : best), 0);
  const longest = all[i]!;
  const len = longest.end - longest.start;
  const other = (avoid: number[]) => [longest.level + 1, longest.level - 1, longest.level + 2, longest.level - 2].find((l) => l >= 0 && l <= 4 && !avoid.includes(l))!;
  if (day.length + 2 <= MAX_SLOTS && len >= 3 * STEP_MIN) {
    const a = snap(longest.start + len / 3);
    const b = snap(longest.start + (2 * len) / 3);
    return normalize([...day, { from: fromMin(a), level: other([longest.level]) }, { from: fromMin(b), level: longest.level }]);
  }
  if (len < 2 * STEP_MIN) return null;
  const next = all[i + 1]?.level ?? -1;
  return normalize([...day, { from: fromMin(snap(longest.start + len / 2)), level: other([longest.level, next]) }]);
}

export function sameDay(a: Day, b: Day): boolean {
  return a.length === b.length && a.every((s, i) => s.from === b[i]!.from && s.level === b[i]!.level);
}

export type Grouping = 'all' | 'weekdays' | 'each';

/** Days of each group of the editor. */
const GROUP_DAYS: Record<Grouping, number[][]> = {
  all: [[0, 1, 2, 3, 4, 5, 6]],
  weekdays: [[0, 1, 2, 3, 4], [5], [6]],
  each: [[0], [1], [2], [3], [4], [5], [6]],
};

function groupLabels(days: number[]): { label: string; short: string } {
  if (days.length === 7) return { label: t('schedule:groups.everyDay'), short: t('schedule:groups.daily') };
  if (days.length === 5) return { label: t('schedule:groups.monFri'), short: t('schedule:groups.monFri') };
  return { label: weekdayName(days[0]!, 'long'), short: weekdayName(days[0]!, 'short') };
}

/** Day groups of the editor in the current language; `short` is used on narrow screens. */
export function dayGroups(grouping: Grouping): { days: number[]; label: string; short: string }[] {
  return GROUP_DAYS[grouping].map((days) => ({ days, ...groupLabels(days) }));
}

export const GROUPINGS: Grouping[] = ['all', 'weekdays', 'each'];

/** "Same every day", "Weekdays and weekend", "Each day separately". */
export const groupingLabel = (g: Grouping): string => t(`schedule:groups.${g}`);

/** "Tuesday to Sunday" for a run of days, else "Tuesday, Thursday". */
export function dayList(days: number[]): string {
  const name = (d: number) => weekdayName(d, 'long');
  const run = days.every((d, i) => i === 0 || d === days[i - 1]! + 1);
  if (days.length > 2 && run) return t('schedule:groups.range', { from: name(days[0]!), to: name(days.at(-1)!) });
  if (days.length === 2) return t('schedule:groups.pair', { a: name(days[0]!), b: name(days[1]!) });
  return days.map(name).join(', ');
}

/** The question before switching to fewer day groups, or null when nothing would be lost. */
export function groupingQuestion(days: Day[], grouping: Grouping): { title: string; body: string } | null {
  const lost = overwrittenBy(days, grouping);
  if (lost.length === 0) return null;
  const which = grouping === 'all' ? t('schedule:groups.everyDayLower') : dayList([0, 1, 2, 3, 4]);
  return {
    title: t('schedule:groups.questionTitle', { monday: weekdayName(0, 'long'), which }),
    body: t('schedule:groups.questionBody', { days: dayList(lost) }),
  };
}

/** The coarsest grouping that represents `days` without losing anything. */
export function detectGrouping(days: Day[]): Grouping {
  const [mon] = days;
  if (!mon) return 'each';
  if (days.every((d) => sameDay(d, mon))) return 'all';
  if (days.slice(0, 5).every((d) => sameDay(d, mon))) return 'weekdays';
  return 'each';
}

/** Writes `day` to every day of the group. */
export function setGroup(days: Day[], group: number[], day: Day): Day[] {
  return days.map((d, i) => (group.includes(i) ? day.map((s) => ({ ...s })) : d));
}

/** Which days a change of grouping would overwrite (their content differs from the group's first day). */
export function overwrittenBy(days: Day[], grouping: Grouping): number[] {
  return GROUP_DAYS[grouping].flatMap((g) => g.filter((d) => !sameDay(days[d]!, days[g[0]!]!)));
}

/** Applies a grouping: each group takes the content of its first day. */
export function applyGrouping(days: Day[], grouping: Grouping): Day[] {
  return GROUP_DAYS[grouping].reduce((acc, g) => setGroup(acc, g, acc[g[0]!]!), days);
}
