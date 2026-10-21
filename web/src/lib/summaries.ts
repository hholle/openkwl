// One-line summaries for SectionList rows and the Overview sentences (pure; summaries.test.ts).
import type { Schemas } from '../api/client';
import { reading } from './readings';
import { t } from '../i18n';

/** 45 → "45 min", 60 → "1 h", 105 → "1 h 45 min". */
export function minutes(m: number): string {
  const total = Math.round(m);
  if (total < 60) return `${total} min`;
  const h = Math.floor(total / 60);
  const rest = total % 60;
  return rest ? `${h} h ${rest} min` : `${h} h`;
}

// Dates in the user's language and region; re-exported where the pages already import them.
import { dateRange, dayMonth, monthName } from '../i18n/format';

export { dateRange, dayMonth };

export const levelWord = (l: number) => (l === 0 ? t('offWord') : t('level', { n: l }));

export function holidaySummary(h: Schemas['HolidayConfig'] | undefined): string | undefined {
  if (!h) return undefined;
  return h.enabled && h.start && h.end ? dateRange(h.start, h.end) : t('off');
}

/** "level 1 for 60 min every 2 h" */
export function holidayRhythm(h: Schemas['HolidayConfig']): string {
  return t('schedule:summary.holidayRhythm', { level: levelWord(h.level), min: h.active_min, h: h.interval_h });
}

export function programsSummary(p: Schemas['ProgramConfig'] | undefined): string | undefined {
  if (!p) return undefined;
  return t('schedule:summary.programs', {
    partyLevel: levelWord(p.party_level),
    partyLength: minutes(p.party_min),
    quietLevel: levelWord(p.quiet_level),
    quietLength: minutes(p.quiet_min),
  });
}

export function coolingSummary(b: Schemas['BypassConfig'] | undefined): string | undefined {
  if (!b) return undefined;
  if (!b.enabled) return t('off');
  // "23 °C", "23.5 °C": no ".0" / ",0" for whole degrees.
  const temp = reading(b.target_c, 'temperature').replace(/[.,]0(?=\u00a0)/, '');
  return b.season
    ? t('schedule:summary.coolingSeason', { from: monthName(Number(b.season.start.slice(0, 2))), to: monthName(Number(b.season.end.slice(0, 2))), temp })
    : t('schedule:summary.coolingOn', { temp });
}

export function backToAutoSummary(s: Schemas['ScheduleConfig'] | undefined): string | undefined {
  if (!s) return undefined;
  const after = s.auto_return_min !== null ? minutes(s.auto_return_min) : null;
  if (s.auto_at_midnight && after) return t('schedule:summary.midnightOrAfter', { time: after });
  if (s.auto_at_midnight) return t('schedule:summary.atMidnight');
  return after ? t('schedule:summary.after', { time: after }) : t('schedule:summary.never');
}

export function filterSummary(f: Schemas['FilterStatus'] | undefined, interval: number | undefined): string | undefined {
  if (!f) return undefined;
  if (f.due) return t('schedule:summary.filterDue');
  if (interval === 0) return t('schedule:summary.reminderOff');
  return t('schedule:summary.daysLeft', { count: f.remaining_days });
}

/** "Running for 12 days" */
export function uptime(s: number): string {
  const d = Math.floor(s / 86400);
  if (d >= 1) return t('schedule:summary.runningDays', { count: d });
  const h = Math.floor(s / 3600);
  return h >= 1 ? t('schedule:summary.runningHours', { h }) : t('schedule:summary.runningMinutes', { min: Math.max(1, Math.floor(s / 60)) });
}
