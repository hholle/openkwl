// Yearly dates (`MM-DD`, e.g. the bypass season); pure, covered by monthday.test.ts.
import { locale } from '../i18n';


/** Days in a month of any year (29 February is allowed, like the firmware's `MonthDay`). */
export function daysIn(month: number): number {
  return month === 2 ? 29 : [4, 6, 9, 11].includes(month) ? 30 : 31;
}

/** `05-01` → `{ month: 5, day: 1 }`; malformed input → 1 January. */
export function parseMonthDay(s: string): { month: number; day: number } {
  const m = /^(\d{2})-(\d{2})$/.exec(s);
  const month = m ? Number(m[1]) : 1;
  if (month < 1 || month > 12) return { month: 1, day: 1 };
  const day = m ? Number(m[2]) : 1;
  return { month, day: Math.min(Math.max(day, 1), daysIn(month)) };
}

/** Keeps the day valid when the month changes (31 March → April gives 30 April). */
export function formatMonthDay(month: number, day: number): string {
  const d = Math.min(Math.max(day, 1), daysIn(month));
  return `${String(month).padStart(2, '0')}-${String(d).padStart(2, '0')}`;
}

/** `03-01` → `1 March` / `1. März`. */
export function describeMonthDay(s: string): string {
  const { month, day } = parseMonthDay(s);
  return new Intl.DateTimeFormat(locale, { day: 'numeric', month: 'long', timeZone: 'UTC' }).format(new Date(Date.UTC(2024, month - 1, day, 12)));
}
