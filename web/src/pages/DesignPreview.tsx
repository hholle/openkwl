// Development only (`pnpm dev`, /design): every AirflowSchematic state at both sizes, laid out
// like the design system's preview so the two can be compared side by side. Not in the build.
import { Box, Card, Group, SimpleGrid, Text } from '@mantine/core';
import { ApiError, api, type Status } from '../api/client';
import { AirflowSchematic, type AirflowProps } from '../components/AirflowSchematic';
import { StatusBadge } from '../components/StatusBadge';
import { concat, encodeRecord, type RecordInit, sampleDay } from '../lib/dayRecordFixtures';
import { statusWith } from '../lib/fixtures';
import { recordLive } from '../lib/history';
import { HistoryPage } from './History';
import { Overview } from './Overview';

const NORMAL = { outdoor: 9.2, exhaust: 12.5, supply: 19.6, extract: 21.9, efficiency: 83 };
const SUMMER = { outdoor: 14.1, exhaust: 22.8, supply: 14.6, extract: 23.4, efficiency: 12 };
const COLD = { outdoor: -6.4, exhaust: 3.1, supply: 15.2, extract: 21.7, efficiency: 61 };
const RUN = { rpm: 1350, stalled: false };

const base: Omit<AirflowProps, 'size'> = {
  ...NORMAL,
  bypass: 'closed',
  frost: 'off',
  supplyFan: RUN,
  exhaustFan: RUN,
};

const STATES: [string, Omit<AirflowProps, 'size'>, ('desktop' | 'phone')?][] = [
  ['Normal: heat recovery', base],
  ['Bypass open: summer cooling', { ...base, ...SUMMER, bypass: 'open' }],
  ['Frost protection: exhaust fan boosted', { ...base, ...COLD, frost: 'boost', exhaustFan: { rpm: 1900, stalled: false } }],
  ['Defrosting: fresh air paused', { ...base, ...COLD, frost: 'defrost', supplyFan: { rpm: 0, stalled: false } }],
  ['Fault: supply fan stopped', { ...base, supplyFan: { rpm: 0, stalled: true } }],
  ['Bypass moving', { ...base, ...SUMMER, bypass: 'moving' }],
  ['Fans off', { ...base, supplyFan: { rpm: 0, stalled: false }, exhaustFan: { rpm: 0, stalled: false } }],
  ['Offline', { ...base, offlineSince: '14:31' }],
  ['Sensor missing', { ...base, outdoor: null, efficiency: null }],
  ['Level 4 (fast)', { ...base, supplyFan: { rpm: 2700, stalled: false }, exhaustFan: { rpm: 2700, stalled: false } }],
  ['Phone: normal', base, 'phone'],
  ['Phone: bypass open', { ...base, ...SUMMER, bypass: 'open' }, 'phone'],
  ['Phone: defrosting', { ...base, ...COLD, frost: 'defrost', supplyFan: { rpm: 0, stalled: false } }, 'phone'],
];

const COLD_T = { outdoor: -6.4, exhaust: 3.1, supply: 15.2, extract: 21.7 };

/** Overview with a fixed status: /design?overview=<name> (offline: …&offline). */
const OVERVIEW: Record<string, Status> = {
  normal: statusWith({}),
  'bypass-open': statusWith({ bypass: { open: true, moving: false }, temperatures: { outdoor: 14.1, exhaust: 22.8, supply: 14.6, extract: 23.4 }, heat_recovery: { power_w: 0 } }),
  'bypass-moving': statusWith({ bypass: { open: false, moving: true } }),
  frost: statusWith({ frost: { active: true, state: 'boost_hold', exhaust_boost_v: 1.2 }, temperatures: COLD_T, efficiency_pct: 61 }),
  defrost: statusWith({
    frost: { active: true, defrost: true, supply_stopped: true, state: 'defrost' },
    temperatures: COLD_T,
    supply_fan: { rpm: 0, volts: 0, duty_pct: 0, airflow_m3h: null },
    heat_recovery: { power_w: 0 },
  }),
  'fan-fault': statusWith({
    faults: ['supply_fan_stall'],
    control: { mode: 'manual', level: 3, auto_return_s: null },
    supply_fan: { stalled: true, rpm: 0, airflow_m3h: 0 },
    heat_recovery: { power_w: 0 },
  }),
  'fans-off': statusWith({ control: { mode: 'manual', level: 0, base: 'manual', manual_level: 0 }, supply_fan: { rpm: 0, volts: 0, duty_pct: 0, airflow_m3h: null }, exhaust_fan: { rpm: 0, volts: 0, duty_pct: 0, airflow_m3h: null }, heat_recovery: { power_w: 0 } }),
  holiday: statusWith({ control: { mode: 'holiday', level: 1 }, holiday: { until: '2026-10-24', burst: true } }),
  party: statusWith({ control: { mode: 'party', level: 4, remaining_s: 2 * 3600 + 8 * 60 }, supply_fan: { rpm: 2700 }, exhaust_fan: { rpm: 2700 } }),
  'filter-due': statusWith({ faults: ['filter_change'], filter: { remaining_days: 0, due: true } }),
  'two-conditions': statusWith({ faults: ['supply_fan_stall', 'filter_change'], filter: { remaining_days: 0, due: true }, supply_fan: { stalled: true, rpm: 0, airflow_m3h: 0 }, control: { mode: 'manual', level: 3, base: 'manual', manual_level: 3 } }),
  'three-conditions': statusWith({ faults: ['sd_card', 'filter_change', 'time_not_set'], filter: { remaining_days: 0, due: true }, frost: { active: true } }),
  'clock-not-set': statusWith({ faults: ['time_not_set'], schedule: null }),
  manual: statusWith({ control: { mode: 'manual', level: 3, base: 'manual', manual_level: 3 } }),
};

/** History with generated day records: /design?history=<view> (live, day, month; nothing
 *  recorded: …&empty; firmware without history: …&missing). */
function mockHistory(q: URLSearchParams) {
  const today = SAMPLE_TIME.slice(0, 10);
  const notFound = () => Promise.reject(new ApiError(404, null));
  if (q.has('missing')) {
    api.historyToday = notFound;
    api.historyDays = notFound;
    return;
  }
  if (q.has('empty')) {
    api.historyToday = async () => encodeRecord({ date: today, recordedS: 60, levelS: [0, 0, 60, 0, 0] });
    api.historyDays = (d) => (d.length === 7 ? Promise.resolve(new ArrayBuffer(0)) : notFound());
    return;
  }
  const withBands = (r: RecordInit): RecordInit => ({
    ...r,
    bypassS: 3 * 3600,
    frostS: 3600,
    quarters: r.quarters?.map((x, i) => (x ? { ...x, bypass: i >= 52 && i < 64, frost: i >= 20 && i < 24 } : x)),
  });
  const day = (date: string): RecordInit => {
    const n = Number(date.slice(8, 10));
    const r = sampleDay(date, { upTo: n === 3 ? 40 : 96, shift: 4 * Math.sin(n / 3) - n / 4 });
    return n % 5 === 0 ? withBands(r) : r;
  };
  api.historyToday = async () => encodeRecord(sampleDay(today, { upTo: 58, shift: -3 }));
  api.historyDays = async (d) => {
    if (d.length === 10) return d < today ? encodeRecord(day(d)) : notFound();
    const dates = Array.from({ length: 31 }, (_, i) => `${d}-${String(i + 1).padStart(2, '0')}`).filter((x) => x < today && x !== `${d}-08`);
    return concat(dates.map((x) => encodeRecord(day(x))));
  };
  // Half an hour of live readings, with a gap where the page was closed.
  const now = Date.now();
  for (let s = 1800; s > 0; s -= 10) {
    if (s > 900 && s < 1200) continue;
    recordLive(statusWith({ temperatures: { outdoor: 9 + Math.sin(s / 300), supply: 19.6, extract: 21.9 + Math.cos(s / 400) / 2, exhaust: 12.5 }, control: { level: s < 600 ? 3 : 2 } }), now - s * 1000);
  }
}

const SAMPLE_TIME = statusWith({}).time!;

export function DesignPreview() {
  const q = new URLSearchParams(window.location.search);
  const history = q.get('history');
  if (history) {
    mockHistory(q);
    return (
      <Box className="app-main" mih="100vh" p={{ base: 'md', sm: 'xl' }}>
        <Box maw={1040} mx="auto">
          <HistoryPage view={history} status={statusWith({})} />
        </Box>
      </Box>
    );
  }
  const name = q.get('overview');
  if (name) {
    const status = OVERVIEW[name] ?? OVERVIEW.normal!;
    const offline = q.has('offline');
    return (
      <Box className="app-main" mih="100vh" p={{ base: 'md', sm: 'xl' }}>
        <Box maw={1040} mx="auto">
          <Group justify="space-between" mb="lg">
            <Text size="xs" c="dimmed">
              {name}
              {offline ? ' (offline)' : ''}
            </Text>
            <StatusBadge status={status} offline={offline} />
          </Group>
          <Overview status={status} offline={offline} onChanged={async () => undefined} />
        </Box>
      </Box>
    );
  }
  return (
    <Box p="md" maw={1240}>
      <SimpleGrid cols={{ base: 1, md: 2 }} spacing={18}>
        {STATES.map(([label, props, size]) => (
          <div key={label}>
            <Text size="xs" fw={600} c="dimmed" mb={6} ml={2}>
              {label}
            </Text>
            <Card padding={0} px={18} py={16} w={size === 'phone' ? 351 : undefined}>
              <AirflowSchematic {...props} size={size ?? 'desktop'} />
            </Card>
          </div>
        ))}
      </SimpleGrid>
    </Box>
  );
}
