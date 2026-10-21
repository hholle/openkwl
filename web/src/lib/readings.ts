// Readings beyond the kinds in theme/units.ts (which stays as published), formatted by the
// same rules: fixed decimals per kind, the locale's decimal mark, a no-break space before the unit,
// a true minus sign, "—" for no value.
import { formatReading, type ReadingKind } from '../theme/units';
import { locale, t } from '../i18n';

type ExtraKind = 'percent' | 'power' | 'energy' | 'voltage' | 'hours' | 'days';

const EXTRA: Record<ExtraKind, { unit: string; decimals: number }> = {
  percent: { unit: '%', decimals: 0 },
  power: { unit: 'W', decimals: 0 },
  energy: { unit: 'kWh', decimals: 1 },
  voltage: { unit: 'V', decimals: 1 },
  hours: { unit: 'h', decimals: 0 },
  days: { unit: '', decimals: 0 },
};

/** A reading in the current language's number format (`i18n`), unless `loc` is given. */
export function reading(value: number | null | undefined, kind: ReadingKind | ExtraKind, loc: string = locale): string {
  if (!(kind in EXTRA)) return formatReading(value, kind as ReadingKind, loc);
  if (value === null || value === undefined || Number.isNaN(value)) return '—';
  const { unit, decimals } = EXTRA[kind as ExtraKind];
  const n = new Intl.NumberFormat(loc, { minimumFractionDigits: decimals, maximumFractionDigits: decimals })
    .format(value)
    .replace('-', '−');
  return `${n}\u00a0${kind === 'days' ? t('days', { count: value }) : unit}`;
}

/** Splits "21.5 °C" into number and unit for tiles (unit in ink-muted at 70 %). */
export function splitUnit(text: string): [string, string] {
  const i = text.indexOf('\u00a0');
  return i < 0 ? [text, ''] : [text.slice(0, i), text.slice(i + 1)];
}
