// openKWL — one formatter per kind of reading, so tiles, tables and Home Assistant agree.
// formatReading(21.46, 'temperature', 'en-GB') → "21.5 °C"
// formatReading(47.8, 'humidity', 'de-DE')     → "48 %"
// formatReading(null, 'co2', 'en-GB')          → "—"
export type ReadingKind = 'temperature' | 'humidity' | 'co2' | 'voc' | 'airflow' | 'fanSpeed';

const SPEC: Record<ReadingKind, { unit: string; decimals: number }> = {
  temperature: { unit: '°C', decimals: 1 },
  humidity: { unit: '%', decimals: 0 },
  co2: { unit: 'ppm', decimals: 0 },
  voc: { unit: 'ppb', decimals: 0 },
  airflow: { unit: 'm³/h', decimals: 0 },
  fanSpeed: { unit: 'rpm', decimals: 0 },
};

export function formatReading(value: number | null | undefined, kind: ReadingKind, locale: string): string {
  if (value === null || value === undefined || Number.isNaN(value)) return '—';
  const { unit, decimals } = SPEC[kind];
  const n = new Intl.NumberFormat(locale, {
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
  })
    .format(value)
    .replace('-', '−');
  return `${n}\u00a0${unit}`; // non-breaking space (Geist has no narrow-space glyph)
}
