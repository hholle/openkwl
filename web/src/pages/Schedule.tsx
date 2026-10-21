import {
  ActionIcon,
  Alert,
  Button,
  Card,
  Checkbox,
  Group,
  Input,
  Menu,
  NumberInput,
  SegmentedControl,
  Select,
  Skeleton,
  Stack,
  Switch,
  Text,
  Title,
  Tooltip,
} from '@mantine/core';
import { DatePickerInput, TimeInput } from '@mantine/dates';
import { useMediaQuery } from '@mantine/hooks';
import { IconChevronDown, IconCopy, IconDots, IconMinus, IconPlus, IconTrash } from '@tabler/icons-react';
import type { TFunction } from 'i18next';
import { type ReactNode, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { api, type Schemas, type Status } from '../api/client';
import { ConfirmModal } from '../components/ConfirmModal';
import { ActionError, useAction, useFlash } from '../components/Feedback';
import { MonthDayInput } from '../components/MonthDayInput';
import { DayBar, HourAxis, LevelChips } from '../components/ScheduleBar';
import { SectionList } from '../components/SectionList';
import { type FormState, SaveBar, SettingsSection, saveError, useConfigForm, useLeaveGuard } from '../components/SettingsSection';
import { clock, dayMonth, number, weekdayName } from '../i18n/format';
import { profileName } from '../lib/format';

import { minuteOf, weekday } from '../lib/overview';
import { setResource, useConfig, useProfile } from '../lib/resources';
import {
  addBlock,
  applyGrouping,
  blocks,
  type Day,
  DAY_MIN,
  detectGrouping,
  dayGroups,
  fromMin,
  type Grouping,
  groupingLabel,
  groupingQuestion,
  GROUPINGS,
  MAX_SLOTS,
  moveBoundary,
  normalizeWeek,
  removeBlock,
  sameDay,
  setGroup,
  setLevel,
  setStart,
  STEP_MIN,
} from '../lib/schedule';
import { backToAutoSummary, coolingSummary, holidaySummary, levelWord, programsSummary } from '../lib/summaries';
import { CHECKBOX_TAP, SWITCH_TAP } from '../lib/tap';

const num = (v: string | number, fallback = 0) => (typeof v === 'number' ? v : Number.parseFloat(v) || fallback);
const unit = (u: string) => (
  <Text size="sm" c="dimmed" pr={4}>
    {u}
  </Text>
);

/** Schedule: what the unit does by itself in Auto mode (KWL screen spec "Schedule"). */
export function SchedulePage({ section, status }: { section: string | null; status: Status | null }) {
  const { t } = useTranslation('schedule');
  const schedule = useConfig('schedule');
  const holiday = useConfig('holiday');
  const programs = useConfig('programs');
  const bypass = useConfig('bypass');
  const active = schedule.data?.active_profile ?? null;
  const profile = useProfile(active);
  const today = status?.time ? weekday(status.time) : 0;
  const changes = profile.data?.days ? normalizeWeek(profile.data.days)[today]!.length : undefined;

  return (
    <SectionList
      page="schedule"
      title={t('title')}
      intro={t('intro')}
      selected={section}
      groups={[
        {
          rows: [
            {
              id: 'week',
              title: t('rows.week'),
              summary: active !== null && changes !== undefined ? t('rows.weekSummary', { profile: profileName(active), count: changes }) : undefined,
              badge: status?.faults.includes('time_not_set') ? { label: t('rows.clockNotSet'), color: 'yellow' } : undefined,
            },
            {
              id: 'holiday',
              title: t('rows.holiday'),
              summary: holidaySummary(holiday.data),
              badge: status?.holiday ? { label: t('on'), color: 'blue' } : undefined,
            },
            { id: 'party-quiet', title: t('rows.partyQuiet'), summary: programsSummary(programs.data) },
            { id: 'summer-cooling', title: t('rows.cooling'), summary: coolingSummary(bypass.data) },
            { id: 'back-to-auto', title: t('rows.backToAuto'), summary: backToAutoSummary(schedule.data) },
          ],
        },
      ]}
      render={(id) =>
        id === 'week' ? (
          <WeekSection status={status} />
        ) : id === 'holiday' ? (
          <HolidaySection />
        ) : id === 'party-quiet' ? (
          <ProgramsSection />
        ) : id === 'summer-cooling' ? (
          <CoolingSection />
        ) : (
          <BackToAutoSection />
        )
      }
    />
  );
}

// ---- Week ---------------------------------------------------------------------------

function WeekSection({ status }: { status: Status | null }) {
  const { t } = useTranslation(['schedule', 'common']);
  const settings = useConfig('schedule');
  const [shown, setShown] = useState<number | null>(null);
  // The editor's unsaved changes: showing another profile remounts it, so ask first.
  const [dirty, setDirty] = useState(false);
  const [switchTo, setSwitchTo] = useState<{ shown: number | null } | null>(null);
  const active = settings.data?.active_profile ?? null;
  const n = shown ?? active;
  const profile = useProfile(n);
  const wide = useMediaQuery('(min-width: 62em)', true, { getInitialValueInEffect: false });
  const switching = useAction();

  if (settings.error && !settings.data) return <Alert color="red">{saveError(settings.error)}</Alert>;
  if (active === null || n === null) return <Skeleton height={300} radius="lg" />;

  // Switching the profile in use: the menu then names it, which is the confirmation.
  const use = () =>
    switching.run(async () => {
      setResource('schedule', await api.put('schedule', { ...settings.data!, active_profile: n! }));
      setShown(null);
    }, t('week.switchFailed'));

  const show = (p: number) => {
    if (p === n) return;
    const next = p === active ? null : p;
    if (dirty) setSwitchTo({ shown: next });
    else setShown(next);
  };

  const menu = (
    <Menu position="bottom-end" shadow="md">
      <Menu.Target>
        <Button variant="default" rightSection={<IconChevronDown size={16} stroke={1.6} />}>
          {t('week.using', { profile: profileName(active) })}
        </Button>
      </Menu.Target>
      <Menu.Dropdown>
        {[1, 2, 3, 4, 5].map((p) => (
          <Menu.Item key={p} onClick={() => show(p)} fw={p === n ? 600 : undefined}>
            {p === active ? t('week.inUse', { profile: profileName(p) }) : profileName(p)}
          </Menu.Item>
        ))}
      </Menu.Dropdown>
    </Menu>
  );

  return (
    <Stack gap="lg">
      <Group justify={wide ? 'space-between' : 'flex-end'} align="center">
        {wide && <Title order={2}>{t('rows.week')}</Title>}
        {menu}
      </Group>
      {n !== active && (
        <Alert color="gray" variant="light">
          <Group justify="space-between" gap="sm">
            <Text size="sm">{t('week.viewing', { profile: profileName(n) })}</Text>
            <Button size="xs" variant="light" onClick={use} loading={switching.busy}>
              {t('week.useThis')}
            </Button>
          </Group>
        </Alert>
      )}
      <ActionError action={switching} />
      {profile.data ? (
        <WeekEditor
          key={n}
          name={profileName(n)}
          saved={normalizeWeek(profile.data.days)}
          time={n === active ? (status?.time ?? null) : null}
          onSave={async (days) => setResource(`profile:${n}`, await api.putProfile(n, { days }))}
          onDirtyChange={setDirty}
        />
      ) : profile.error ? (
        <Alert color="red">{saveError(profile.error)}</Alert>
      ) : (
        <Skeleton height={260} radius="lg" />
      )}
      <ConfirmModal
        opened={switchTo !== null}
        title={t('common:discardTitle', { section: t('rows.week') })}
        confirmLabel={t('common:discardConfirm')}
        onConfirm={() => {
          if (switchTo) setShown(switchTo.shown);
          setSwitchTo(null);
        }}
        onClose={() => setSwitchTo(null)}
      />
    </Stack>
  );
}

function WeekEditor({
  name,
  saved,
  time,
  onSave,
  onDirtyChange,
}: {
  name: string;
  saved: Day[];
  time: string | null;
  onSave: (days: Day[]) => Promise<void>;
  onDirtyChange: (dirty: boolean) => void;
}) {
  const { t } = useTranslation('schedule');
  const [days, setDays] = useState(saved);
  const [grouping, setGrouping] = useState<Grouping>(() => detectGrouping(saved));
  const [asking, setAsking] = useState<Grouping | null>(null);
  const question = asking ? groupingQuestion(days, asking) : null;
  const [selected, setSelected] = useState(0);
  const [clipboard, setClipboard] = useState<Day | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [justSaved, flash] = useFlash();
  const fine = useMediaQuery('(pointer: fine)', true, { getInitialValueInEffect: false });
  const isPhone = !useMediaQuery('(min-width: 48em)', false, { getInitialValueInEffect: false });
  const dirty = days.some((d, i) => !sameDay(d, saved[i]!));
  const groups = dayGroups(grouping);
  const today = time ? weekday(time) : null;
  const guard = useLeaveGuard(dirty, t('rows.week'));
  useEffect(() => {
    onDirtyChange(dirty);
    return () => onDirtyChange(false);
  }, [dirty, onDirtyChange]);

  function chooseGrouping(g: Grouping) {
    if (groupingQuestion(days, g)) setAsking(g);
    else {
      setGrouping(g);
      setSelected(0);
    }
  }

  const form: FormState = {
    dirty,
    saving,
    error,
    saved: justSaved,
    save: async () => {
      setSaving(true);
      setError(null);
      try {
        await onSave(days);
        flash();
      } catch (e) {
        setError(saveError(e));
      } finally {
        setSaving(false);
      }
    },
    discard: () => {
      setDays(saved);
      setGrouping(detectGrouping(saved));
      setError(null);
    },
  };

  const group = groups[selected] ?? groups[0]!;
  const first = group.days[0]!;
  const update = (day: Day) => setDays(setGroup(days, group.days, day));
  const labelW = isPhone ? 56 : 88;

  return (
    <>
      <Card p={{ base: 'md', sm: 'lg' }}>
        <Stack gap="md">
          <Stack gap={8}>
            {groups.map((g, gi) => (
              <Group key={g.label} gap="sm" wrap="nowrap">
                <Text size="sm" fw={gi === selected ? 600 : 500} w={labelW} style={{ flexShrink: 0 }}>
                  {isPhone ? g.short : g.label}
                </Text>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <DayBar
                    day={days[g.days[0]!]!}
                    label={g.label}
                    nowMin={time && today !== null && g.days.includes(today) ? minuteOf(time) : null}
                    selected={gi === selected && groups.length > 1}
                    onSelect={() => setSelected(gi)}
                    onChange={fine && !isPhone ? (d) => setDays(setGroup(days, g.days, d)) : undefined}
                  />
                </div>
              </Group>
            ))}
            <HourAxis offset={labelW + 12} />
          </Stack>
          <Text size="xs" c="var(--mantine-color-placeholder)">
            {fine && !isPhone ? t('week.dragHint') : t('week.editHint')}
          </Text>
          {isPhone ? (
            <Select
              label={t('week.days')}
              size="md"
              data={GROUPINGS.map((g) => ({ value: g, label: groupingLabel(g) }))}
              value={grouping}
              onChange={(v) => v && chooseGrouping(v as Grouping)}
              allowDeselect={false}
            />
          ) : (
            <SegmentedControl
              aria-label={t('week.days')}
              value={grouping}
              onChange={(v) => chooseGrouping(v as Grouping)}
              data={GROUPINGS.map((g) => ({ value: g, label: groupingLabel(g) }))}
            />
          )}
        </Stack>
      </Card>

      <Card p={{ base: 'md', sm: 'lg' }} style={{ overflow: 'visible' }} component="section" aria-label={t('week.blocksOf', { group: group.label })}>
        <Group justify="space-between" mb="sm" wrap="nowrap">
          <Title order={3}>{group.label}</Title>
          <Menu position="bottom-end" shadow="md">
            <Menu.Target>
              <Tooltip label={t('week.more', { group: group.label })}>
                <ActionIcon variant="subtle" color="gray" size={42} aria-label={t('week.more', { group: group.label })}>
                  <IconDots size={18} stroke={1.6} />
                </ActionIcon>
              </Tooltip>
            </Menu.Target>
            <Menu.Dropdown>
              <Menu.Item leftSection={<IconCopy size={16} stroke={1.6} />} onClick={() => setClipboard(days[first]!)}>
                {t('week.copy')}
              </Menu.Item>
              <Menu.Item disabled={!clipboard} onClick={() => clipboard && update(clipboard)}>
                {t('week.paste')}
              </Menu.Item>
              <Menu.Item onClick={() => update(saved[first]!)}>{t('week.reset', { profile: name })}</Menu.Item>
            </Menu.Dropdown>
          </Menu>
        </Group>
        <BlockRows day={days[first]!} onChange={update} isPhone={isPhone} />
        <SaveBar form={form} saveLabel={t('week.save')} discardLabel={t('week.undo')} />
      </Card>

      <ConfirmModal
        opened={asking !== null}
        title={question?.title ?? ''}
        confirmLabel={t('week.useMonday', { monday: weekdayName(0, 'long') })}
        onConfirm={() => {
          if (!asking) return;
          setDays(applyGrouping(days, asking));
          setGrouping(asking);
          setSelected(0);
          setAsking(null);
        }}
        onClose={() => setAsking(null)}
      >
        <Text>{question?.body}</Text>
      </ConfirmModal>
      {guard}
    </>
  );
}

/** The selected group's blocks as rows: the way to edit on phones and without dragging. */
function BlockRows({ day, onChange, isPhone }: { day: Day; onChange: (d: Day) => void; isPhone: boolean }) {
  const { t } = useTranslation('schedule');
  const bs = blocks(day);
  const full = day.length >= MAX_SLOTS;
  return (
    <Stack gap="sm">
      {bs.map((b, i) => {
        const time = (
          <Group gap={4} wrap="nowrap">
            {i === 0 ? (
              <Text className="tabular" w={isPhone ? 'auto' : 168}>
                {t('week.from0')}
              </Text>
            ) : (
              <>
                <ActionIcon variant="default" size={isPhone ? 42 : 'lg'} aria-label={t('week.earlier')} onClick={() => onChange(moveBoundary(day, i, b.start - STEP_MIN))}>
                  <IconMinus size={16} stroke={1.6} />
                </ActionIcon>
                <StartInput
                  label={t('week.fromBlock', { n: i + 1 })}
                  isPhone={isPhone}
                  day={day}
                  index={i}
                  onChange={onChange}
                />
                <ActionIcon variant="default" size={isPhone ? 42 : 'lg'} aria-label={t('week.later')} onClick={() => onChange(moveBoundary(day, i, b.start + STEP_MIN))}>
                  <IconPlus size={16} stroke={1.6} />
                </ActionIcon>
              </>
            )}
            <Text size="sm" c="dimmed" className="tabular" ml={4} style={{ whiteSpace: 'nowrap' }}>
              {t('week.to', { time: fromMin(b.end % DAY_MIN) })}
            </Text>
          </Group>
        );
        const remove = (
          <Tooltip label={t('week.delete')}>
            <ActionIcon
              variant="subtle"
              color="gray"
              size={isPhone ? 42 : 'lg'}
              aria-label={t('week.delete')}
              disabled={day.length === 1}
              onClick={() => onChange(removeBlock(day, i))}
            >
              <IconTrash size={18} stroke={1.6} />
            </ActionIcon>
          </Tooltip>
        );
        const chips = <LevelChips value={b.level} onChange={(l) => onChange(setLevel(day, i, l))} label={t('week.levelFrom', { time: clock(fromMin(b.start)) })} />;
        return isPhone ? (
          <Stack key={i} gap={6} pb="sm" style={{ borderBottom: '1px solid var(--mantine-color-default-border)' }}>
            <Group justify="space-between" wrap="nowrap">
              {time}
              {remove}
            </Group>
            {chips}
          </Stack>
        ) : (
          <Group key={i} gap="md" wrap="nowrap">
            {time}
            <div style={{ flex: 1 }}>{chips}</div>
            {remove}
          </Group>
        );
      })}
      <Group justify="space-between" mt={4}>
        <Text size="xs" c="var(--mantine-color-placeholder)">
          {t('week.count', { n: day.length, max: MAX_SLOTS })}
        </Text>
        <Button
          variant="default"
          leftSection={<IconPlus size={16} stroke={1.6} />}
          disabled={full}
          onClick={() => {
            const next = addBlock(day);
            if (next) onChange(next);
          }}
        >
          {t('week.add')}
        </Button>
      </Group>
    </Stack>
  );
}

/**
 * A block's start time. Rows are keyed by index, so the field keeps focus while typing. A time
 * outside the neighbours' range stays as typed until the field is left (then it is clamped):
 * clamping half-typed values like "01:00" on the way to "18:00" would break the typing.
 */
function StartInput({
  label,
  isPhone,
  day,
  index,
  onChange,
}: {
  label: string;
  isPhone: boolean;
  day: Day;
  index: number;
  onChange: (d: Day) => void;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  return (
    <TimeInput
      aria-label={label}
      size={isPhone ? 'md' : 'sm'}
      w={96}
      step={STEP_MIN * 60}
      value={draft ?? day[index]!.from}
      onChange={(e) => {
        const v = e.currentTarget.value;
        const next = setStart(day, index, v);
        if (next[index]!.from === v) {
          onChange(next);
          setDraft(null);
        } else setDraft(v);
      }}
      onBlur={() => {
        if (draft === null) return;
        onChange(setStart(day, index, draft));
        setDraft(null);
      }}
    />
  );
}

// ---- Settings sections ------------------------------------------------------------

function Labelled({ label, description, children }: { label: string; description?: string; children: ReactNode }) {
  return (
    <Input.Wrapper label={label} description={description}>
      <div style={{ marginTop: 4 }}>{children}</div>
    </Input.Wrapper>
  );
}

/** "from 12 to 24 Oct" / "from 28 Oct to 3 Nov" (components only: reads the language). */
function fromTo(t: TFunction<'schedule'>, start: string, end: string) {
  if (start.slice(0, 7) === end.slice(0, 7)) return t('holiday.fromToSameMonth', { from: number(Number(start.slice(8, 10))), to: dayMonth(end) });
  return t('holiday.fromTo', { from: dayMonth(start), to: dayMonth(end) });
}

function HolidaySection() {
  const { t } = useTranslation('schedule');
  const { value: v, set, form, loadError } = useConfigForm('holiday');
  return (
    <SettingsSection
      title={t('rows.holiday')}
      intro={t('holiday.intro')}
      form={form}
      loading={!v}
      loadError={loadError}
    >
      {v && (
        <>
          <Switch size="md" styles={SWITCH_TAP} label={t('holiday.enabled')} checked={v.enabled} onChange={(e) => set({ ...v, enabled: e.currentTarget.checked })} />
          <DatePickerInput
            type="range"
            label={t('holiday.away')}
            description={t('holiday.awayHint')}
            valueFormat="D MMM YYYY"
            firstDayOfWeek={1}
            allowSingleDateInRange
            value={[v.start || null, v.end || null]}
            onChange={([start, end]) => set({ ...v, start: start ?? '', end: end ?? start ?? '' })}
          />
          <Labelled label={t('holiday.level')}>
            <LevelChips value={v.level} onChange={(level) => set({ ...v, level })} label={t('holiday.level')} />
          </Labelled>
          <Group grow align="flex-start">
            <NumberInput label={t('holiday.every')} rightSection={unit('h')} min={1} max={24} value={v.interval_h} onChange={(n) => set({ ...v, interval_h: num(n, 2) })} />
            <NumberInput label={t('holiday.for')} rightSection={unit('min')} min={5} max={300} value={v.active_min} onChange={(n) => set({ ...v, active_min: num(n, 60) })} />
          </Group>
          {v.start && v.end && v.level > 0 && (
            <Text size="sm">
              {t('holiday.sentence', { level: levelWord(v.level), min: v.active_min, h: v.interval_h, when: fromTo(t, v.start, v.end) })}
            </Text>
          )}
        </>
      )}
    </SettingsSection>
  );
}

function ProgramsSection() {
  const { t } = useTranslation('schedule');
  const { value: v, set, form, loadError } = useConfigForm('programs');
  return (
    <SettingsSection
      title={t('rows.partyQuiet')}
      intro={t('programs.intro')}
      form={form}
      loading={!v}
      loadError={loadError}
    >
      {v && (
        <>
          <Labelled label={t('programs.partyLevel')}>
            <LevelChips value={v.party_level} onChange={(party_level) => set({ ...v, party_level })} label={t('programs.partyLevel')} />
          </Labelled>
          <NumberInput label={t('programs.partyLength')} rightSection={unit('min')} min={5} max={180} step={5} maw={220} value={v.party_min} onChange={(n) => set({ ...v, party_min: num(n, 5) })} />
          <Labelled label={t('programs.quietLevel')}>
            <LevelChips value={v.quiet_level} onChange={(quiet_level) => set({ ...v, quiet_level })} label={t('programs.quietLevel')} />
          </Labelled>
          <NumberInput label={t('programs.quietLength')} rightSection={unit('min')} min={5} max={180} step={5} maw={220} value={v.quiet_min} onChange={(n) => set({ ...v, quiet_min: num(n, 5) })} />
        </>
      )}
    </SettingsSection>
  );
}

function CoolingSection() {
  const { t } = useTranslation('schedule');
  const [ask, setAsk] = useState<null | ((ok: boolean) => void)>(null);
  const [plugOut, setPlugOut] = useState(false);
  // Turning cooling on needs the rubber plug removed (condensate), as the stock UI warns.
  const beforeSave = (next: Schemas['BypassConfig'], saved: Schemas['BypassConfig']) =>
    next.enabled && !saved.enabled
      ? new Promise<boolean>((resolve) => {
          setPlugOut(false);
          setAsk(() => resolve);
        })
      : Promise.resolve(true);
  const { value: v, set, form, loadError } = useConfigForm('bypass', beforeSave);
  const answer = (ok: boolean) => {
    ask?.(ok);
    setAsk(null);
  };

  return (
    <SettingsSection
      title={t('rows.cooling')}
      intro={t('cooling.intro')}
      form={form}
      loading={!v}
      loadError={loadError}
    >
      {v && (
        <>
          <Switch
            size="md"
            styles={SWITCH_TAP}
            label={t('cooling.enabled')}
            description={t('cooling.enabledHint')}
            checked={v.enabled}
            onChange={(e) => set({ ...v, enabled: e.currentTarget.checked })}
          />
          <Group grow align="flex-start">
            <NumberInput
              label={t('cooling.target')}
              rightSection={unit('°C')}
              min={10}
              max={35}
              step={0.5}
              decimalScale={1}
              value={v.target_c}
              onChange={(n) => set({ ...v, target_c: num(n, 23) })}
            />
            <NumberInput
              label={t('cooling.minOutdoor')}
              description={t('cooling.minOutdoorHint')}
              rightSection={unit('°C')}
              min={0}
              max={30}
              step={0.5}
              decimalScale={1}
              value={v.min_outdoor_c}
              onChange={(n) => set({ ...v, min_outdoor_c: num(n, 12) })}
            />
          </Group>
          <Switch
            size="md"
            styles={SWITCH_TAP}
            label={t('cooling.closeIfWarmer')}
            checked={v.close_if_outdoor_warmer}
            onChange={(e) => set({ ...v, close_if_outdoor_warmer: e.currentTarget.checked })}
          />
          <Switch
            size="md"
            styles={SWITCH_TAP}
            label={t('cooling.season')}
            checked={v.season !== null}
            onChange={(e) => set({ ...v, season: e.currentTarget.checked ? { start: '05-01', end: '09-30' } : null })}
          />
          {v.season && (
            <Group grow align="flex-start">
              <MonthDayInput label={t('cooling.from')} value={v.season.start} onChange={(start) => set({ ...v, season: { ...v.season!, start } })} />
              <MonthDayInput label={t('cooling.until')} value={v.season.end} onChange={(end) => set({ ...v, season: { ...v.season!, end } })} />
            </Group>
          )}
        </>
      )}
      <ConfirmModal
        opened={ask !== null}
        title={t('cooling.plugTitle')}
        confirmLabel={t('cooling.plugConfirm')}
        confirmDisabled={!plugOut}
        onConfirm={() => answer(true)}
        onClose={() => answer(false)}
      >
        <Stack gap="md">
          <Text>{t('cooling.plugText')}</Text>
          <Checkbox styles={CHECKBOX_TAP} label={t('cooling.plugOut')} checked={plugOut} onChange={(e) => setPlugOut(e.currentTarget.checked)} />
        </Stack>
      </ConfirmModal>
    </SettingsSection>
  );
}

function BackToAutoSection() {
  const { t } = useTranslation('schedule');
  const { value: v, set, form, loadError } = useConfigForm('schedule');
  const returning = v?.auto_return_min !== null && v?.auto_return_min !== undefined;
  return (
    <SettingsSection
      title={t('rows.backToAuto')}
      intro={t('backToAuto.intro')}
      form={form}
      loading={!v}
      loadError={loadError}
    >
      {v && (
        <>
          <Switch size="md" styles={SWITCH_TAP} label={t('backToAuto.midnight')} checked={v.auto_at_midnight} onChange={(e) => set({ ...v, auto_at_midnight: e.currentTarget.checked })} />
          <Switch
            size="md"
            styles={SWITCH_TAP}
            label={t('backToAuto.afterChange')}
            checked={returning}
            onChange={(e) => set({ ...v, auto_return_min: e.currentTarget.checked ? 120 : null })}
          />
          {returning && (
            <NumberInput
              label={t('backToAuto.after')}
              rightSection={unit('min')}
              min={10}
              max={1440}
              step={10}
              maw={220}
              value={v.auto_return_min ?? 120}
              onChange={(n) => set({ ...v, auto_return_min: num(n, 120) })}
            />
          )}
          <Labelled label={t('backToAuto.fallback')}>
            <LevelChips value={v.fallback_level} onChange={(fallback_level) => set({ ...v, fallback_level })} label={t('backToAuto.fallback')} />
          </Labelled>
        </>
      )}
    </SettingsSection>
  );
}
