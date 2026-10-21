// Formatting helpers shared by the pages (pure; covered by format.test.ts).
import { t } from '../i18n';
import { dateTime, number } from '../i18n/format';

/** Seconds → `2 h 05 min`, `12 min`, `45 s`. */
export function duration(s: number): string {
  if (s < 60) return `${Math.max(0, Math.round(s))} s`;
  const m = Math.ceil(s / 60);
  if (m < 60) return `${m} min`;
  const h = Math.floor(m / 60);
  const rest = m % 60;
  return rest ? `${h} h ${String(rest).padStart(2, '0')} min` : `${h} h`;
}

/** Weekly profile 1–5: "Standard 1–3", "User defined 1–2" in the current language. */
export function profileName(n: number): string {
  if (n < 1 || n > 5) return '';
  return n <= 3 ? t('schedule:profile.standard', { n }) : t('schedule:profile.user', { n: n - 3 });
}

/** The five profile names; each entry is read when used, so it follows the language. */
export const PROFILE_NAMES: readonly string[] = Object.freeze(
  Object.defineProperties(
    [] as string[],
    Object.fromEntries([1, 2, 3, 4, 5].map((n) => [n - 1, { get: () => profileName(n), enumerable: true }])),
  ),
);

/** Bytes as `12.3 KB` / `1.5 MB` (1 KB = 1024 B, like the flash and RAM sizes). */
export function bytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${number(n / 1024, 1)} KB`;
  return `${number(n / 1024 / 1024, 1)} MB`;
}

/** `0x411` */
export function hex(n: number, digits = 3): string {
  return `0x${n.toString(16).padStart(digits, '0')}`;
}

/** UTC seconds → local date and time in the browser's time zone, e.g. `17/10/2026, 01:25`. */
export function unixTime(s: number): string {
  return dateTime(s);
}
