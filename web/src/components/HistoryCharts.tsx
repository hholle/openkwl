import { AreaChart, type ChartReferenceAreaProps, LineChart } from '@mantine/charts';
import { Box, Group, Paper, SimpleGrid, Stack, Text, Title, Tooltip } from '@mantine/core';
import { IconInfoCircle } from '@tabler/icons-react';
import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { number } from '../i18n/format';
import { type Sensor, SENSORS } from '../lib/dayRecord';
import { bands, type ChartPoint, niceTicks } from '../lib/history';
import { reading } from '../lib/readings';

/** Line colours: the schematic's cold and warm air for outdoor and extract air, petrol for the
 *  supply air into the rooms, the muted ink for the exhaust air leaving the house. */
export const SENSOR_COLOR: Record<Sensor, string> = {
  outdoor: 'var(--app-air-cold)',
  supply: 'var(--app-brand)',
  extract: 'var(--app-air-warm)',
  exhaust: 'var(--mantine-color-dimmed)',
};

const BYPASS_COLOR = 'var(--mantine-color-dimmed)';
const FROST_COLOR = 'var(--app-air-frost)';
const Y_WIDTH = 40;
const SYNC = { syncId: 'history', syncMethod: 'value' } as const;

/** The x axis shared by the charts of one view. */
export interface Axis {
  domain: [number, number];
  ticks: number[];
  format: (x: number) => string;
  /** How long one point lasts (in x units), for the bypass and frost bands. */
  step: number;
}

function xAxis(axis: Axis) {
  return {
    xAxisProps: { type: 'number' as const, domain: axis.domain, ticks: axis.ticks, tickFormatter: axis.format, allowDataOverflow: true, interval: 0 as const },
    tooltipProps: { labelFormatter: (x: unknown) => (typeof x === 'number' && Number.isFinite(x) ? axis.format(x) : '') },
  };
}

function Swatch({ color, opacity = 1, line = false }: { color: string; opacity?: number; line?: boolean }) {
  return <Box w={12} h={line ? 3 : 12} bg={color} opacity={opacity} style={{ borderRadius: line ? 2 : 3, flexShrink: 0 }} aria-hidden />;
}

function Legend({ items }: { items: { label: string; color: string; opacity?: number; line?: boolean }[] }) {
  return (
    <Group gap="md" style={{ rowGap: 4 }}>
      {items.map((i) => (
        <Group key={i.label} gap={6} wrap="nowrap">
          <Swatch color={i.color} opacity={i.opacity} line={i.line} />
          <Text size="xs" c="dimmed">
            {i.label}
          </Text>
        </Group>
      ))}
    </Group>
  );
}

/** The four air temperatures, the recovered heat power (optional) and the fan level over one axis. */
export function HistoryCharts({ points, axis, power = true, isPhone }: { points: ChartPoint[]; axis: Axis; power?: boolean; isPhone: boolean }) {
  const { t } = useTranslation(['history', 'common']);
  const x = xAxis(axis);
  const bypass = bands(points, 'bypass', axis.step);
  const frost = bands(points, 'frost', axis.step);
  const areas: ChartReferenceAreaProps[] = [
    ...bypass.map((b) => ({ ...b, color: BYPASS_COLOR, fillOpacity: 0.12, strokeOpacity: 0, ifOverflow: 'hidden' as const })),
    ...frost.map((b) => ({ ...b, color: FROST_COLOR, fillOpacity: 0.25, strokeOpacity: 0, ifOverflow: 'hidden' as const })),
  ];
  const tempTicks = niceTicks(points.flatMap((p) => SENSORS.map((s) => p[s])));
  const powerTicks = niceTicks(points.map((p) => p.power), 3, true);
  const levelWord = (v: number) => (v === 0 ? t('common:off') : t('common:Level', { n: v }));

  return (
    <Stack gap="lg">
      <Stack gap="xs">
        <Title order={4}>{t('chart.temperatures')}</Title>
        <Legend
          items={[
            ...SENSORS.map((s) => ({ label: t(`sensor.${s}`), color: SENSOR_COLOR[s], line: true })),
            ...(bypass.length ? [{ label: t('chart.bypass'), color: BYPASS_COLOR, opacity: 0.25 }] : []),
            ...(frost.length ? [{ label: t('chart.frost'), color: FROST_COLOR, opacity: 0.4 }] : []),
          ]}
        />
        <LineChart
          h={isPhone ? 200 : 260}
          data={points}
          dataKey="x"
          series={SENSORS.map((s) => ({ name: s, label: t(`sensor.${s}`), color: SENSOR_COLOR[s] }))}
          curveType="monotone"
          withDots={false}
          strokeWidth={1.75}
          connectNulls={false}
          gridAxis="x"
          referenceAreas={areas}
          valueFormatter={(v) => reading(v, 'temperature')}
          yAxisProps={{ width: Y_WIDTH, tickFormatter: (v: number) => number(v), domain: [tempTicks[0]!, tempTicks[tempTicks.length - 1]!], ticks: tempTicks, interval: 0 }}
          lineChartProps={SYNC}
          {...x}
        />
      </Stack>

      {power && (
        <Stack gap="xs">
          <Title order={4}>{t('chart.power')}</Title>
          <Text size="xs" c="dimmed">
            {t('chart.powerHint')}
          </Text>
          <AreaChart
            h={isPhone ? 110 : 140}
            data={points}
            dataKey="x"
            series={[{ name: 'power', label: t('chart.power'), color: 'var(--app-air-warm)' }]}
            curveType="monotone"
            withDots={false}
            withGradient={false}
            fillOpacity={0.2}
            strokeWidth={1.5}
            connectNulls={false}
            gridAxis="x"
            valueFormatter={(v) => reading(v, 'power')}
            yAxisProps={{ width: Y_WIDTH, tickFormatter: (v: number) => number(v), domain: [powerTicks[0]!, powerTicks[powerTicks.length - 1]!], ticks: powerTicks, interval: 0 }}
            areaChartProps={SYNC}
            {...x}
          />
        </Stack>
      )}

      <Stack gap="xs">
        <Title order={4}>{t('chart.level')}</Title>
        <AreaChart
          h={isPhone ? 100 : 120}
          data={points}
          dataKey="x"
          series={[{ name: 'level', label: t('chart.level'), color: 'var(--app-level-3)' }]}
          curveType="stepAfter"
          withDots={false}
          withGradient={false}
          fillOpacity={0.3}
          strokeWidth={1.5}
          connectNulls={false}
          gridAxis="x"
          valueFormatter={levelWord}
          yAxisProps={{ width: Y_WIDTH, domain: [0, 4], ticks: [0, 1, 2, 3, 4], interval: 0, tickFormatter: (v: number) => (v === 0 ? t('common:off') : String(v)) }}
          areaChartProps={SYNC}
          {...x}
        />
      </Stack>
    </Stack>
  );
}

export interface Stat {
  label: string;
  value: string;
  /** A second, smaller line under the value. */
  detail?: string;
  hint?: string;
}

/** Figures in tiles: two columns on phones, up to five on wide screens. */
export function StatGrid({ items }: { items: Stat[] }) {
  return (
    <SimpleGrid cols={{ base: 2, sm: 3, md: 5 }} spacing="sm" verticalSpacing="sm">
      {items.map((s) => (
        <Paper key={s.label} withBorder p="sm" radius="md">
          <Group gap={4} wrap="nowrap" align="flex-start">
            <Text size="xs" c="dimmed" lh={1.3}>
              {s.label}
            </Text>
            {s.hint && <Hint label={s.hint} />}
          </Group>
          <Text size="lg" fw={600} className="tabular" mt={4}>
            {s.value}
          </Text>
          {s.detail && (
            <Text size="xs" c="dimmed" className="tabular">
              {s.detail}
            </Text>
          )}
        </Paper>
      ))}
    </SimpleGrid>
  );
}

function Hint({ label }: { label: string }) {
  return (
    <Tooltip label={label} multiline w={260} events={{ hover: true, focus: true, touch: true }}>
      <Box component="span" tabIndex={0} c="var(--mantine-color-placeholder)" lh={0} aria-label={label} style={{ flexShrink: 0 }}>
        <IconInfoCircle size={14} stroke={1.8} />
      </Box>
    </Tooltip>
  );
}

/** An empty view: what is missing and when it will come. */
export function EmptyNote({ title, children }: { title: string; children: ReactNode }) {
  return (
    <Stack gap={4} py="lg" align="center" ta="center">
      <Text fw={600}>{title}</Text>
      <Text size="sm" c="dimmed" maw={480}>
        {children}
      </Text>
    </Stack>
  );
}
