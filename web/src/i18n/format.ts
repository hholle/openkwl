// Numbers, dates and times in the user's language and region (`Intl`, nothing bundled). The unit
// sends dates as `YYYY-MM-DD` / `MM-DD` and times as `HH:MM` in its own local time; these only
// change how they are shown, never which day or hour they mean (no time zone conversion).
import { locale } from '.';

/** 1234.5 → "1,234.5" / "1.234,5"; a true minus sign. */
export function number(n: number, decimals = 0): string {
  return new Intl.NumberFormat(locale, { minimumFractionDigits: decimals, maximumFractionDigits: decimals }).format(n).replace('-', '−');
}

// A fixed date in UTC, so month and weekday names never shift with the browser's time zone.
const utc = (y: number, mo: number, d: number) => new Date(Date.UTC(y, mo - 1, d, 12));

/** Month name, 1–12: "Oct" / "Okt." (short), "October" / "Oktober" (long). */
export function monthName(month: number, style: 'short' | 'long' = 'short'): string {
  return new Intl.DateTimeFormat(locale, { month: style, timeZone: 'UTC' }).format(utc(2024, month, 1));
}

/** Weekday name, 0 = Monday: "Mon" / "Mo" (short), "Monday" / "Montag" (long). */
export function weekdayName(day: number, style: 'short' | 'long' = 'long'): string {
  // 2024-01-01 was a Monday.
  return new Intl.DateTimeFormat(locale, { weekday: style, timeZone: 'UTC' }).format(utc(2024, 1, 1 + day));
}

/** "2026-10-24" → "24 Oct" / "Oct 24" / "24. Okt." */
export function dayMonth(date: string): string {
  const d = utc(Number(date.slice(0, 4)) || 2024, Number(date.slice(5, 7)), Number(date.slice(8, 10)));
  return new Intl.DateTimeFormat(locale, { day: 'numeric', month: 'short', timeZone: 'UTC' }).format(d);
}

/** "MM-DD" (a yearly date) → "24 Oct" / "24. Okt." */
export function monthDay(md: string): string {
  return dayMonth(`2024-${md}`);
}

/** "12–24 Oct" / "12.–24. Okt.", or across months "28 Oct – 3 Nov". */
export function dateRange(start: string, end: string): string {
  if (start === end) return dayMonth(start);
  const s = utc(Number(start.slice(0, 4)) || 2024, Number(start.slice(5, 7)), Number(start.slice(8, 10)));
  const e = utc(Number(end.slice(0, 4)) || 2024, Number(end.slice(5, 7)), Number(end.slice(8, 10)));
  return new Intl.DateTimeFormat(locale, { day: 'numeric', month: 'short', timeZone: 'UTC' }).formatRange(s, e);
}

/** "16:00" (the unit's local time) → "16:00" / "4:00 PM", as the region writes times. */
export function clock(hhmm: string): string {
  const [h, mi] = hhmm.split(':').map(Number);
  if (h === undefined || mi === undefined || Number.isNaN(h) || Number.isNaN(mi)) return hhmm;
  return new Intl.DateTimeFormat(locale, { hour: 'numeric', minute: '2-digit', timeZone: 'UTC' }).format(new Date(Date.UTC(2024, 0, 1, h, mi)));
}

/** UTC seconds → date and time in the browser's time zone, e.g. "17/10/2026, 01:25". */
export function dateTime(unixS: number): string {
  return new Intl.DateTimeFormat(locale, { dateStyle: 'short', timeStyle: 'short' }).format(new Date(unixS * 1000));
}

/** A browser timestamp (ms) → time of day with seconds, e.g. "14:41:07". */
export function timeOfDay(ms: number): string {
  return new Intl.DateTimeFormat(locale, { timeStyle: 'medium' }).format(new Date(ms));
}

/** A browser timestamp (ms) → hours and minutes, e.g. "14:41" / "2:41 PM". */
export function shortTime(ms: number): string {
  return new Intl.DateTimeFormat(locale, { hour: 'numeric', minute: '2-digit' }).format(new Date(ms));
}

/** "2026-10" → "October 2026" / "Oktober 2026". */
export function monthYear(month: string): string {
  return new Intl.DateTimeFormat(locale, { month: 'long', year: 'numeric', timeZone: 'UTC' }).format(utc(Number(month.slice(0, 4)), Number(month.slice(5, 7)), 1));
}

/** "2026-10-24" → "Sat 24" / "Sa., 24." (a day within a known month). */
export function weekdayDay(date: string): string {
  const d = utc(Number(date.slice(0, 4)), Number(date.slice(5, 7)), Number(date.slice(8, 10)));
  return new Intl.DateTimeFormat(locale, { weekday: 'short', day: 'numeric', timeZone: 'UTC' }).format(d);
}

/** "2026-10-24" → "Saturday, 24 October 2026" / "Samstag, 24. Oktober 2026". */
export function longDate(date: string): string {
  const d = utc(Number(date.slice(0, 4)), Number(date.slice(5, 7)), Number(date.slice(8, 10)));
  return new Intl.DateTimeFormat(locale, { dateStyle: 'full', timeZone: 'UTC' }).format(d);
}
