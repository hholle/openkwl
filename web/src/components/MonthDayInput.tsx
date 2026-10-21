import { Group, Select } from '@mantine/core';
import { useTranslation } from 'react-i18next';
import { monthName } from '../i18n/format';
import { daysIn, formatMonthDay, parseMonthDay } from '../lib/monthday';

/** A day of the year without a year (`MM-DD`): day and month pickers side by side. */
export function MonthDayInput({ label, value, onChange }: { label: string; value: string; onChange: (v: string) => void }) {
  const { t } = useTranslation('schedule');
  const { month, day } = parseMonthDay(value);
  const months = Array.from({ length: 12 }, (_, i) => ({ value: String(i + 1), label: monthName(i + 1, 'long') }));
  const days = Array.from({ length: daysIn(month) }, (_, i) => String(i + 1));
  return (
    <Group gap="xs" wrap="nowrap" align="flex-end" role="group" aria-label={label}>
      <Select
        label={label}
        aria-label={t('monthDay.day', { label })}
        data={days}
        value={String(day)}
        onChange={(d) => onChange(formatMonthDay(month, Number(d)))}
        allowDeselect={false}
        w={84}
      />
      <Select
        aria-label={t('monthDay.month', { label })}
        data={months}
        value={String(month)}
        onChange={(m) => onChange(formatMonthDay(Number(m), day))}
        allowDeselect={false}
        style={{ flex: 1 }}
      />
    </Group>
  );
}
