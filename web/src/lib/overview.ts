// The Overview's sentences (KWL screen spec "Overview", FanControl): pure functions of the
// status poll and the schedule settings, covered by overview.test.ts.
import type { Schemas, Status } from '../api/client';
import { t } from '../i18n';
import { clock } from '../i18n/format';
import { type Day, fromMin, normalize, toMin } from './schedule';
import { dayMonth, holidayRhythm, levelWord, minutes } from './summaries';

export type Control = Status['control'];

/** Device time "2026-10-17T14:32:05" plus `s` seconds → "14:40". */
export function clockAfter(time: string, s: number): string {
  const m = (toMin(time.slice(11, 16)) + Math.round((Number(time.slice(17, 19)) + s) / 60)) % 1440;
  return fromMin(m);
}

/** Weekday (Monday = 0) of the device's local date. */
export const weekday = (time: string) => (new Date(`${time.slice(0, 10)}T12:00:00`).getDay() + 6) % 7;
export const minuteOf = (time: string) => toMin(time.slice(11, 16));

/** How a manual choice ends: at midnight, after the timer, or not at all. */
export type ManualReturn = { kind: 'midnight' } | { kind: 'timer'; min: number } | { kind: 'never' };

/**
 * The return to Auto that comes first. `remaining_s` is the running timer (Control.auto_return_s,
 * only while manual); otherwise the configured length predicts what choosing a level would do.
 */
export function manualReturn(
  schedule: Pick<Schemas['ScheduleConfig'], 'auto_at_midnight' | 'auto_return_min'> | undefined,
  remaining_s: number | null,
  time: string | null,
): ManualReturn {
  const timerMin = remaining_s !== null ? Math.ceil(remaining_s / 60) : (schedule?.auto_return_min ?? null);
  const toMidnight = time ? 1440 - minuteOf(time) : null;
  if (timerMin !== null && (!schedule?.auto_at_midnight || toMidnight === null || timerMin < toMidnight)) {
    return { kind: 'timer', min: timerMin };
  }
  if (schedule?.auto_at_midnight ?? true) return { kind: 'midnight' };
  return { kind: 'never' };
}

/** "2 hours" for whole hours, else "1 h 45 min" / "45 min". */
const forHowLong = (m: number) => (m % 60 === 0 && m >= 60 ? t('overview:hours', { count: m / 60 }) : minutes(m));

export function stateSentence(c: Control): string {
  const lv = c.level === 0 ? t('overview:state.fansOffLower') : t('common:level', { n: c.level });
  switch (c.mode) {
    case 'holiday':
      return t('common:mode.holiday');
    case 'party':
    case 'quiet':
      return t('overview:state.program', { program: t(`common:mode.${c.mode}`), what: lv });
    default:
      return c.level === 0 ? t('overview:state.fansOff') : t('overview:state.runningAt', { n: c.level });
  }
}

export interface Next {
  level: number;
  at: string;
}

/** The weekly program's next level change, as the unit reports it. */
export function nextChange(status: Pick<Status, 'schedule'>): Next | null {
  const n = status.schedule?.next;
  return n ? { level: n.level, at: n.at } : null;
}

/** Today's switching points from 00:00 (the unit carries yesterday's level over), or null without a clock. */
export function todaySlots(status: Pick<Status, 'schedule'>): Day | null {
  return status.schedule ? normalize(status.schedule.today) : null;
}

/** Today's changes after midnight, for the Today card: "06:30 level 2". */
export function todayChanges(day: Day | null, time: string | null): { at: string; level: number; past: boolean }[] {
  if (!day || !time) return [];
  const now = minuteOf(time);
  return day.filter((s) => s.from !== '00:00').map((s) => ({ at: s.from, level: s.level, past: toMin(s.from) <= now }));
}

export interface SubLineInput {
  control: Control;
  time: string | null;
  schedule?: Pick<Schemas['ScheduleConfig'], 'auto_at_midnight' | 'auto_return_min'>;
  next: Next | null;
  /** For the burst rhythm; the end date comes from `holidayUntil`. */
  holiday?: Schemas['HolidayConfig'];
  holidayUntil?: string;
}

/** What a program returns to: "Auto", or the manual level it interrupted. */
function afterProgram(c: Control) {
  return c.base === 'auto'
    ? t('common:mode.auto')
    : c.manual_level === 0
      ? t('overview:state.fansOffLower')
      : t('common:level', { n: c.manual_level });
}

export function subLine({ control: c, time, schedule, next, holiday, holidayUntil }: SubLineInput): string {
  switch (c.mode) {
    case 'auto':
      return next ? t('overview:sub.autoNext', { level: levelWord(next.level), at: clock(next.at) }) : t('common:mode.auto');
    case 'party':
    case 'quiet':
      return time && c.remaining_s !== null
        ? t('overview:sub.untilThen', { time: clock(clockAfter(time, c.remaining_s)), after: afterProgram(c) })
        : t('overview:sub.then', { after: afterProgram(c) });
    case 'holiday': {
      const until = holidayUntil ?? holiday?.end;
      if (!until) return t('overview:sub.holidayProgram');
      return holiday
        ? t('overview:sub.untilRhythm', { date: dayMonth(until), rhythm: holidayRhythm(holiday) })
        : t('overview:sub.until', { date: dayMonth(until) });
    }
    default: {
      const r = manualReturn(schedule, c.auto_return_s, time);
      if (r.kind === 'midnight') return t('overview:sub.midnight');
      if (r.kind === 'timer') return t('overview:sub.timer', { duration: minutes(r.min) });
      return t('overview:sub.never');
    }
  }
}

/** The caption under the Mode control. */
export function hintLine(
  c: Control,
  schedule: Pick<Schemas['ScheduleConfig'], 'auto_at_midnight' | 'auto_return_min'> | undefined,
  time: string | null,
): string | null {
  if (c.mode === 'auto') {
    const r = manualReturn(schedule, null, null);
    if (r.kind === 'midnight') return t('overview:hint.autoMidnight');
    if (r.kind === 'timer') return t('overview:hint.autoTimer', { duration: forHowLong(r.min) });
    return t('overview:hint.autoNever');
  }
  if (c.mode !== 'manual') return null;
  const what = c.level === 0 ? t('overview:state.fansOff') : t('common:Level', { n: c.level });
  const r = manualReturn(schedule, c.auto_return_s, time);
  if (r.kind === 'never') return t('overview:hint.never', { what });
  const off = c.level === 0;
  if (r.kind === 'midnight') return off ? t('overview:hint.midnight', { what }) : t('overview:hint.midnightManual', { what });
  const duration = minutes(r.min);
  return off ? t('overview:hint.timer', { what, duration }) : t('overview:hint.timerManual', { what, duration });
}

/** The Today card's caption while Auto is paused. */
export function pausedLine(c: Control, schedule: SubLineInput['schedule'], time: string | null): string | null {
  if (c.mode === 'party' || c.mode === 'quiet') {
    const program = t(`common:mode.${c.mode}`);
    // Party started from a manual level returns to that level, not to Auto.
    if (c.base === 'manual') return t('overview:paused.never');
    return time && c.remaining_s !== null
      ? t('overview:paused.untilTime', { time: clock(clockAfter(time, c.remaining_s)), program })
      : t('overview:paused.untilEnds', { program });
  }
  if (c.mode !== 'manual') return null;
  const r = manualReturn(schedule, c.auto_return_s, time);
  if (r.kind === 'never') return t('overview:paused.never');
  return r.kind === 'midnight' ? t('overview:paused.midnight') : t('overview:paused.timer', { duration: minutes(r.min) });
}

/**
 * The Off confirm's sentence: how long choosing Off from `time` stops the ventilation, from the
 * schedule's return-to-Auto settings (a new manual choice starts a fresh timer).
 */
export function offConfirmLine(
  schedule: Pick<Schemas['ScheduleConfig'], 'auto_at_midnight' | 'auto_return_min'> | undefined,
  time: string | null,
): string {
  const r = manualReturn(schedule, null, time);
  if (r.kind === 'midnight') return t('overview:control.confirm.offMidnight');
  if (r.kind === 'timer') return t('overview:control.confirm.offTimer', { duration: forHowLong(r.min) });
  return t('overview:control.confirm.offNever');
}

/**
 * The confirm sentence for Quiet at level 0, started from `c` (Auto or a manual level): it stops
 * the ventilation for `min`, then the unit goes back to what ran before.
 */
export function quietOffLine(c: Control, min: number): string {
  const after = c.mode === 'manual' ? (c.level === 0 ? t('overview:state.fansOffLower') : t('common:level', { n: c.level })) : t('common:mode.auto');
  return t('overview:control.confirm.quietStops', { duration: forHowLong(min), after });
}
