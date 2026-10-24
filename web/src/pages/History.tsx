import { BarChart, LineChart } from '@mantine/charts';
import { ActionIcon, Alert, Anchor, Button, Card, Group, SegmentedControl, Skeleton, Stack, Table, Text, Title, Tooltip } from '@mantine/core';
import { useMediaQuery } from '@mantine/hooks';
import { IconAlertTriangle, IconChevronLeft, IconChevronRight } from '@tabler/icons-react';
import { type ReactNode, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ApiError, api, type Status } from '../api/client';
import { actionError } from '../components/Feedback';
import { type Axis, EmptyNote, HistoryCharts, SENSOR_COLOR, type Stat, StatGrid } from '../components/HistoryCharts';
import { t as tr } from '../i18n';
import { clock, longDate, monthYear, number, shortTime, weekdayDay } from '../i18n/format';
import {
  addDays,
  addMonths,
  type DayRecord,
  type DayStats,
  dayStats,
  daysInMonth,
  decodeRecord,
  decodeRecords,
  type MonthTotals,
  monthTotals,
  RecordError,
  type Sensor,
  SENSORS,
} from '../lib/dayRecord';
import { clockTicks, dayWindow, hhmm, isEmpty, niceTicks, last24h, LIVE_STEP_MS, QUARTER_MIN, type QuarterWindow, quarterOfTime, recordLive, timeTicks, useLive, withGaps } from '../lib/history';
import { reading } from '../lib/readings';
import { navigate, pathOf } from '../lib/router';
import { minutes } from '../lib/summaries';
import { TAP } from '../lib/tap';
import { documentTitle } from '../lib/title';

const VIEWS = ['live', 'last-24-hours', 'day', 'month'] as const;
type View = (typeof VIEWS)[number];
const DEFAULT_VIEW: View = 'last-24-hours';
/** Today's record grows every 15 minutes; look again once a minute. */
const TODAY_REFRESH_MS = 60_000;

// The day and month shown, kept while navigating between pages.
let shownDay: string | null = null;
let shownMonth: string | null = null;

/** The unit's local date and time (`YYYY-MM-DDTHH:MM`), else the browser's. */
function localNow(status: Status | null): string {
  if (status?.time) return status.time.slice(0, 16);
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}`;
}

/** History: live while the page is open, the last 24 hours, one day, one month. */
export function HistoryPage({ view, status }: { view: string | null; status: Status | null }) {
  const { t } = useTranslation('history');
  const isPhone = !useMediaQuery('(min-width: 48em)', false, { getInitialValueInEffect: false });
  const v: View = VIEWS.find((x) => x === view) ?? DEFAULT_VIEW;
  const now = localNow(status);
  const today = now.slice(0, 10);

  useEffect(() => {
    document.title = documentTitle(t('title'));
  }, [t]);
  // The live view's buffer fills from the app's 1 s status poll while this page is open.
  useEffect(() => {
    if (status) recordLive(status);
  }, [status]);

  return (
    <Stack gap="lg">
      <Stack gap={4}>
        <Title order={1}>{t('title')}</Title>
        <Text c="dimmed">{t('intro')}</Text>
      </Stack>
      <SegmentedControl
        aria-label={t('views.label')}
        fullWidth={isPhone}
        maw={isPhone ? undefined : 480}
        value={v}
        onChange={(x) => navigate(pathOf('history', x === DEFAULT_VIEW ? null : x), { replace: true })}
        data={[
          { value: 'live', label: t('views.live') },
          { value: 'last-24-hours', label: t('views.last24') },
          { value: 'day', label: t('views.day') },
          { value: 'month', label: t('views.month') },
        ]}
      />
      {v === 'live' ? (
        <LiveView isPhone={isPhone} />
      ) : v === 'day' ? (
        <DayView today={today} isPhone={isPhone} />
      ) : v === 'month' ? (
        <MonthView today={today} isPhone={isPhone} />
      ) : (
        <Last24View today={today} nowQuarter={quarterOfTime(now)} isPhone={isPhone} />
      )}
    </Stack>
  );
}

// ---- loading ----

type Loaded<T> = { state: 'loading' } | { state: 'ok'; data: T } | { state: 'missing' } | { state: 'error'; message: string };

/** Loads `load` for `key` (again when the key changes, and every `refreshMs` while visible). A
 *  404 is "missing"; a refresh keeps showing the previous answer until the new one is in. */
function useHistory<T>(key: string, load: () => Promise<T>, refreshMs?: number): [Loaded<T>, () => void] {
  const [result, setResult] = useState<{ key: string; value: Loaded<T> }>({ key, value: { state: 'loading' } });
  const [nonce, setNonce] = useState(0);
  useEffect(() => {
    let alive = true;
    load().then(
      (data) => alive && setResult({ key, value: { state: 'ok', data } }),
      (e: unknown) => {
        if (!alive) return;
        const value: Loaded<T> =
          e instanceof ApiError && e.status === 404
            ? { state: 'missing' }
            : { state: 'error', message: e instanceof RecordError ? tr('history:load.unreadable') : `${tr('history:load.failed')} ${actionError(e)}` };
        setResult({ key, value });
      },
    );
    return () => {
      alive = false;
    };
    // `load` follows `key`.
  }, [key, nonce]);
  useEffect(() => {
    if (!refreshMs) return;
    const id = setInterval(() => document.visibilityState === 'visible' && setNonce((n) => n + 1), refreshMs);
    return () => clearInterval(id);
  }, [refreshMs]);
  return [result.key === key ? result.value : { state: 'loading' }, () => setNonce((n) => n + 1)];
}

function LoadError({ message, retry }: { message: string; retry: () => void }) {
  const { t } = useTranslation('history');
  return (
    <Alert color="red" variant="light" icon={<IconAlertTriangle size={18} stroke={1.8} />}>
      <Stack gap="xs" align="flex-start">
        <Text size="sm">{message}</Text>
        <Button variant="default" size="xs" onClick={retry}>
          {t('load.retry')}
        </Button>
      </Stack>
    </Alert>
  );
}

/** The axis of a quarter-hour window: full 3 hours (6 on phones) of the clock. */
function quarterAxis(w: QuarterWindow, isPhone: boolean): Axis {
  return {
    domain: [0, 24 * 60],
    ticks: clockTicks(w.startMin, isPhone ? 360 : 180),
    format: (x) => clock(hhmm(w.startMin, x)),
    step: QUARTER_MIN,
  };
}

// ---- live ----

function LiveView({ isPhone }: { isPhone: boolean }) {
  const { t } = useTranslation('history');
  const points = useLive();
  const first = points[0]?.x ?? 0;
  const last = points[points.length - 1]?.x ?? 0;
  const end = Math.max(last, first + 60_000);
  const axis: Axis = { domain: [first, end], ticks: timeTicks(first, end, isPhone ? 3 : 5), format: shortTime, step: LIVE_STEP_MS };
  return (
    <Card p={{ base: 'md', sm: 'lg' }}>
      <Stack gap="md">
        <Stack gap={4}>
          <Title order={2}>{t('live.title')}</Title>
          <Text size="sm" c="dimmed">
            {t('live.note')}
          </Text>
        </Stack>
        {points.length < 2 ? <EmptyNote title={t('live.waiting')}>{''}</EmptyNote> : <HistoryCharts points={withGaps(points)} axis={axis} isPhone={isPhone} />}
      </Stack>
    </Card>
  );
}

// ---- last 24 hours ----

async function loadLast24(today: string): Promise<{ yesterday: DayRecord | null; today: DayRecord }> {
  const [now, before] = await Promise.all([
    api.historyToday().then((b) => decodeRecord(b)),
    api
      .historyDays(addDays(today, -1))
      .then((b) => decodeRecord(b))
      .catch((e: unknown) => {
        if (e instanceof ApiError && e.status === 404) return null;
        throw e;
      }),
  ]);
  return { yesterday: before, today: now };
}

function Last24View({ today, nowQuarter, isPhone }: { today: string; nowQuarter: number | null; isPhone: boolean }) {
  const { t } = useTranslation('history');
  const [r, retry] = useHistory(`24h ${today}`, () => loadLast24(today), TODAY_REFRESH_MS);
  let body;
  if (r.state === 'loading') body = <Skeleton h={420} />;
  else if (r.state === 'error') body = <LoadError message={r.message} retry={retry} />;
  else if (r.state === 'missing') body = <EmptyNote title={t('load.unsupported')}>{''}</EmptyNote>;
  else {
    const w = last24h(r.data.yesterday, r.data.today, nowQuarter);
    body = isEmpty(w.points) ? (
      <EmptyNote title={t('load.empty')}>{t('load.emptyBody')}</EmptyNote>
    ) : (
      <HistoryCharts points={w.points} axis={quarterAxis(w, isPhone)} isPhone={isPhone} />
    );
  }
  return (
    <Card p={{ base: 'md', sm: 'lg' }}>
      <Stack gap="md">
        <Stack gap={4}>
          <Title order={2}>{t('last24.title')}</Title>
          <Text size="sm" c="dimmed">
            {t('last24.note')}
          </Text>
        </Stack>
        {body}
      </Stack>
    </Card>
  );
}

// ---- statistics ----

/** Hours as "45 min", "3 h 15 min" below 10 h, else "212 h". */
function hoursText(h: number): string {
  return h < 10 ? minutes(h * 60) : reading(h, 'hours');
}

function statItems(s: DayStats | MonthTotals, recorded: string): Stat[] {
  const temp = (k: Sensor, label: string): Stat => {
    const r = s.temps[k];
    return {
      label,
      value: reading(r?.mean, 'temperature'),
      detail: r ? tr('history:stats.range', { min: reading(r.min, 'temperature'), max: reading(r.max, 'temperature') }) : undefined,
    };
  };
  const rpm = (v: number | null) => (v === null || v === 0 ? '—' : number(v));
  return [
    { label: tr('history:stats.heating'), value: reading(s.heatingKwh, 'energy') },
    { label: tr('history:stats.cooling'), value: reading(s.coolingKwh, 'energy') },
    { label: tr('history:stats.efficiency'), value: reading(s.efficiency === null ? null : s.efficiency * 100, 'percent'), hint: tr('history:stats.efficiencyHint') },
    { label: tr('history:stats.degreeDays'), value: reading(s.degreeDays, 'degreeDays'), hint: tr('history:stats.degreeDaysHint') },
    temp('outdoor', tr('history:stats.outdoorMean')),
    temp('extract', tr('history:stats.extractMean')),
    { label: tr('history:stats.bypass'), value: hoursText(s.bypassH) },
    { label: tr('history:stats.frost'), value: hoursText(s.frostH) },
    { label: tr('history:stats.fans'), value: tr('history:stats.fansValue', { supply: rpm(s.supplyRpm), exhaust: rpm(s.exhaustRpm) }), detail: 'rpm', hint: tr('history:stats.fansHint') },
    { label: tr('history:stats.recorded'), value: recorded },
  ];
}

function Nav({ title, previous, next, onPrevious, onNext, children }: { title: string; previous: string; next: string; onPrevious: () => void; onNext?: () => void; children?: ReactNode }) {
  return (
    <Group justify="space-between" wrap="nowrap" gap="xs">
      <Group gap="xs" wrap="nowrap" miw={0}>
        <ActionIcon variant="default" size={42} aria-label={previous} onClick={onPrevious}>
          <IconChevronLeft size={18} />
        </ActionIcon>
        <ActionIcon variant="default" size={42} aria-label={next} onClick={onNext} disabled={!onNext}>
          <IconChevronRight size={18} />
        </ActionIcon>
        <Title order={2} fz={{ base: 'md', sm: 'lg' }} style={{ minWidth: 0 }}>
          {title}
        </Title>
      </Group>
      {children}
    </Group>
  );
}

// ---- day ----

function DayView({ today, isPhone }: { today: string; isPhone: boolean }) {
  const { t } = useTranslation('history');
  const [date, setDateState] = useState(() => (shownDay && shownDay <= today ? shownDay : today));
  const setDate = (d: string) => {
    shownDay = d === today ? null : d;
    setDateState(d);
  };
  const isToday = date >= today;
  const [r, retry] = useHistory(`day ${date}`, () => (isToday ? api.historyToday() : api.historyDays(date)).then((b) => decodeRecord(b)), isToday ? TODAY_REFRESH_MS : undefined);

  let body;
  if (r.state === 'loading') body = <Skeleton h={420} />;
  else if (r.state === 'error') body = <LoadError message={r.message} retry={retry} />;
  else if (r.state === 'missing') body = <EmptyNote title={isToday ? t('load.unsupported') : t('day.empty')}>{isToday ? '' : t('day.emptyBody')}</EmptyNote>;
  else {
    const w = dayWindow(r.data);
    const s = dayStats(r.data);
    body = isEmpty(w.points) ? (
      <EmptyNote title={isToday ? t('load.empty') : t('day.empty')}>{isToday ? t('load.emptyBody') : t('day.emptyBody')}</EmptyNote>
    ) : (
      <Stack gap="lg">
        <StatGrid items={statItems(s, hoursText(s.recordedH))} />
        <HistoryCharts points={w.points} axis={quarterAxis(w, isPhone)} isPhone={isPhone} />
      </Stack>
    );
  }
  return (
    <Card p={{ base: 'md', sm: 'lg' }}>
      <Stack gap="md">
        <Nav title={longDate(date)} previous={t('day.previous')} next={t('day.next')} onPrevious={() => setDate(addDays(date, -1))} onNext={isToday ? undefined : () => setDate(addDays(date, 1))}>
          {!isToday && !isPhone && (
            <Button variant="subtle" onClick={() => setDate(today)}>
              {t('day.today')}
            </Button>
          )}
        </Nav>
        {!isToday && isPhone && (
          <Anchor component="button" size="sm" c="var(--app-brand-text)" style={{ ...TAP, alignSelf: 'flex-start' }} onClick={() => setDate(today)}>
            {t('day.today')}
          </Anchor>
        )}
        {body}
      </Stack>
    </Card>
  );
}

// ---- month ----

function MonthView({ today, isPhone }: { today: string; isPhone: boolean }) {
  const { t } = useTranslation(['history', 'common']);
  const current = today.slice(0, 7);
  const [month, setMonthState] = useState(() => (shownMonth && shownMonth <= current ? shownMonth : current));
  const setMonth = (m: string) => {
    shownMonth = m === current ? null : m;
    setMonthState(m);
  };
  const [sensor, setSensor] = useState<Sensor>('outdoor');
  const [r, retry] = useHistory(`month ${month}`, () => api.historyDays(month).then(decodeRecords));
  const openDay = (date: string) => {
    shownDay = date;
    navigate(pathOf('history', 'day'));
  };

  let body;
  if (r.state === 'loading') body = <Skeleton h={420} />;
  else if (r.state === 'error') body = <LoadError message={r.message} retry={retry} />;
  else if (r.state === 'missing' || r.data.length === 0) body = <EmptyNote title={t('month.empty', { month: monthYear(month) })}>{t('month.emptyBody')}</EmptyNote>;
  else {
    const days = r.data.map(dayStats);
    const total = monthTotals(days);
    const n = month === current ? Number(today.slice(8, 10)) : daysInMonth(month);
    const byDay = new Map(days.map((d) => [Number(d.date.slice(8, 10)), d]));
    // Every day of the month (up to today) on the axis; days without a record stay empty.
    const rows = Array.from({ length: n }, (_, i) => {
      const d = byDay.get(i + 1);
      const range = d?.temps[sensor];
      return {
        day: i + 1,
        min: range?.min ?? null,
        mean: range?.mean ?? null,
        max: range?.max ?? null,
        heating: d?.heatingKwh ?? null,
        cooling: d?.coolingKwh ?? null,
        ...Object.fromEntries([0, 1, 2, 3, 4].map((l) => [`l${l}`, d ? d.levelH[l] : null])),
      };
    });
    const dayLabel = (x: unknown) => (typeof x === 'number' && x >= 1 ? weekdayDay(`${month}-${String(x).padStart(2, '0')}`) : '');
    const tempTicks = niceTicks(rows.flatMap((r) => [r.min, r.max]));
    const energyTicks = niceTicks(rows.map((r) => (r.heating ?? 0) + (r.cooling ?? 0)), 3, true);
    const xAxisProps = { interval: isPhone ? 4 : 1 } as const;
    const color = SENSOR_COLOR[sensor];
    const faint = `color-mix(in srgb, ${color} 45%, transparent)`;
    body = (
      <Stack gap="lg">
        <StatGrid items={statItems(total, t('stats.daysValue', { days: total.days, total: n }))} />

        <Stack gap="xs">
          <Title order={3}>{t('month.temperatures')}</Title>
          <Text size="xs" c="dimmed">
            {t('month.range')}
          </Text>
          <SegmentedControl
            size="xs"
            fullWidth={isPhone}
            maw={isPhone ? undefined : 480}
            value={sensor}
            onChange={(x) => setSensor(x as Sensor)}
            data={SENSORS.map((s) => ({ value: s, label: t(`sensor.${s}`) }))}
          />
          <LineChart
            h={isPhone ? 200 : 240}
            data={rows}
            dataKey="day"
            withLegend
            series={[
              { name: 'max', label: t('month.highest'), color: faint },
              { name: 'mean', label: t('month.mean'), color },
              { name: 'min', label: t('month.lowest'), color: faint, strokeDasharray: '4 3' },
            ]}
            withDots={!isPhone}
            strokeWidth={1.75}
            connectNulls={false}
            gridAxis="x"
            valueFormatter={(v) => reading(v, 'temperature')}
            yAxisProps={{ width: 40, tickFormatter: (v: number) => number(v), domain: [tempTicks[0]!, tempTicks[tempTicks.length - 1]!], ticks: tempTicks, interval: 0 }}
            xAxisProps={xAxisProps}
            tooltipProps={{ labelFormatter: dayLabel }}
          />
        </Stack>

        <Stack gap="xs">
          <Title order={3}>{t('month.energy')}</Title>
          <BarChart
            h={isPhone ? 160 : 200}
            data={rows}
            dataKey="day"
            type="stacked"
            withLegend
            series={[
              { name: 'heating', label: t('stats.heating'), color: 'var(--app-air-warm)' },
              { name: 'cooling', label: t('stats.cooling'), color: 'var(--app-air-cold)' },
            ]}
            gridAxis="x"
            valueFormatter={(v) => reading(v, 'energy')}
            yAxisProps={{ width: 40, tickFormatter: (v: number) => number(v), domain: [energyTicks[0]!, energyTicks[energyTicks.length - 1]!], ticks: energyTicks, interval: 0 }}
            xAxisProps={xAxisProps}
            tooltipProps={{ labelFormatter: dayLabel }}
          />
        </Stack>

        <Stack gap="xs">
          <Title order={3}>{t('month.levels')}</Title>
          <BarChart
            h={isPhone ? 180 : 220}
            data={rows}
            dataKey="day"
            type="stacked"
            withLegend
            series={[0, 1, 2, 3, 4].map((l) => ({
              name: `l${l}`,
              label: l === 0 ? t('common:off') : t('common:Level', { n: l }),
              // Off has no fill of its own in the level greys (it's the hatched block elsewhere).
              color: l === 0 ? 'var(--app-disabled-ink)' : `var(--app-level-${l})`,
            }))}
            gridAxis="x"
            valueFormatter={(v) => hoursText(v)}
            yAxisProps={{ width: 40, domain: [0, 24], ticks: [0, 6, 12, 18, 24], tickFormatter: (v: number) => number(v) }}
            xAxisProps={xAxisProps}
            tooltipProps={{ labelFormatter: dayLabel }}
          />
        </Stack>

        <DayTable days={days} onOpen={openDay} />
        {total.partialDays > 0 && (
          <Text size="xs" c="dimmed">
            {t('month.partialNote')}
          </Text>
        )}
      </Stack>
    );
  }

  return (
    <Card p={{ base: 'md', sm: 'lg' }}>
      <Stack gap="md">
        <Nav
          title={monthYear(month)}
          previous={t('month.previous')}
          next={t('month.next')}
          onPrevious={() => setMonth(addMonths(month, -1))}
          onNext={month >= current ? undefined : () => setMonth(addMonths(month, 1))}
        />
        {body}
      </Stack>
    </Card>
  );
}

function DayTable({ days, onOpen }: { days: DayStats[]; onOpen: (date: string) => void }) {
  const { t } = useTranslation('history');
  return (
    <Stack gap="xs">
      <Title order={3}>{t('month.days')}</Title>
      <Table.ScrollContainer minWidth={720}>
        <Table style={{ whiteSpace: 'nowrap' }}>
          <Table.Thead>
            <Table.Tr>
              <Table.Th>{t('month.dayCol')}</Table.Th>
              <Table.Th>{t('sensor.outdoor')}</Table.Th>
              <Table.Th>{t('sensor.extract')}</Table.Th>
              <Table.Th>{t('stats.efficiency')}</Table.Th>
              <Table.Th>{t('stats.heating')}</Table.Th>
              <Table.Th>{t('stats.degreeDays')}</Table.Th>
              <Table.Th>{t('stats.bypass')}</Table.Th>
              <Table.Th>{t('stats.frost')}</Table.Th>
            </Table.Tr>
          </Table.Thead>
          <Table.Tbody>
            {days.map((d) => (
              <Table.Tr key={d.date}>
                <Table.Td>
                  <Group gap={4} wrap="nowrap">
                    <Anchor component="button" size="sm" c="var(--app-brand-text)" aria-label={t('month.openDay', { date: longDate(d.date) })} onClick={() => onOpen(d.date)}>
                      {weekdayDay(d.date)}
                    </Anchor>
                    {d.partial && (
                      <Tooltip label={t('month.partial', { hours: hoursText(d.recordedH) })} events={{ hover: true, focus: true, touch: true }}>
                        <Text span size="sm" c="dimmed" tabIndex={0} aria-label={t('month.partial', { hours: hoursText(d.recordedH) })}>
                          ◐
                        </Text>
                      </Tooltip>
                    )}
                  </Group>
                </Table.Td>
                <Table.Td>
                  {reading(d.temps.outdoor?.mean, 'temperature')}
                  {d.temps.outdoor && (
                    <Text span size="xs" c="dimmed" display="block">
                      {t('stats.range', { min: reading(d.temps.outdoor.min, 'temperature'), max: reading(d.temps.outdoor.max, 'temperature') })}
                    </Text>
                  )}
                </Table.Td>
                <Table.Td>{reading(d.temps.extract?.mean, 'temperature')}</Table.Td>
                <Table.Td>{reading(d.efficiency === null ? null : d.efficiency * 100, 'percent')}</Table.Td>
                <Table.Td>{reading(d.heatingKwh, 'energy')}</Table.Td>
                <Table.Td>{reading(d.degreeDays, 'degreeDays')}</Table.Td>
                <Table.Td>{hoursText(d.bypassH)}</Table.Td>
                <Table.Td>{hoursText(d.frostH)}</Table.Td>
              </Table.Tr>
            ))}
          </Table.Tbody>
        </Table>
      </Table.ScrollContainer>
    </Stack>
  );
}
