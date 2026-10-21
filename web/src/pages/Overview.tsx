import { Anchor, Box, Card, Grid, Group, Skeleton, Stack, Text, Title, Tooltip } from '@mantine/core';
import { useMediaQuery } from '@mantine/hooks';
import { Fragment } from 'react';
import { useTranslation } from 'react-i18next';
import type { Schemas, Status } from '../api/client';
import { AirflowSchematic, type AirflowProps } from '../components/AirflowSchematic';
import { FanControl } from '../components/FanControl';
import { FaultAlerts } from '../components/FaultAlert';
import { SafetyNotice } from '../components/SafetyNotice';
import { DayBar, HourAxis } from '../components/ScheduleBar';
import { clock } from '../i18n/format';
import { conditions } from '../lib/faults';
import { minuteOf, nextChange, pausedLine, todayChanges, todaySlots } from '../lib/overview';
import { reading } from '../lib/readings';
import { useConfig } from '../lib/resources';
import { link, navigate } from '../lib/router';
import { dayMonth, levelWord } from '../lib/summaries';
import { TAP } from '../lib/tap';

/** Overview: what the unit is doing now, and whether anything needs doing (KWL screen spec). */
export function Overview({
  status,
  offline,
  onChanged,
}: {
  status: Status | null;
  offline: boolean;
  onChanged: () => Promise<void>;
}) {
  const isPhone = !useMediaQuery('(min-width: 48em)', false, { getInitialValueInEffect: false });
  const schedule = useConfig('schedule');
  const programs = useConfig('programs');
  const holiday = useConfig('holiday');
  const fans = useConfig('fans');
  const lastSeen = status?.time?.slice(11, 16) ?? null;
  const alerts = conditions(status, offline, lastSeen);

  if (!status) {
    return (
      <Stack gap="lg">
        <FaultAlerts items={alerts} />
        {!offline && <Skeleton height={320} radius="lg" />}
      </Stack>
    );
  }

  const next = nextChange(status);

  return (
    <Stack gap="lg">
      <FaultAlerts items={alerts} />
      <SafetyNotice />
      <Grid gap="lg">
        <Grid.Col span={{ base: 12, md: 5 }}>
          <Card h="100%" p={{ base: 'md', sm: 'lg' }}>
            <FanControl
              status={status}
              offline={offline}
              schedule={schedule.data}
              programs={programs.data}
              holiday={holiday.data}
              next={next}
              onChanged={onChanged}
            />
          </Card>
        </Grid.Col>
        <Grid.Col span={{ base: 12, md: 7 }}>
          <AirCard status={status} fans={fans.data} offlineSince={offline ? lastSeen : null} size={isPhone ? 'phone' : 'desktop'} />
        </Grid.Col>
      </Grid>
      <TodayCard status={status} schedule={schedule.data} holiday={holiday.data} />
    </Stack>
  );
}

/** The diagram's input from the status poll. */
export function airflowOf(s: Status): Omit<AirflowProps, 'size' | 'offlineSince'> {
  const t = s.temperatures;
  const off = s.control.level === 0;
  return {
    outdoor: t.outdoor,
    supply: t.supply,
    extract: t.extract,
    exhaust: t.exhaust,
    efficiency: s.efficiency_pct,
    bypass: s.bypass.moving ? 'moving' : s.bypass.open ? 'open' : 'closed',
    frost: s.frost.defrost ? 'defrost' : s.frost.active ? 'boost' : 'off',
    supplyFan: { rpm: off ? 0 : s.supply_fan.rpm, stalled: s.supply_fan.stalled },
    exhaustFan: { rpm: off ? 0 : s.exhaust_fan.rpm, stalled: s.exhaust_fan.stalled },
  };
}

function AirCard({
  status,
  fans,
  offlineSince,
  size,
}: {
  status: Status;
  fans?: Schemas['FanConfig'];
  offlineSince: string | null;
  size: 'desktop' | 'phone';
}) {
  const { t } = useTranslation('overview');
  const flow = status.supply_fan.airflow_m3h;
  const level = status.control.level;
  // Measured per level in the fan settings, else estimated from the fan speed.
  const estimated = !fans || level === 0 || (fans.airflow_m3h[level - 1] ?? 0) === 0;
  const power = status.heat_recovery.power_w;
  // A stalled supply fan has its own red alert; the recovery figure would mislead.
  const footnote = status.supply_fan.stalled
    ? null
    : status.bypass.open
      ? t('air.bypassOpen')
      : power !== null && power > 0
        ? t('air.recovering', { power: reading(power, 'power') })
        : null;
  return (
    <Card h="100%" px="md" py={14}>
      <Stack gap="sm" h="100%">
        <Group justify="space-between" align="baseline" wrap="nowrap">
          <Title order={3}>{t('air.title')}</Title>
          {/* Always shown ("—" while the fan is off or spinning up), so the card never changes size. */}
          <Tooltip label={estimated ? t('air.estimated') : t('air.measured')} disabled={!estimated || flow === null}>
            <Text size="xs" c="var(--mantine-color-placeholder)" className="tabular">
              {t('air.freshAir', { value: `${estimated && flow !== null ? '≈ ' : ''}${reading(flow, 'airflow')}` })}
            </Text>
          </Tooltip>
        </Group>
        <Box style={{ display: 'flex', justifyContent: 'center', flex: 1, alignItems: 'center' }}>
          <AirflowSchematic {...airflowOf(status)} offlineSince={offlineSince} size={size} />
        </Box>
        {/* The line keeps its place while there is nothing to say, so the layout doesn't jump. */}
        <Text size="xs" c="var(--mantine-color-placeholder)" style={{ visibility: footnote ? 'visible' : 'hidden' }} aria-hidden={!footnote}>
          {footnote ?? '\u00a0'}
        </Text>
      </Stack>
    </Card>
  );
}

function TodayCard({
  status,
  schedule,
  holiday,
}: {
  status: Status;
  schedule?: Schemas['ScheduleConfig'];
  holiday?: Schemas['HolidayConfig'];
}) {
  const { t } = useTranslation('overview');
  const c = status.control;
  const clockMissing = status.faults.includes('time_not_set') || !status.time;
  const time = status.time;
  const day = todaySlots(status);
  const changes = todayChanges(day, time);
  const paused = pausedLine(c, schedule, time);
  let body;
  const holidayEnd = status.holiday?.until ?? holiday?.end;
  if (c.mode === 'holiday') {
    body = (
      <Text size="sm">
        {holidayEnd ? t('today.holidayUntil', { date: dayMonth(holidayEnd) }) : t('today.holidayOpen')}{' '}
        <Anchor {...link('/schedule/holiday')} size="sm" c="var(--app-brand-text)" style={TAP}>
          {t('today.editHoliday')}
        </Anchor>
      </Text>
    );
  } else if (clockMissing) {
    body = (
      <Text size="sm">
        {t('today.clockMissing')}{' '}
        <Anchor {...link('/settings/clock')} size="sm" c="var(--app-brand-text)" style={TAP}>
          {t('today.setClock')}
        </Anchor>
      </Text>
    );
  } else if (!day || !time) {
    body = <Skeleton height={32} />;
  } else {
    body = (
      <>
        <DayBar day={day} label={t('today.title')} nowMin={minuteOf(time)} height={32} dim={paused !== null} onSelect={() => navigate('/schedule/week')} />
        <HourAxis />
        {paused && (
          <Text size="xs" c="dimmed" mt={8}>
            {paused}
          </Text>
        )}
        {changes.length > 0 && (
          <Text size="xs" c="dimmed" mt={8} className="tabular">
            {changes.map((ch, i) => (
              <Fragment key={ch.at}>
                <Text span inherit c={ch.past ? 'var(--mantine-color-placeholder)' : undefined}>
                  {clock(ch.at)} {levelWord(ch.level)}
                </Text>
                {i < changes.length - 1 && ' · '}
              </Fragment>
            ))}
          </Text>
        )}
      </>
    );
  }
  return (
    <Card px="md" py={14}>
      <Group justify="space-between" align="baseline" mb="sm">
        <Title order={3}>{t('today.title')}</Title>
        <Anchor {...link('/schedule/week')} size="sm" c="var(--app-brand-text)" style={TAP}>
          {t('today.editSchedule')}
        </Anchor>
      </Group>
      {body}
    </Card>
  );
}
