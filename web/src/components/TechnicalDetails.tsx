import { Alert, Badge, Button, Card, Collapse, Group, SimpleGrid, Skeleton, Stack, Table, Text, Title } from '@mantine/core';
import { IconCheck, IconCopy } from '@tabler/icons-react';
import i18next, { type TFunction } from 'i18next';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { api, type Schemas } from '../api/client';
import type { resources } from '../i18n';
import { dateTime, number, timeOfDay } from '../i18n/format';
import { bytes, duration, hex, unixTime } from '../lib/format';
import { usePoll } from '../lib/hooks';
import { ActionError, useAction } from './Feedback';

// KWL TechnicalDetails: everything GET /system/details reports, as small key/value tables
// (collapsed until asked for), refreshed every 5 s while open, and copyable as plain text.

type Details = Schemas['SystemDetails'];
type DetailsT = TFunction<'details'>;
type RowId = keyof (typeof resources)['en']['details']['rows'];
type GroupId = keyof (typeof resources)['en']['details']['groups'];
/** A row: its stable id (the label's key), its value and whether it points at a problem. */
type Row = [id: RowId, value: string | number, problem?: boolean];

/** Identifiers are set in monospace so they can be checked character by character. */
const IDENTIFIERS = new Set<RowId>(['version', 'build', 'uniqueId', 'address', 'gateway', 'dns', 'timeServer', 'ntpDhcp', 'mac', 'broker', 'deviceId', 'bootloader', 'fwBasis']);

/** How values are written: in the page's language, or in English for the copied report. */
interface Fmt {
  t: DetailsT;
  num: (n: number, decimals?: number) => string;
  time: (unixS: number) => string;
}
const screenFmt = (t: DetailsT): Fmt => ({ t, num: number, time: dateTime });
// The copied report stays in English: it goes into bug reports read by the maintainers.
const reportFmt = (): Fmt => ({ t: i18next.getFixedT('en', 'details'), num: (n, d = 0) => n.toFixed(d), time: unixTime });

const NTC_ROWS = ['ntcOutdoor', 'ntcSupplyAir', 'ntcExtract', 'ntcExhaust'] as const satisfies readonly RowId[];

const orDash = (v: string | number | null | undefined) => (v === null || v === undefined || v === '' ? '—' : v);

/** Everything `GET /system/details` reports, grouped like the firmware sees it. Values that
 *  point at a problem get a yellow badge beside them, never colour alone. */
function groups(d: Details, { t: tr, num, time }: Fmt): [group: GroupId, rows: Row[]][] {
  const { firmware: f, chip: c, memory: m, network: n, time: t, mqtt: q, storage: s, stock: k, counters: x, board: b, modules: mo } = d;
  const known = (v: boolean | null, yes: string, no: string) => (v === null ? '—' : v ? yes : no);
  const ofTotal = (part: number, whole: number, share = true) =>
    share && whole > 0
      ? tr('value.ofTotalShare', { part: bytes(part), whole: bytes(whole), pct: num(Math.round((part / whole) * 100)) })
      : tr('value.ofTotal', { part: bytes(part), whole: bytes(whole) });
  const mhz = (hz: number) => num(hz / 1e6);
  const volts = (mv: number) => `${num(mv / 1000, 2)}\u00a0V`;
  // The sensor's resistance as the firmware computes it (10 kΩ pull-up, 2.5 V ADC reference).
  const ntc = (i: number) => {
    const raw = b.ntc_raw?.[i];
    if (raw === undefined || b.ntc_supply_mv === null) return '—';
    const v = (raw * 2.5) / 4096;
    const headroom = b.ntc_supply_mv / 1000 - v;
    return headroom > 0 ? tr('value.ntcRaw', { raw, ohms: num(Math.floor((10_000 * v) / headroom)) }) : String(raw);
  };
  return [
    [
      'firmware',
      [
        ['version', `openKWL ${f.version}`],
        ['build', f.build],
        ['built', f.built_unix_s ? time(f.built_unix_s) : '—'],
        ['platform', `${f.platform} (${f.target})`],
        ['compiler', f.rustc ? `rustc ${f.rustc}, ${f.profile}` : f.profile],
        ['uptime', duration(f.uptime_s)],
      ],
    ],
    [
      'controller',
      [
        ['chip', c.mcu],
        ['deviceId', c.device_id ? `${hex(c.device_id)} / ${hex(c.revision_id, 4)}` : '—'],
        ['uniqueId', orDash(c.unique_id)],
        ['clock', c.hse_hz ? tr('value.clockCrystal', { mhz: mhz(c.sysclk_hz), crystal: mhz(c.hse_hz) }) : tr('value.clockInternal', { mhz: mhz(c.sysclk_hz) })],
        [
          'lastReset',
          c.reset_flags ? tr('value.resetFlags', { reason: tr(`resetReason.${c.reset_reason}`), flags: hex(c.reset_flags, 2) }) : tr(`resetReason.${c.reset_reason}`),
        ],
      ],
    ],
    [
      'memory',
      [
        ['image', m.image_bytes ? ofTotal(m.image_bytes, m.flash_slot_bytes) : '—'],
        ['flash', c.flash_kb ? tr('value.kb', { n: num(c.flash_kb) }) : '—'],
        ['ramStatic', m.static_ram_bytes ? ofTotal(m.static_ram_bytes, m.ram_bytes) : '—'],
        ['stackPeak', m.stack_peak_bytes === null ? '—' : ofTotal(m.stack_peak_bytes, m.stack_bytes, false)],
      ],
    ],
    [
      'network',
      [
        ['link', n.link_up ? tr('value.linkUp') : tr('value.linkDown'), !n.link_up],
        ['address', n.ip ? `${n.ip}${n.prefix_len !== null ? `/${n.prefix_len}` : ''} (${n.dhcp ? tr('value.dhcp') : tr('value.static')})` : '—'],
        ['gateway', orDash(n.gateway)],
        ['dns', n.dns.length ? n.dns.join(', ') : '—'],
        ['ntpDhcp', orDash(n.ntp_server)],
        ['hostname', `${n.hostname}${n.mdns ? ` (${n.hostname}.local)` : ''}`],
        ['mac', n.mac],
        ['phy', orDash(n.phy)],
      ],
    ],
    [
      'clock',
      [
        ['timeFrom', tr(`timeSource.${t.source}`)],
        ['timeServer', orDash(t.server)],
        ['lastSync', t.last_sync ? t.last_sync.replace('T', ' ') : tr('value.notYet')],
        ['lastOffset', t.last_offset_ms === null ? '—' : tr('value.seconds', { n: num(t.last_offset_ms / 1000, 1) })],
        ['syncs', t.syncs],
        ['rtcClock', t.rtc_clock === 'LSE' ? tr('value.lse') : orDash(t.rtc_clock)],
      ],
    ],
    [
      'mqtt',
      [
        ['mqttState', !q.enabled ? tr('value.mqttOff') : q.connected ? tr('value.mqttConnected') : tr('value.mqttNotConnected')],
        ['broker', orDash(q.broker)],
        ['connects', q.connects],
        ['messages', `${q.published} / ${q.received}`],
      ],
    ],
    [
      'sd',
      [
        ['card', s.present ? (s.capacity_mb ? tr('value.gb', { n: num(s.capacity_mb / 1024, 1) }) : tr('value.cardPresent')) : tr('value.cardMissing'), !s.present],
        ['settingsFile', s.config_valid ? tr('value.configOk') : tr('value.configDamaged'), !s.config_valid],
        ['writes', s.writes],
        ['stateSaved', s.last_state_save ? s.last_state_save.replace('T', ' ') : tr('value.notYet')],
      ],
    ],
    [
      'stock',
      [
        ['bootloader', orDash(k.bootloader)],
        ['fwBasis', orDash(k.fw_basis)],
      ],
    ],
    [
      'modules',
      [
        ['moduleSocket', tr(`busPower.${mo.bus_power}`), mo.bus_power !== 'on'],
        ['knx', tr(`knx.${mo.knx}`)],
        ['knxFirmware', orDash(mo.knx_firmware)],
        ['knxFrames', `${mo.knx_frames} / ${mo.knx_errors}`],
        ['knxTimeouts', mo.knx_timeouts],
        ['knxRx', `${mo.knx_rx_bytes} / ${mo.knx_line_errors}`, mo.knx_line_errors > 0],
        ['busSense', `${num(mo.bus_sense_mv)}\u00a0mV`],
      ],
    ],
    [
      'board',
      [
        ['cardDetect', known(b.card_detect_present, tr('value.cardDetectYes'), tr('value.cardDetectNo'))],
        ['sdPower', known(b.sd_power_active_low, tr('value.sdPowerLow'), tr('value.sdPowerHigh'))],
        ['mainsPulses', b.mains_pulses_per_s === null ? '—' : tr('value.perSecond', { n: b.mains_pulses_per_s })],
        ['phyAddress', b.phy_address === null ? tr('value.noneFound') : String(b.phy_address)],
        ['ntcSupply', b.ntc_supply_mv === null ? '—' : volts(b.ntc_supply_mv)],
        ...NTC_ROWS.map((id, i): Row => [id, ntc(i)]),
      ],
    ],
    [
      'counters',
      [
        ['requests', `${x.http_requests} / ${x.http_errors}`],
        ['failedLogins', x.failed_logins, x.failed_logins > 0],
        ['sessions', x.sessions],
      ],
    ],
  ];
}

/** The plain-text report for "Copy as text", always in English. */
function asText(d: Details): string {
  const fmt = reportFmt();
  return groups(d, fmt)
    .map(([group, rows]) => [fmt.t(`groups.${group}`), ...rows.map(([id, v]) => `  ${fmt.t(`rows.${id}`)}: ${v}`)].join('\n'))
    .join('\n\n');
}

const STORAGE_KEY = 'openkwl.details';
function readOpen(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) === 'open';
  } catch {
    return false;
  }
}
function writeOpen(open: boolean) {
  try {
    localStorage.setItem(STORAGE_KEY, open ? 'open' : 'closed');
  } catch {
    // Private window: the grid just starts closed next time.
  }
}

export function TechnicalDetails() {
  const { t } = useTranslation('details');
  const [open, setOpen] = useState(readOpen);
  const toggle = () => {
    writeOpen(!open);
    setOpen(!open);
  };
  return (
    <Card p={{ base: 'md', sm: 'lg' }} component="section" aria-labelledby="details-title">
      <Group justify="space-between" align="flex-start" gap="sm">
        <div style={{ flex: '1 1 220px' }}>
          <Title order={2} id="details-title">
            {t('title')}
          </Title>
          <Text size="sm" c="dimmed">
            {t('intro')}
          </Text>
        </div>
        <Button variant="subtle" onClick={toggle} aria-expanded={open} aria-controls="details-body">
          {open ? t('hide') : t('show')}
        </Button>
      </Group>
      <Collapse expanded={open}>{open && <DetailsBody />}</Collapse>
    </Card>
  );
}

function DetailsBody() {
  const { t } = useTranslation('details');
  const details = usePoll(api.systemDetails, 5000);
  const copying = useAction();
  if (!details.data) {
    return details.error ? (
      <Alert color="red" mt="md">
        {details.error.describe()}
      </Alert>
    ) : (
      <Skeleton height={240} mt="md" />
    );
  }
  const d = details.data;
  const updated = details.updatedAt ? timeOfDay(details.updatedAt) : null;
  const copy = () => copying.run(() => navigator.clipboard.writeText(asText(d)), t('copyFailed'));
  return (
    <Stack id="details-body" gap="md" mt="md">
      <Group justify="space-between">
        {updated && (
          <Text size="xs" c="var(--mantine-color-placeholder)">
            {t('updated', { time: updated })}
          </Text>
        )}
        {/* The icon turns into a tick for a moment; the label stays, so the button keeps its size. */}
        <Button
          variant="default"
          leftSection={copying.done ? <IconCheck size={16} stroke={2} color="var(--app-success)" /> : <IconCopy size={16} stroke={1.6} />}
          onClick={copy}
          aria-label={copying.done ? t('copied') : undefined}
        >
          {t('copy')}
        </Button>
      </Group>
      <ActionError action={copying} />
      <SimpleGrid cols={{ base: 1, sm: 2, lg: 3 }} spacing="lg">
        {groups(d, screenFmt(t)).map(([group, rows]) => (
          <Table key={group} captionSide="top" highlightOnHover={false} verticalSpacing={4} horizontalSpacing={0}>
            <Table.Caption ta="left" fw={600} fz="sm" c="var(--mantine-color-text)" mb={4}>
              {t(`groups.${group}`)}
            </Table.Caption>
            <Table.Tbody>
              {rows.map(([id, value, problem]) => (
                <Table.Tr key={id}>
                  <Table.Td fz="xs" c="dimmed" pr="sm" style={{ verticalAlign: 'top', width: '45%' }}>
                    {t(`rows.${id}`)}
                  </Table.Td>
                  <Table.Td fz="sm" ff={IDENTIFIERS.has(id) ? 'monospace' : undefined} style={{ overflowWrap: 'anywhere' }}>
                    {value}
                    {problem && (
                      <Badge color="yellow" ml={6} size="sm">
                        {t('check')}
                      </Badge>
                    )}
                  </Table.Td>
                </Table.Tr>
              ))}
            </Table.Tbody>
          </Table>
        ))}
      </SimpleGrid>
    </Stack>
  );
}
