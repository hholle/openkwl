import { Anchor, Button, Chip, Group, Paper, Popover, SegmentedControl, SimpleGrid, Stack, Text, VisuallyHidden } from '@mantine/core';
import { IconAlertTriangle, IconVolume2, IconWind } from '@tabler/icons-react';
import { type ReactNode, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { api, type Schemas, type Status } from '../api/client';
import { clockAfter, hintLine, type Next, offConfirmLine, quietOffLine, stateSentence, subLine } from '../lib/overview';
import { clock } from '../i18n/format';
import { setResource } from '../lib/resources';
import { dayMonth, minutes } from '../lib/summaries';
import { ConfirmModal } from './ConfirmModal';
import { actionError } from './Feedback';
import { TAP } from '../lib/tap';

// KWL FanControl: one choice for how the unit runs (Auto · Off · 1–4, solid red, the only red
// control on Overview) plus the timed Party and Quiet buttons. Choosing a number is manual
// control; Home Assistant still calls it Manual.

type Control = Status['control'];
type Program = 'party' | 'quiet';

/** Durations offered for Party/Quiet; the firmware allows 5–180 min. */
const DURATIONS = [30, 60, 120, 180];

/** How long a choice on the Mode control rests before it is sent. */
const SETTLE_MS = 600;

interface Command {
  /** A control request, or another call that must not overlap one (ending the holiday). */
  req: Schemas['ControlRequest'] | (() => Promise<unknown>);
  shown: Partial<Control>;
  failed: string;
}

export interface FanControlProps {
  status: Status;
  offline: boolean;
  schedule?: Schemas['ScheduleConfig'];
  programs?: Schemas['ProgramConfig'];
  holiday?: Schemas['HolidayConfig'];
  next: Next | null;
  onChanged: () => Promise<void>;
}

export function FanControl({ status, offline, schedule, programs, holiday, next, onChanged }: FanControlProps) {
  const { t } = useTranslation(['overview', 'common']);
  // What the control shows while a choice rests, waits for a confirm or is on its way; the next
  // poll confirms, a failure snaps back.
  const [pending, setPending] = useState<Partial<Control> | null>(null);
  const [ending, setEnding] = useState(false);
  // A choice that stops the ventilation, waiting for the confirm.
  const [confirm, setConfirm] = useState<{ kind: 'off' } | { kind: 'quiet'; min: number } | null>(null);
  // A command that failed; shown in the hint line until the next one. Success needs no message:
  // the control and the headline show the new state.
  const [failure, setFailure] = useState<string | null>(null);
  const control: Control = { ...status.control, ...pending };
  const program = control.mode === 'party' || control.mode === 'quiet';
  const isHoliday = control.mode === 'holiday';
  const holidayEnd = status.holiday?.until ?? holiday?.end;
  const disabled = offline || isHoliday;

  // Arrow keys on the Mode control pass every segment (Auto → Off → 1 → 2), so a choice is only
  // sent once it has rested; commands go out one at a time, the latest target last.
  const real = useRef(status.control);
  useEffect(() => {
    real.current = status.control;
  }, [status.control]);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const queued = useRef<Command | null>(null);
  const busy = useRef(false);
  const confirming = useRef(false);
  useEffect(() => () => clearTimer(), []);

  function clearTimer() {
    if (timer.current) clearTimeout(timer.current);
    timer.current = null;
  }

  /** Drops the shown choice once nothing is resting, waiting or on its way. */
  function settle() {
    if (!busy.current && !queued.current && !timer.current && !confirming.current) setPending(null);
  }

  function enqueue(c: Command) {
    clearTimer();
    queued.current = c;
    setPending(c.shown);
    setFailure(null);
    if (!busy.current) void drain();
  }

  async function drain() {
    busy.current = true;
    while (queued.current) {
      const c = queued.current;
      queued.current = null;
      setFailure(null);
      try {
        await (typeof c.req === 'function' ? c.req() : api.control(c.req));
      } catch (e) {
        setFailure(`${c.failed} ${actionError(e)}`);
      }
    }
    try {
      await onChanged();
    } catch {
      // The next poll catches up.
    }
    busy.current = false;
    if (queued.current) void drain();
    else settle();
  }

  // Keeps the modal's text while it closes.
  const [asked, setAsked] = useState<NonNullable<typeof confirm>>({ kind: 'off' });

  function openConfirm(c: { kind: 'off' } | { kind: 'quiet'; min: number }) {
    confirming.current = true;
    setAsked(c);
    setConfirm(c);
  }

  function closeConfirm() {
    confirming.current = false;
    setConfirm(null);
    settle();
  }

  const offCommand = (): Command => ({ req: { mode: 'manual', level: 0 }, shown: { mode: 'manual', level: 0, remaining_s: null }, failed: t('control.failed.level') });

  /** Sends the choice that has rested; Off asks first unless the unit is already off. */
  function commit(v: string) {
    timer.current = null;
    const r = real.current;
    if (v === 'auto') {
      if (r.mode === 'auto') return settle();
      return enqueue({ req: { mode: 'auto' }, shown: { mode: 'auto', remaining_s: null }, failed: t('control.failed.auto') });
    }
    const level = Number(v);
    if (r.mode === 'manual' && r.level === level) return settle();
    if (level === 0) return openConfirm({ kind: 'off' });
    enqueue({ req: { mode: 'manual', level }, shown: { mode: 'manual', level, remaining_s: null }, failed: t('control.failed.level') });
  }

  function choose(v: string) {
    setPending(v === 'auto' ? { mode: 'auto', remaining_s: null } : { mode: 'manual', level: Number(v), remaining_s: null });
    setFailure(null);
    clearTimer();
    timer.current = setTimeout(() => commit(v), SETTLE_MS);
  }

  function startCommand(kind: Program, min: number): Command {
    const level = kind === 'party' ? programs?.party_level : programs?.quiet_level;
    return {
      req: { mode: kind, duration_min: min },
      shown: { mode: kind, remaining_s: min * 60, ...(level !== undefined ? { level } : {}) },
      failed: t('control.failed.start', { program: t(`common:mode.${kind}`) }),
    };
  }

  function start(kind: Program, min: number) {
    clearTimer();
    // Quiet at level 0 stops the ventilation (unless the fans are off already): ask first.
    const r = real.current;
    if (kind === 'quiet' && programs?.quiet_level === 0 && !(r.mode === 'manual' && r.level === 0)) {
      settle();
      return openConfirm({ kind: 'quiet', min });
    }
    enqueue(startCommand(kind, min));
  }

  /** Ends the program early; the unit goes back to what ran before (Auto or the manual level). */
  function stop() {
    const manual = control.base === 'manual';
    const shown: Partial<Control> = manual ? { mode: 'manual', level: control.manual_level, remaining_s: null } : { mode: 'auto', remaining_s: null };
    enqueue({ req: { end_program: true }, shown, failed: t('control.failed.stop') });
  }

  function endHoliday() {
    setEnding(false);
    if (!holiday) return;
    enqueue({
      req: async () => setResource('holiday', await api.put('holiday', { ...holiday, enabled: false })),
      shown: {},
      failed: t('control.failed.endHoliday'),
    });
  }


  const dot = (n: number, text: string): ReactNode =>
    control.mode === 'auto' && control.level === n ? (
      // The dot hangs below the number; the negative margin keeps the segment's height.
      <span style={{ position: 'relative', display: 'inline-block', paddingBottom: 10, marginBottom: -10 }}>
        {text}
        <span className="kwl-auto-dot" />
        <VisuallyHidden>{t('control.autoRunsThis')}</VisuallyHidden>
      </span>
    ) : (
      text
    );
  const value = control.mode === 'auto' ? 'auto' : isHoliday ? '' : String(control.level);
  const hint = offline ? t('hint.offline') : hintLine(control, schedule, status.time);

  return (
    <Stack gap="md" h="100%">
      <div>
        <Text fz={20} lh="28px" fw={600} component="h2" m={0}>
          {stateSentence(control)}
        </Text>
        <Text c="dimmed" size="md">
          {subLine({ control, time: status.time, schedule, next, holiday, holidayUntil: status.holiday?.until })}
        </Text>
      </div>

      <div>
        <SegmentedControl
          fullWidth
          size="md"
          // Solid brand colour only while it can change the unit.
          color={disabled ? undefined : 'brand'}
          autoContrast
          aria-label={t('control.label')}
          // Room for the Auto dot under the number, and 42px+ tap targets on phones.
          styles={{ label: { paddingTop: 10, paddingBottom: 10 } }}
          disabled={disabled}
          value={value}
          onChange={choose}
          data={[
            { value: 'auto', label: t('common:mode.auto') },
            { value: '0', label: dot(0, t('common:off')) },
            ...[1, 2, 3, 4].map((n) => ({ value: String(n), label: dot(n, String(n)) })),
          ]}
        />
        {failure && !offline ? (
          // In the hint's place, so nothing below moves.
          <Group gap={6} wrap="nowrap" align="flex-start" mt={6} c="var(--app-danger)" role="alert">
            <IconAlertTriangle size={14} stroke={1.8} style={{ flexShrink: 0, marginTop: 2 }} aria-hidden />
            <Text size="xs" c="inherit">
              {failure}
            </Text>
          </Group>
        ) : (
          hint &&
          !isHoliday && (
            <Text size="xs" c="var(--mantine-color-placeholder)" mt={6}>
              {hint}
            </Text>
          )
        )}
      </div>

      <div style={{ marginTop: 'auto' }}>
        {program && !offline ? (
          <Paper bg="var(--app-brand-soft)" c="var(--app-brand-text)" radius="md" py={6} pl="md" pr={6}>
            <Group justify="space-between" wrap="nowrap">
              <Text fw={600} size="md">
                {status.time && control.remaining_s !== null
                  ? t('control.programUntil', { program: t(`common:mode.${control.mode}`), time: clock(clockAfter(status.time, control.remaining_s)) })
                  : t(`common:mode.${control.mode}`)}
              </Text>
              <Button variant="subtle" onClick={stop} size="sm">
                {t('control.stop')}
              </Button>
            </Group>
          </Paper>
        ) : (
          <SimpleGrid cols={2} spacing="sm">
            <ProgramButton kind="party" defaultMin={programs?.party_min} disabled={disabled} onStart={start} />
            <ProgramButton kind="quiet" defaultMin={programs?.quiet_min} disabled={disabled} onStart={start} />
          </SimpleGrid>
        )}
        {isHoliday && (
          <Text size="xs" c="var(--mantine-color-placeholder)" mt={8}>
            {holidayEnd ? t('control.holidayUntil', { date: dayMonth(holidayEnd) }) : t('control.holidayOpen')}{' '}
            <Anchor component="button" size="xs" c="var(--app-brand-text)" onClick={() => setEnding(true)} disabled={offline} style={TAP}>
              {t('control.endHoliday')}
            </Anchor>
          </Text>
        )}
      </div>

      <ConfirmModal opened={ending} title={t('control.endHolidayTitle')} confirmLabel={t('control.endHoliday')} onConfirm={endHoliday} onClose={() => setEnding(false)} />
      <ConfirmModal
        opened={confirm !== null}
        title={asked.kind === 'quiet' ? t('control.confirm.quietTitle') : t('control.confirm.offTitle')}
        confirmLabel={asked.kind === 'quiet' ? t('control.confirm.quiet') : t('control.confirm.off')}
        onConfirm={() => {
          const c = confirm;
          confirming.current = false;
          setConfirm(null);
          if (c) enqueue(c.kind === 'quiet' ? startCommand('quiet', c.min) : offCommand());
        }}
        onClose={closeConfirm}
      >
        {asked.kind === 'quiet' ? quietOffLine(status.control, asked.min) : offConfirmLine(schedule, status.time)}
      </ConfirmModal>
    </Stack>
  );
}

function ProgramButton({
  kind,
  defaultMin,
  disabled,
  onStart,
}: {
  kind: Program;
  defaultMin?: number;
  disabled: boolean;
  onStart: (kind: Program, min: number) => void;
}) {
  const { t } = useTranslation(['overview', 'common']);
  const [opened, setOpened] = useState(false);
  const options = [...new Set([...DURATIONS, ...(defaultMin ? [defaultMin] : [])])].sort((a, b) => a - b);
  const [min, setMin] = useState(String(defaultMin ?? 60));
  useEffect(() => {
    if (opened) setMin(String(defaultMin ?? 60));
  }, [opened, defaultMin]);
  const label = kind === 'party' ? t('control.party') : t('control.quiet');
  return (
    <Popover opened={opened} onChange={setOpened} position="bottom" withArrow trapFocus shadow="md">
      <Popover.Target>
        <Button
          variant="default"
          size="md"
          px="xs"
          fullWidth
          disabled={disabled}
          leftSection={kind === 'party' ? <IconWind size={16} stroke={1.6} /> : <IconVolume2 size={16} stroke={1.6} />}
          onClick={() => setOpened((o) => !o)}
        >
          {label}
        </Button>
      </Popover.Target>
      <Popover.Dropdown>
        <Stack gap="sm">
          <Chip.Group value={min} onChange={(v) => setMin(v as string)}>
            <Group gap={6} role="radiogroup" aria-label={kind === 'party' ? t('control.partyLength') : t('control.quietLength')}>
              {options.map((m) => (
                <Chip key={m} value={String(m)} size="sm">
                  {minutes(m)}
                </Chip>
              ))}
            </Group>
          </Chip.Group>
          <Button
            onClick={() => {
              setOpened(false);
              onStart(kind, Number(min));
            }}
          >
            {t('control.start')}
          </Button>
        </Stack>
      </Popover.Dropdown>
    </Popover>
  );
}
