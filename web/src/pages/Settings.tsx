import {
  ActionIcon,
  Alert,
  Anchor,
  Box,
  Button,
  Card,
  Code,
  type ComboboxItem,
  CopyButton,
  FileButton,
  Group,
  Loader,
  NumberInput,
  type OptionsFilter,
  PasswordInput,
  Select,
  SimpleGrid,
  Skeleton,
  Stack,
  Switch,
  Table,
  Text,
  TextInput,
  Title,
  Tooltip,
} from '@mantine/core';
import { IconCheck, IconCopy, IconLogout } from '@tabler/icons-react';
import { type FormEvent, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ApiError, api, expectRestart, type Schemas, setToken, type Status } from '../api/client';
import { ConfirmModal } from '../components/ConfirmModal';
import { SectionList } from '../components/SectionList';
import { type FormState, SettingsSection, saveError, useConfigForm } from '../components/SettingsSection';
import { DotBadge } from '../components/StatusBadge';
import { ActionDone, ActionError, useAction, useFlash } from '../components/Feedback';
import { type LangChoice, language, languageChoice, resolve, setLanguage, t } from '../i18n';
import { clock, number } from '../i18n/format';
import { TechnicalDetails } from '../components/TechnicalDetails';
import { UpdateSection } from '../components/UpdateSection';
import { duration } from '../lib/format';
import { usePoll } from '../lib/hooks';
import { reading, splitUnit } from '../lib/readings';
import { clearResources, useConfig, useMqtt, useSystem } from '../lib/resources';
import { afterLeaveGuard, navigate } from '../lib/router';
import { filterSummary, minutes, uptime } from '../lib/summaries';
import { PASSWORD_TAP, SWITCH_TAP, TAP } from '../lib/tap';

// Monospace for the typed value only: `ff` on an input also sets its label and description.
const MONO_VALUE = { styles: { input: { fontFamily: 'var(--mantine-font-family-monospace)' } } };

/** How long the fans stand still during a restart (bootloader + start-up). */
const RESTART_S = 20;
/** The same with a firmware install: the bootloader erases and programs the flash first. */
export const INSTALL_S = 60;
const num = (v: string | number, fallback = 0) => (typeof v === 'number' ? v : Number.parseFloat(v) || fallback);
const unit = (u: string) => (
  <Text size="sm" c="dimmed" pr={4}>
    {u}
  </Text>
);

/**
 * Logs out and returns to the login, without a confirm of its own. With unsaved changes in a
 * section, the leave guard asks first ("Discard"), like any other way out of the section.
 */
export function logout() {
  afterLeaveGuard(() => {
    void (async () => {
      await api.logout().catch(() => undefined);
      navigate('/', { replace: true, force: true });
      setToken(null);
      clearResources();
    })();
  });
}

/** Settings: what this device is and how it's set up (KWL screen spec "Settings"). */
export function SettingsPage({ section, status, onChanged }: { section: string | null; status: Status | null; onChanged: () => Promise<void> }) {
  const { t } = useTranslation(['settings', 'common', 'app', 'update']);
  const system = useSystem();
  const mqtt = useMqtt();
  const filter = useConfig('filter');
  const network = useConfig('network');
  const time = useConfig('time');
  const fans = useConfig('fans');
  const frost = useConfig('frost');
  const bypass = useConfig('bypass');
  const sys = system.data;
  const has = (f: Status['faults'][number]) => status?.faults.includes(f) ?? false;
  const haFailing = mqtt.data?.enabled === true && sys?.mqtt_connected === false;
  const clockSet = status ? !has('time_not_set') : undefined;

  const identity = sys && (
    <>
      {sys.device.model || 'Helios KWL'} · {t('firmware')}{' '}
      <Text span ff="monospace" inherit>
        {sys.firmware_version}
      </Text>
      {sys.ip && (
        <>
          {' · '}
          <Text span ff="monospace" inherit>
            {sys.ip}
          </Text>
        </>
      )}
    </>
  );

  return (
    <SectionList
      page="settings"
      title={t('title')}
      intro={identity}
      selected={section}
      groups={[
        {
          label: t('groups.care'),
          rows: [
            {
              id: 'filters',
              title: t('filters.title'),
              summary: filterSummary(status?.filter, filter.data?.interval_months),
              badge: status?.filter.due ? { label: t('badge.due'), color: 'yellow' } : undefined,
            },
            { id: 'restart', title: t('restart.title'), summary: sys ? uptime(sys.uptime_s) : undefined },
          ],
        },
        {
          label: t('groups.connections'),
          rows: [
            {
              id: 'home-assistant',
              title: t('ha.title'),
              summary: !mqtt.data ? undefined : !mqtt.data.enabled ? t('common:off') : sys?.mqtt_connected ? t('summary.connected') : t('summary.notConnected'),
              badge: haFailing ? { label: t('badge.notConnected'), color: 'yellow' } : undefined,
            },
            {
              id: 'network',
              title: t('network.title'),
              summary: sys && network.data ? `${sys.ip ?? t('summary.noAddress')} · ${network.data.dhcp ? t('summary.automatic') : t('summary.fixedAddress')}` : undefined,
            },
            {
              id: 'clock',
              title: t('clock.title'),
              summary:
                clockSet === false
                  ? t('summary.notSet')
                  : status?.time && time.data
                    ? `${time.data.ntp_enabled ? t('summary.fromNetwork') : t('summary.byHand')} · ${clock(status.time.slice(11, 16))}`
                    : undefined,
              badge: clockSet === false ? { label: t('badge.notSet'), color: 'yellow' } : undefined,
            },
          ],
        },
        {
          label: t('groups.access'),
          rows: [
            { id: 'language', title: t('app:language.title'), summary: languageSummary() },
            { id: 'password', title: t('password.title') },
            { id: 'logout', title: t('logout'), action: { onClick: logout, icon: <IconLogout size={20} stroke={1.6} /> } },
          ],
        },
        {
          label: t('groups.advanced'),
          note: t('common:advancedNote'),
          rows: [
            {
              id: 'fan-levels',
              title: t('fans.title'),
              summary: fans.data ? t('summary.fanLevels', { n: fans.data.m3h_per_krpm }) : undefined,
            },
            {
              id: 'frost-protection',
              title: t('frost.title'),
              summary: frost.data ? t('summary.frost', { temp: reading(frost.data.arm_below_c, 'temperature') }) : undefined,
              badge: status?.frost.active ? { label: t('badge.active'), color: 'blue' } : undefined,
            },
            {
              id: 'bypass-actuator',
              title: t('actuator.title'),
              summary: bypass.data ? `${t(`actuator.types.${bypass.data.actuator}`)} · ${bypass.data.run_time_s} s` : undefined,
            },
            { id: 'backup', title: t('backup.title'), summary: t('summary.backup') },
            { id: 'update', title: t('update:title'), summary: sys ? `openKWL ${sys.firmware_version}` : undefined },
            {
              id: 'technical-details',
              title: t('technical.title'),
              summary: t('summary.technical'),
              badge: has('sd_card') || has('config_invalid') ? { label: t('badge.check'), color: 'yellow' } : undefined,
            },
          ],
        },
      ]}
      render={(id) => {
        switch (id) {
          case 'filters':
            return <FiltersSection status={status} onChanged={onChanged} />;
          case 'restart':
            return <RestartSection />;
          case 'home-assistant':
            return <HomeAssistantSection />;
          case 'network':
            return <NetworkSection />;
          case 'clock':
            return <ClockSection />;
          case 'language':
            return <LanguageSection />;
          case 'password':
            return <PasswordSection />;
          case 'fan-levels':
            return <FanLevelsSection />;
          case 'frost-protection':
            return <FrostSection />;
          case 'bypass-actuator':
            return <ActuatorSection />;
          case 'backup':
            return <BackupSection />;
          case 'update':
            return <UpdateSection />;
          default:
            return <TechnicalSection status={status} />;
        }
      }}
    />
  );
}

// ---- Language (this browser only) -----------------------------------------------------

/** Each language in its own name: the same in every language. */
const LANGUAGE_NAMES = { en: 'English', de: 'Deutsch' } as const;

function languageSummary(): string {
  return languageChoice() === 'auto' ? t('app:language.auto', { name: LANGUAGE_NAMES[language()] }) : LANGUAGE_NAMES[language()];
}

function LanguageSection() {
  const browser = resolve('auto').lang;
  return (
    <SettingsSection title={t('app:language.title')} intro={t('app:language.intro')}>
      <Select
        label={t('app:language.label')}
        maw={320}
        allowDeselect={false}
        data={[
          { value: 'auto', label: t('app:language.auto', { name: LANGUAGE_NAMES[browser] }) },
          { value: 'de', label: LANGUAGE_NAMES.de },
          { value: 'en', label: LANGUAGE_NAMES.en },
        ]}
        value={languageChoice()}
        // Takes effect at once (the page redraws in the new language); nothing to save.
        onChange={(v) => setLanguage((v ?? 'auto') as LangChoice)}
      />
    </SettingsSection>
  );
}

// ---- Care ---------------------------------------------------------------------------

function FiltersSection({ status, onChanged }: { status: Status | null; onChanged: () => Promise<void> }) {
  const { t } = useTranslation(['settings', 'common']);
  const { value: v, set, form, loadError } = useConfigForm('filter');
  const [asking, setAsking] = useState(false);
  const reset = useAction();
  const due = status?.filter.due ?? false;
  async function confirmed() {
    await reset.run(async () => {
      await api.resetFilter();
      await onChanged();
    }, t('filters.confirmFailed'));
    setAsking(false);
  }
  return (
    <SettingsSection title={t('filters.title')} form={form} loading={!v || !status} loadError={loadError}>
      {due ? (
        <Stack gap="sm" align="flex-start">
          <DotBadge color="yellow">{t('badge.due')}</DotBadge>
          <Text>{t('filters.dueText')}</Text>
          <Button onClick={() => setAsking(true)}>{t('filters.confirmButton')}</Button>
        </Stack>
      ) : (
        status && (
          <div>
            <Text>{t('filters.next', { days: reading(status.filter.remaining_days, 'days') })}</Text>
            <Anchor component="button" size="sm" c="var(--app-brand-text)" onClick={() => setAsking(true)} style={TAP}>
              {t('filters.resetEarly')}
            </Anchor>
          </div>
        )
      )}
      {v && (
        <NumberInput
          label={t('filters.interval')}
          description={t('filters.intervalHint')}
          rightSection={unit(t('common:months'))}
          rightSectionWidth={64}
          min={0}
          max={24}
          maw={220}
          value={v.interval_months}
          onChange={(n) => set({ ...v, interval_months: num(n, 3) })}
        />
      )}
      <ActionError action={reset} />
      <ActionDone action={reset}>{t('filters.restarted')}</ActionDone>
      <ConfirmModal opened={asking} title={t('filters.askTitle')} confirmLabel={t('filters.askConfirm')} onConfirm={confirmed} onClose={() => setAsking(false)} />
    </SettingsSection>
  );
}

/** Restart with the running/back/not-back states (KWL ConfirmModal); `install` after a
 *  firmware update was uploaded, when the restart also installs it. `confirmed` skips the
 *  question (the update dialog asked already) and restarts at once. */
export function RestartModal({
  opened,
  onClose,
  install = false,
  confirmed = false,
}: {
  opened: boolean;
  onClose: () => void;
  install?: boolean;
  confirmed?: boolean;
}) {
  const { t } = useTranslation('settings');
  const seconds = install ? INSTALL_S : RESTART_S;
  const [stage, setStage] = useState<'confirm' | 'running' | 'back' | 'late'>('confirm');
  const restart = useAction();
  async function reboot() {
    if (await restart.run(() => api.reboot(), t('restart.failed'))) {
      expectRestart(true);
      setStage('running');
    }
  }
  useEffect(() => {
    if (!opened) return;
    setStage('confirm');
    // Only when the dialog opens: the update dialog asked already.
    if (confirmed) void reboot();
  }, [opened]);
  const waiting = stage === 'running' || stage === 'late';
  useEffect(() => {
    if (!waiting) return;
    // Sessions live in RAM: once the unit answers again, everyone logs in anew. Keeps asking
    // after "late" too, so a unit that comes back later is noticed.
    let down = false;
    const started = Date.now();
    const id = setInterval(() => {
      if (Date.now() - started > 3 * seconds * 1000) setStage('late');
      api
        .setupStatus()
        .then(() => {
          if (down || Date.now() - started > 8000) setStage('back');
        })
        .catch(() => {
          down = true;
        });
    }, 2000);
    return () => clearInterval(id);
  }, [waiting, seconds]);
  useEffect(() => {
    if (stage !== 'back' || !install) return;
    // The update also replaced the web UI: load it, then log in.
    const id = setTimeout(() => {
      expectRestart(false);
      setToken(null);
      window.location.reload();
    }, 1500);
    return () => clearTimeout(id);
  }, [stage, install]);

  const running =
    stage === 'running' ? (
      <Group gap="md" wrap="nowrap">
        <Loader size="sm" />
        <Text>
          {install
            ? t('restart.installing', { s: seconds })
            : t('restart.restarting', { s: seconds })}
        </Text>
      </Group>
    ) : stage === 'back' && install ? (
      <Group gap="md" wrap="nowrap">
        <Loader size="sm" />
        <Text>{t('restart.backLoading')}</Text>
      </Group>
    ) : stage === 'back' ? (
      <Stack gap="md">
        <Text>{t('restart.back')}</Text>
        <Group justify="flex-end">
          <Button
            onClick={() => {
              expectRestart(false);
              setToken(null);
            }}
          >
            {t('restart.logIn')}
          </Button>
        </Group>
      </Stack>
    ) : stage === 'late' ? (
      <Stack gap="md">
        <Text>{t('restart.late')}</Text>
        <Group justify="flex-end">
          <Button variant="default" onClick={() => window.location.reload()}>
            {t('restart.reload')}
          </Button>
        </Group>
      </Stack>
    ) : undefined;

  return (
    <ConfirmModal
      opened={opened}
      title={install ? t('restart.askInstall') : t('restart.ask')}
      confirmLabel={install ? t('restart.confirmInstall') : t('restart.confirm')}
      running={running}
      onConfirm={reboot}
      onClose={() => {
        restart.clear();
        onClose();
      }}
    >
      <Stack gap="sm">
        <Text>
          {install
            ? t('restart.textInstall', { s: seconds })
            : t('restart.text', { s: seconds })}
        </Text>
        <ActionError action={restart} />
      </Stack>
    </ConfirmModal>
  );
}

function RestartSection() {
  const { t } = useTranslation('settings');
  const [open, setOpen] = useState(false);
  return (
    <SettingsSection title={t('restart.title')} intro={t('restart.intro')}>
      <Group>
        <Button variant="default" onClick={() => setOpen(true)}>
          {t('restart.button')}
        </Button>
      </Group>
      <RestartModal opened={open} onClose={() => setOpen(false)} />
    </SettingsSection>
  );
}

// ---- Connections ----------------------------------------------------------------------

function HomeAssistantSection() {
  const { t } = useTranslation('settings');
  const mqtt = useMqtt();
  const details = usePoll(api.systemDetails, 5000);
  const [draft, setDraft] = useState<Schemas['MqttConfig'] | null>(null);
  // A typed new password; empty keeps the saved one (the API's null). Removing the saved one is
  // its own step (`removing`), so emptying the field again never clears it by accident.
  const [password, setPassword] = useState('');
  const [removing, setRemoving] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [justSaved, flash] = useFlash();
  const v = draft ?? mqtt.data ?? null;
  const dirty = (draft !== null && JSON.stringify(draft) !== JSON.stringify(mqtt.data)) || password !== '' || removing;
  const form: FormState = {
    dirty,
    saving,
    error,
    saved: justSaved,
    save: async () => {
      if (!v) return;
      setSaving(true);
      setError(null);
      try {
        const { password_set: _, ...rest } = v;
        mqtt.set(await api.putMqtt({ ...rest, password: password !== '' ? password : removing ? '' : null }));
        setDraft(null);
        setPassword('');
        setRemoving(false);
        flash();
      } catch (e) {
        setError(saveError(e));
      } finally {
        setSaving(false);
      }
    },
    discard: () => {
      setDraft(null);
      setPassword('');
      setRemoving(false);
      setError(null);
    },
  };
  const set = (x: Schemas['MqttConfig']) => setDraft(x);
  const q = details.data?.mqtt;
  // The unit keeps the saved password only for the same broker.
  const saved = mqtt.data;
  const retype = !!v && !!saved?.password_set && (v.host !== saved.host || v.port !== saved.port);
  const removeLink = (label: string, remove: boolean) => (
    <Anchor
      component="button"
      type="button"
      inherit
      c="var(--app-brand-text)"
      style={TAP}
      onClick={() => {
        setRemoving(remove);
        if (remove) setPassword('');
      }}
    >
      {label}
    </Anchor>
  );
  // The same line while typing, so nothing moves; a new broker drops the saved password anyway.
  const passwordNote = !saved?.password_set ? undefined : retype ? (
    t('ha.retype')
  ) : removing ? (
    <>
      {t('ha.passwordRemoved')} {removeLink(t('ha.keepPassword'), false)}
    </>
  ) : (
    removeLink(t('ha.removePassword'), true)
  );
  return (
    <SettingsSection
      title={t('ha.title')}
      intro={t('ha.intro')}
      form={form}
      loading={!v}
      loadError={mqtt.error}
      advanced={
        v && (
          <>
            <TextInput label={t('ha.topic')} {...MONO_VALUE} value={v.base_topic} onChange={(e) => set({ ...v, base_topic: e.currentTarget.value })} />
            <TextInput label={t('ha.clientId')} {...MONO_VALUE} value={v.client_id} onChange={(e) => set({ ...v, client_id: e.currentTarget.value })} />
            <Switch size="md" styles={SWITCH_TAP} label={t('ha.discovery')} checked={v.discovery} onChange={(e) => set({ ...v, discovery: e.currentTarget.checked })} />
            {v.discovery && (
              <TextInput label={t('ha.discoveryPrefix')} {...MONO_VALUE} value={v.discovery_prefix} onChange={(e) => set({ ...v, discovery_prefix: e.currentTarget.value })} />
            )}
          </>
        )
      }
    >
      {q?.enabled &&
        (q.connected ? (
          <Text size="sm" c="var(--app-success)" fw={500}>
            {t('ha.connectedTo')}{' '}
            <Text span ff="monospace" inherit>
              {q.broker}
            </Text>{' '}
            · {t('ha.messages', { count: q.published, n: number(q.published) })}
          </Text>
        ) : (
          <Text size="sm" c="var(--app-danger)" fw={500}>
            {t('ha.notConnectedTo')}{' '}
            <Text span ff="monospace" inherit>
              {q.broker || t('ha.theBroker')}
            </Text>
          </Text>
        ))}
      {v && (
        <>
          <Switch size="md" styles={SWITCH_TAP} label={t('ha.enable')} checked={v.enabled} onChange={(e) => set({ ...v, enabled: e.currentTarget.checked })} />
          <Group grow align="flex-start">
            <TextInput label={t('ha.broker')} description={t('ha.brokerHint')} {...MONO_VALUE} value={v.host} onChange={(e) => set({ ...v, host: e.currentTarget.value })} />
            <NumberInput label={t('ha.port')} {...MONO_VALUE} min={1} max={65535} maw={120} hideControls value={v.port} onChange={(n) => set({ ...v, port: num(n, 1883) })} />
          </Group>
          <Group grow align="flex-start">
            <TextInput label={t('ha.username')} value={v.username} onChange={(e) => set({ ...v, username: e.currentTarget.value })} />
            <PasswordInput {...PASSWORD_TAP}
              label={t('ha.password')}
              placeholder={v.password_set && !retype && !removing ? t('ha.passwordSaved') : undefined}
              description={passwordNote}
              value={password}
              onChange={(e) => {
                setPassword(e.currentTarget.value);
                if (e.currentTarget.value !== '') setRemoving(false);
              }}
              autoComplete="new-password"
            />
          </Group>
        </>
      )}
    </SettingsSection>
  );
}

function NetworkSection() {
  const { t } = useTranslation('settings');
  const [savedOnce, setSavedOnce] = useState(false);
  const [restart, setRestart] = useState(false);
  const { value: v, set, form, loadError } = useConfigForm('network');
  const wrapped: FormState = {
    ...form,
    save: async () => {
      await form.save();
      setSavedOnce(true);
    },
  };
  const ip = (label: string, key: 'address' | 'netmask' | 'gateway' | 'dns') =>
    v && <TextInput label={label} {...MONO_VALUE} value={v[key] ?? ''} onChange={(e) => set({ ...v, [key]: e.currentTarget.value || null })} />;
  return (
    <SettingsSection
      title={t('network.title')}
      intro={t('network.intro')}
      form={wrapped}
      loading={!v}
      loadError={loadError}
      footer={
        savedOnce &&
        !form.dirty &&
        !form.error && (
          <Alert color="yellow">
            <Group justify="space-between" gap="sm">
              <Text size="sm">{t('network.restartNeeded')}</Text>
              <Button size="xs" variant="default" onClick={() => setRestart(true)}>
                {t('restart.button')}
              </Button>
            </Group>
          </Alert>
        )
      }
    >
      {v && (
        <>
          <TextInput label={t('network.hostname')} {...MONO_VALUE} value={v.hostname} onChange={(e) => set({ ...v, hostname: e.currentTarget.value })} />
          <Switch size="md" styles={SWITCH_TAP} label={t('network.dhcp')} checked={v.dhcp} onChange={(e) => set({ ...v, dhcp: e.currentTarget.checked })} />
          {!v.dhcp && (
            <SimpleGrid cols={{ base: 1, xs: 2 }}>
              {ip(t('network.address'), 'address')}
              {ip(t('network.netmask'), 'netmask')}
              {ip(t('network.gateway'), 'gateway')}
              {ip(t('network.dns'), 'dns')}
            </SimpleGrid>
          )}
        </>
      )}
      <RestartModal opened={restart} onClose={() => setRestart(false)} />
    </SettingsSection>
  );
}

/** Every whole and half hour from UTC−12:00 to UTC+14:00, and the three quarter-hour zones. */
const UTC_OFFSETS = [...Array.from({ length: 53 }, (_, i) => -720 + i * 30), 345, 525, 765].sort((a, b) => a - b);
/** Names for the zones most units are in; the others show their offset only. */
const ZONE_NAMES: Record<number, 'wet' | 'cet' | 'eet'> = { 0: 'wet', 60: 'cet', 120: 'eet' };

/** 60 → "UTC+01:00", −210 → "UTC−03:30". */
function utcOffset(min: number): string {
  const a = Math.abs(min);
  const hhmm = `${String(Math.floor(a / 60)).padStart(2, '0')}:${String(a % 60).padStart(2, '0')}`;
  return `UTC${min < 0 ? '\u2212' : '+'}${hhmm}`;
}

/** The time zone choices, with `current` added if a restored backup holds another offset. */
function zoneOptions(current: number): ComboboxItem[] {
  const all = UTC_OFFSETS.includes(current) ? UTC_OFFSETS : [...UTC_OFFSETS, current].sort((a, b) => a - b);
  return all.map((min) => {
    const name = ZONE_NAMES[min];
    return { value: String(min), label: name ? `${utcOffset(min)} · ${t(`settings:clock.zones.${name}`)}` : utcOffset(min) };
  });
}

/** Search that also finds "UTC−05:00" when typing "-5" (the labels use a true minus). */
const zoneFilter: OptionsFilter = ({ options, search }) => {
  const q = search.trim().toLowerCase().replace(/-/g, '\u2212');
  return (options as ComboboxItem[]).filter((o) => o.label.toLowerCase().includes(q) || o.label.replace(/(\D)0(\d)/g, '$1$2').toLowerCase().includes(q));
};

function ClockSection() {
  const { t } = useTranslation('settings');
  const { value: v, set, form, loadError } = useConfigForm('time');
  const [details, setDetails] = useState<Schemas['SystemDetails'] | null>(null);
  useEffect(() => {
    api.systemDetails().then(setDetails).catch(() => undefined);
  }, []);
  const setClock = useAction();
  const setFromDevice = () => setClock.run(() => api.setTime(Math.round(Date.now() / 1000)), t('clock.setFailed'));
  const n = details?.network;
  // Empty means the router (DHCP gateway) if it answers NTP, else pool.ntp.org.
  const router = n?.ntp_server ?? (n?.dhcp ? n.gateway : null) ?? 'pool.ntp.org';
  return (
    <SettingsSection title={t('clock.title')} intro={t('clock.intro')} form={form} loading={!v} loadError={loadError}>
      {v && (
        <>
          <Switch size="md" styles={SWITCH_TAP} label={t('clock.ntp')} checked={v.ntp_enabled} onChange={(e) => set({ ...v, ntp_enabled: e.currentTarget.checked })} />
          {v.ntp_enabled && (
            <TextInput label={t('clock.server')} {...MONO_VALUE} placeholder={router} value={v.ntp_server} onChange={(e) => set({ ...v, ntp_server: e.currentTarget.value.trim() })} />
          )}
          <Group>
            <Button variant="default" onClick={setFromDevice} loading={setClock.busy}>
              {t('clock.setFromDevice')}
            </Button>
            <ActionDone action={setClock}>{t('clock.set')}</ActionDone>
          </Group>
          <ActionError action={setClock} />
          <Group grow align="flex-end">
            <Select
              label={t('clock.zone')}
              data={zoneOptions(v.utc_offset_min)}
              searchable
              filter={zoneFilter}
              value={String(v.utc_offset_min)}
              onChange={(x) => set({ ...v, utc_offset_min: Number(x ?? 60) })}
              allowDeselect={false}
            />
            <Switch size="md" styles={SWITCH_TAP} mb={8} label={t('clock.dst')} checked={v.eu_dst} onChange={(e) => set({ ...v, eu_dst: e.currentTarget.checked })} />
          </Group>
        </>
      )}
    </SettingsSection>
  );
}

// ---- Access -------------------------------------------------------------------------

function PasswordSection() {
  const { t } = useTranslation('settings');
  const [current, setCurrent] = useState('');
  const [next, setNext] = useState('');
  const [again, setAgain] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [asking, setAsking] = useState(false);
  const mismatch = again.length > 0 && again !== next;
  const ready = current !== '' && next.length >= 10 && next === again;

  function submit(e: FormEvent) {
    e.preventDefault();
    if (ready) setAsking(true);
  }
  async function change() {
    try {
      await api.changePassword(current, next);
      setAsking(false);
      clearResources();
      setToken(null);
    } catch (err) {
      setAsking(false);
      setError(err instanceof ApiError ? err.describe() : String(err));
    }
  }
  return (
    <SettingsSection title={t('password.title')}>
      <Stack component="form" onSubmit={submit} gap="md">
        <PasswordInput {...PASSWORD_TAP} label={t('password.current')} value={current} onChange={(e) => setCurrent(e.currentTarget.value)} autoComplete="current-password" />
        <PasswordInput {...PASSWORD_TAP} label={t('password.new')} description={t('password.newHint')} value={next} onChange={(e) => setNext(e.currentTarget.value)} autoComplete="new-password" />
        <PasswordInput {...PASSWORD_TAP}
          label={t('password.repeat')}
          value={again}
          error={mismatch ? t('password.mismatch') : null}
          onChange={(e) => setAgain(e.currentTarget.value)}
          autoComplete="new-password"
        />
        {error && <Alert color="red">{error}</Alert>}
        <Group>
          <Button type="submit" disabled={!ready}>
            {t('password.change')}
          </Button>
        </Group>
      </Stack>
      <ConfirmModal opened={asking} title={t('password.askTitle')} confirmLabel={t('password.change')} onConfirm={change} onClose={() => setAsking(false)}>
        {t('password.askText')}
      </ConfirmModal>
    </SettingsSection>
  );
}

// ---- Advanced -----------------------------------------------------------------------

function FanLevelsSection() {
  const { t } = useTranslation(['settings', 'common']);
  const { value: v, set, form, loadError } = useConfigForm('fans');
  const patch = (key: 'supply_volts' | 'extract_volts' | 'airflow_m3h', i: number, n: number) => {
    if (!v) return;
    const next = [...v[key]];
    next[i] = n;
    set({ ...v, [key]: next });
  };
  return (
    <SettingsSection title={t('fans.title')} intro={t('fans.intro')} form={form} loading={!v} loadError={loadError}>
      {v && (
        <>
          <Table withRowBorders={false} highlightOnHover={false} horizontalSpacing={4} verticalSpacing={4}>
            <Table.Thead>
              <Table.Tr>
                <Table.Th bg="transparent" />
                <Table.Th bg="transparent">{t('fans.supply')}</Table.Th>
                <Table.Th bg="transparent">{t('fans.exhaust')}</Table.Th>
                <Table.Th bg="transparent">{t('fans.measured')}</Table.Th>
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {[0, 1, 2, 3].map((i) => (
                <Table.Tr key={i}>
                  <Table.Td fw={500} style={{ whiteSpace: 'nowrap' }}>
                    {t('common:Level', { n: i + 1 })}
                  </Table.Td>
                  <Table.Td>
                    <NumberInput aria-label={t('fans.supplyAria', { n: i + 1 })} hideControls min={0} max={10} step={0.1} decimalScale={1} rightSection={unit('V')} value={v.supply_volts[i]} onChange={(n) => patch('supply_volts', i, num(n))} />
                  </Table.Td>
                  <Table.Td>
                    <NumberInput aria-label={t('fans.exhaustAria', { n: i + 1 })} hideControls min={0} max={10} step={0.1} decimalScale={1} rightSection={unit('V')} value={v.extract_volts[i]} onChange={(n) => patch('extract_volts', i, num(n))} />
                  </Table.Td>
                  <Table.Td>
                    <NumberInput aria-label={t('fans.measuredAria', { n: i + 1 })} hideControls min={0} max={1000} rightSection={unit('m³/h')} rightSectionWidth={44} value={v.airflow_m3h[i]} onChange={(n) => patch('airflow_m3h', i, Math.round(num(n)))} />
                  </Table.Td>
                </Table.Tr>
              ))}
            </Table.Tbody>
          </Table>
          <NumberInput
            label={t('fans.perKrpm')}
            description={t('fans.perKrpmHint')}
            rightSection={unit('m³/h')}
            rightSectionWidth={52}
            min={0}
            max={1000}
            maw={260}
            value={v.m3h_per_krpm}
            onChange={(n) => set({ ...v, m3h_per_krpm: Math.round(num(n)) })}
          />
        </>
      )}
    </SettingsSection>
  );
}

function FrostSection() {
  const { t } = useTranslation('settings');
  const { value: v, set, form, loadError } = useConfigForm('frost');
  const min = (s: number) => Math.round(s / 60);
  return (
    <SettingsSection
      title={t('frost.title')}
      intro={t('frost.intro')}
      form={form}
      loading={!v}
      loadError={loadError}
    >
      {v && (
        <>
          <SimpleGrid cols={{ base: 1, xs: 2 }} spacing="sm">
            <Step n={1} title={t('frost.step1Title')} text={t('frost.step1Text', { temp: reading(v.arm_below_c, 'temperature'), pct: reading(v.entry_pct, 'percent') })} />
            <Step n={2} title={t('frost.step2Title')} text={t('frost.step2Text', { boost: minutes(min(v.boost_delay_s)), defrost: minutes(min(v.defrost_s)) })} />
          </SimpleGrid>
          <SimpleGrid cols={{ base: 1, xs: 2 }}>
            <NumberInput label={t('frost.armBelow')} rightSection={unit('°C')} step={0.1} decimalScale={1} min={-25} max={0} value={v.arm_below_c} onChange={(n) => set({ ...v, arm_below_c: num(n) })} />
            <NumberInput label={t('frost.boost')} rightSection={unit('V')} step={0.1} decimalScale={1} min={0} max={2} value={v.exhaust_boost_v} onChange={(n) => set({ ...v, exhaust_boost_v: num(n) })} />
            <NumberInput label={t('frost.entry')} rightSection={unit('%')} min={0} max={100} value={v.entry_pct} onChange={(n) => set({ ...v, entry_pct: num(n) })} />
            <NumberInput label={t('frost.exit')} rightSection={unit('%')} min={0} max={100} value={v.exit_pct} onChange={(n) => set({ ...v, exit_pct: num(n), hold_pct: num(n) })} />
            <NumberInput label={t('frost.boostDelay')} rightSection={unit('min')} min={1} max={60} value={min(v.boost_delay_s)} onChange={(n) => set({ ...v, boost_delay_s: num(n, 15) * 60 })} />
            <NumberInput label={t('frost.defrost')} rightSection={unit('min')} min={10} max={240} value={min(v.defrost_s)} onChange={(n) => set({ ...v, defrost_s: num(n, 180) * 60 })} />
            <NumberInput label={t('frost.testRun')} rightSection={unit('min')} min={1} max={60} value={min(v.test_run_s)} onChange={(n) => set({ ...v, test_run_s: num(n, 15) * 60 })} />
          </SimpleGrid>
        </>
      )}
    </SettingsSection>
  );
}

/** One step of the frost-protection picture. */
function Step({ n, title, text }: { n: number; title: string; text: string }) {
  const { t } = useTranslation('settings');
  return (
    <Card p="sm" bg="var(--app-info-soft)" withBorder={false} radius="md">
      <Text size="xs" fw={600} c="var(--app-info)">
        {t('frost.step', { n })}
      </Text>
      <Text size="sm" fw={600}>
        {title}
      </Text>
      <Text size="xs" c="dimmed">
        {text}
      </Text>
    </Card>
  );
}

function ActuatorSection() {
  const { t } = useTranslation('settings');
  const { value: v, set, form, loadError } = useConfigForm('bypass');
  return (
    <SettingsSection title={t('actuator.title')} form={form} loading={!v} loadError={loadError}>
      {v && (
        <>
          <Select
            label={t('actuator.label')}
            data={[
              { value: 'motor', label: t('actuator.types.motor') },
              { value: 'motor_reversed', label: t('actuator.types.motor_reversed') },
              { value: 'analog', label: t('actuator.types.analog') },
            ]}
            value={v.actuator}
            onChange={(x) => set({ ...v, actuator: (x ?? 'motor') as Schemas['ActuatorType'] })}
            allowDeselect={false}
            maw={320}
          />
          <NumberInput label={t('actuator.runTime')} rightSection={unit('s')} min={5} max={600} maw={220} value={v.run_time_s} onChange={(n) => set({ ...v, run_time_s: num(n, 120) })} />
        </>
      )}
    </SettingsSection>
  );
}

function BackupSection() {
  const { t } = useTranslation('settings');
  const [file, setFile] = useState<File | null>(null);
  const backup = useAction();
  const restore = useAction();
  const download = () =>
    backup.run(async () => {
      const text = await api.exportConfig();
      const url = URL.createObjectURL(new Blob([text], { type: 'text/plain' }));
      const a = document.createElement('a');
      a.href = url;
      a.download = 'CONFIG.INI';
      a.click();
      URL.revokeObjectURL(url);
    }, t('backup.downloadFailed'));
  async function upload() {
    if (!file) return;
    const text = await file.text();
    setFile(null);
    backup.clear();
    // Every section and summary loads the new values.
    if (await restore.run(() => api.importConfig(text), t('backup.uploadFailed'))) clearResources();
  }
  return (
    <SettingsSection title={t('backup.title')} intro={t('backup.intro')}>
      <Group>
        <Button variant="default" onClick={download} loading={backup.busy}>
          {t('backup.download')}
        </Button>
        <FileButton onChange={setFile} accept=".ini,.INI,text/plain">
          {(props) => (
            <Button variant="default" {...props} loading={restore.busy}>
              {t('backup.upload')}
            </Button>
          )}
        </FileButton>
        <ActionDone action={restore}>{t('backup.restored')}</ActionDone>
      </Group>
      <ActionError action={backup} />
      <ActionError action={restore} />
      <ConfirmModal opened={file !== null} title={t('backup.askTitle')} confirmLabel={t('backup.askConfirm')} onConfirm={upload} onClose={() => setFile(null)}>
        {t('backup.askText')}
      </ConfirmModal>
    </SettingsSection>
  );
}

// ---- Technical details --------------------------------------------------------------

function TechnicalSection({ status }: { status: Status | null }) {
  return (
    <Stack gap="lg">
      <DeviceData />
      {status && <LiveValues status={status} />}
      <TechnicalDetails />
      <Log />
    </Stack>
  );
}

function Copyable({ value }: { value: string }) {
  const { t } = useTranslation('settings');
  return (
    <CopyButton value={value}>
      {({ copied, copy }) => (
        <Tooltip label={copied ? t('technical.copied') : t('technical.copy')}>
          <ActionIcon variant="subtle" color="gray" size={42} my={-10} onClick={copy} aria-label={t('technical.copyValue', { value })}>
            {copied ? <IconCheck size={14} stroke={1.6} /> : <IconCopy size={14} stroke={1.6} />}
          </ActionIcon>
        </Tooltip>
      )}
    </CopyButton>
  );
}

function DeviceData() {
  const { t } = useTranslation('settings');
  const sys = useSystem();
  if (!sys.data) return sys.error ? <Alert color="red">{saveError(sys.error)}</Alert> : <Skeleton height={300} radius="lg" />;
  const s = sys.data;
  const d = s.device;
  const blank = (v: string) => v || t('technical.notImported');
  const rows: [string, string, boolean?][] = [
    [t('technical.rows.type'), d.model || t('technical.notImported')],
    [t('technical.rows.article'), blank(d.article), !!d.article],
    [t('technical.rows.serial'), blank(d.serial), !!d.serial],
    [t('technical.rows.production'), blank(d.production_code), !!d.production_code],
    [t('technical.rows.mac'), s.mac, true],
    [t('technical.rows.ip'), s.ip ?? t('technical.none'), !!s.ip],
    [t('technical.rows.firmware'), `openKWL ${s.firmware_version}`, true],
    [t('technical.rows.uptime'), duration(s.uptime_s)],
    [t('technical.rows.resetReason'), t(`technical.reset.${s.reset_reason}`)],
    [t('technical.rows.clockSync'), s.last_time_sync ? s.last_time_sync.replace('T', ' ') : t('technical.notYet')],
    [t('technical.rows.sdCard'), s.sd_card ? t('technical.ready') : t('technical.missing')],
    [t('technical.rows.homeAssistant'), s.mqtt_connected ? t('technical.connected') : t('technical.notConnected')],
  ];
  return (
    <Card padding={0}>
      <Title order={2} px={{ base: 'md', sm: 'lg' }} pt="md" pb="xs">
        {t('technical.deviceData')}
      </Title>
      <Table highlightOnHover={false}>
        <Table.Tbody>
          {rows.map(([k, v, id]) => (
            <Table.Tr key={k}>
              <Table.Td c="dimmed" pl={{ base: 'md', sm: 'lg' }}>
                {k}
              </Table.Td>
              <Table.Td ta="right" ff={id ? 'monospace' : undefined} pr={{ base: 'md', sm: 'lg' }}>
                <Group gap={4} justify="flex-end" wrap="nowrap">
                  <span style={{ overflowWrap: 'anywhere' }}>{v}</span>
                  {id && <Copyable value={v} />}
                </Group>
              </Table.Td>
            </Table.Tr>
          ))}
        </Table.Tbody>
      </Table>
    </Card>
  );
}

/** One figure with its label (KWL ReadingTile, inline variant). */
function Figure({ label, value, footnote }: { label: string; value: string; footnote?: string }) {
  const [n, u] = splitUnit(value);
  return (
    <div>
      <Text size="xs" c="dimmed">
        {label}
      </Text>
      <Text className="kwl-figure" component="div">
        {n}
        {u && <span className="kwl-unit">{u}</span>}
      </Text>
      {footnote && (
        <Text size="xs" c="var(--mantine-color-placeholder)">
          {footnote}
        </Text>
      )}
    </div>
  );
}

function LiveValues({ status: s }: { status: Status }) {
  const { t } = useTranslation('settings');
  const frost = s.frost.defrost ? t('technical.live.defrosting') : s.frost.active ? t('technical.live.active') : t('technical.live.notNeeded');
  return (
    <Card p={{ base: 'md', sm: 'lg' }}>
      <Title order={2} mb="md">
        {t('technical.live.title')}
      </Title>
      <SimpleGrid cols={{ base: 2, sm: 3 }} spacing="lg">
        <Figure label={t('technical.live.bypass')} value={s.bypass.moving ? t('technical.live.moving') : s.bypass.open ? t('technical.live.open') : t('technical.live.closed')} />
        <Figure label={t('technical.live.frost')} value={frost} />
        <Figure label={t('technical.live.supplyFan')} value={reading(s.supply_fan.rpm, 'fanSpeed')} footnote={reading(s.supply_fan.volts, 'voltage')} />
        <Figure label={t('technical.live.exhaustFan')} value={reading(s.exhaust_fan.rpm, 'fanSpeed')} footnote={reading(s.exhaust_fan.volts, 'voltage')} />
        <Figure label={t('technical.live.heat')} value={reading(s.heat_recovery.heating_kwh, 'energy')} />
        <Figure label={t('technical.live.cooling')} value={reading(s.heat_recovery.cooling_kwh, 'energy')} />
        <Figure label={t('technical.live.hours')} value={reading(s.operating_hours, 'hours')} />
      </SimpleGrid>
    </Card>
  );
}

function Log() {
  const { t } = useTranslation('settings');
  const [text, setText] = useState<string | null>(null);
  const loading = useAction();
  const load = () => loading.run(async () => setText(await api.log()), t('technical.log.failed'));
  return (
    <Card p={{ base: 'md', sm: 'lg' }}>
      <Group justify="space-between">
        <Title order={2}>{t('technical.log.title')}</Title>
        <Button variant="subtle" onClick={text === null ? load : () => setText(null)} loading={loading.busy}>
          {text === null ? t('technical.log.show') : t('technical.log.hide')}
        </Button>
      </Group>
      <Box mt={loading.error ? 'md' : 0}>
        <ActionError action={loading} />
      </Box>
      {text !== null && (
        <Code block mah={320} mt="md" style={{ overflow: 'auto' }}>
          {text.trim() || t('technical.log.empty')}
        </Code>
      )}
    </Card>
  );
}
