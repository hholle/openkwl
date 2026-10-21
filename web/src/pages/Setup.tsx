import { Alert, Badge, Button, Card, Group, PasswordInput, Stack, Table, Text, Title } from '@mantine/core';
import { type FormEvent, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ApiError, api, type Schemas, setToken } from '../api/client';
import { Centered } from '../components/Centered';
import { PROFILE_NAMES } from '../lib/format';
import { reading } from '../lib/readings';
import { navigate } from '../lib/router';
import { PASSWORD_TAP } from '../lib/tap';

const MIN_LEN = 10;
const STEPS = ['password', 'check', 'done'] as const;

/** First start (KWL Setup wizard): choose the password, check what was taken over, done. */
export function Setup({ onDone }: { onDone: () => void }) {
  const { t } = useTranslation('setup');
  const [step, setStep] = useState(0);
  const [closesAt, setClosesAt] = useState<number | null>(null);
  const [closed, setClosed] = useState(false);
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    api
      .setupStatus()
      .then((s) => s.window_remaining_s !== null && setClosesAt(Date.now() + s.window_remaining_s * 1000))
      .catch(() => undefined);
    const id = setInterval(() => setNow(Date.now()), 15_000);
    return () => clearInterval(id);
  }, []);

  if (closed) {
    return (
      <Centered width={480}>
        <Card>
          <Stack>
            <Title order={1}>{t('closed.title')}</Title>
            <Text>{t('closed.text')}</Text>
            <Group>
              <Button onClick={() => window.location.reload()}>{t('closed.reload')}</Button>
            </Group>
          </Stack>
        </Card>
      </Centered>
    );
  }

  const leftMin = closesAt !== null ? Math.max(0, Math.ceil((closesAt - now) / 60_000)) : null;
  const finish = (to: string) => {
    navigate(to, { replace: true, force: true });
    onDone();
  };
  return (
    <Centered width={480}>
      <Card>
        <Stack gap="lg">
          <Group justify="space-between" gap="xs">
            <Text size="sm" c="dimmed">
              {t('stepOf', { n: step + 1, total: STEPS.length, name: t(`steps.${STEPS[step] ?? 'done'}`) })}
            </Text>
            {step === 0 &&
              leftMin !== null &&
              (leftMin < 5 ? <Badge color="yellow">{t('closesIn', { min: leftMin })}</Badge> : <Text size="sm" c="dimmed">{t('closesIn', { min: leftMin })}</Text>)}
          </Group>
          {step === 0 && <PasswordStep onNext={() => setStep(1)} onClosed={() => setClosed(true)} />}
          {step === 1 && <ReviewStep onBack={() => setStep(0)} onNext={() => setStep(2)} />}
          {step === 2 && (
            <Stack>
              <Title order={1}>{t('ready.title')}</Title>
              <Text>{t('ready.text')}</Text>
              <Group>
                <Button onClick={() => finish('/')}>{t('ready.overview')}</Button>
                <Button variant="default" onClick={() => finish('/settings/home-assistant')}>
                  {t('ready.homeAssistant')}
                </Button>
              </Group>
            </Stack>
          )}
        </Stack>
      </Card>
    </Centered>
  );
}

function PasswordStep({ onNext, onClosed }: { onNext: () => void; onClosed: () => void }) {
  const [pw, setPw] = useState('');
  const [again, setAgain] = useState('');
  const [error, setError] = useState<string | null>(null);
  const mismatch = again.length > 0 && again !== pw;
  const { t } = useTranslation('setup');

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (pw.length < MIN_LEN || pw !== again) return;
    try {
      const r = await api.setup(pw);
      setToken(r.token);
      onNext();
    } catch (err) {
      if (err instanceof ApiError && err.status === 403) onClosed();
      // Setup answers only to the unit's own address (against DNS rebinding).
      else if (err instanceof ApiError && err.status === 400 && err.body?.message?.includes('address')) setError(t('wrongHost'));
      else setError(err instanceof ApiError ? err.describe() : String(err));
    }
  }

  return (
    <Stack component="form" onSubmit={submit}>
      <Title order={1}>{t('password.title')}</Title>
      <Text>{t('password.intro')}</Text>
      <PasswordInput {...PASSWORD_TAP}
        label={t('password.new')}
        description={t('password.minLength', { n: MIN_LEN })}
        value={pw}
        onChange={(e) => setPw(e.currentTarget.value)}
        autoComplete="new-password"
        autoFocus
      />
      <PasswordInput {...PASSWORD_TAP}
        label={t('password.repeat')}
        value={again}
        onChange={(e) => setAgain(e.currentTarget.value)}
        error={mismatch ? t('password.mismatch') : null}
        autoComplete="new-password"
      />
      {error && <Alert color="red">{error}</Alert>}
      <Group>
        <Button type="submit" disabled={pw.length < MIN_LEN || pw !== again}>
          {t('password.continue')}
        </Button>
      </Group>
    </Stack>
  );
}

function ReviewStep({ onBack, onNext }: { onBack: () => void; onNext: () => void }) {
  const [data, setData] = useState<{
    network: Schemas['NetworkConfig'];
    fans: Schemas['FanConfig'];
    schedule: Schemas['ScheduleConfig'];
    system: Schemas['SystemInfo'];
  } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { t } = useTranslation('setup');

  useEffect(() => {
    Promise.all([api.get('network'), api.get('fans'), api.get('schedule'), api.system()])
      .then(([network, fans, schedule, system]) => setData({ network, fans, schedule, system }))
      .catch((e) => setError(e instanceof ApiError ? e.describe() : String(e)));
  }, []);

  if (error) return <Alert color="red">{error}</Alert>;
  if (!data) return null;
  const { network, fans, schedule, system } = data;
  const imported = system.device.serial !== '';
  const rows: [string, string, boolean?][] = [
    [t('review.unit'), system.device.model || 'Helios KWL'],
    [t('review.network'), network.dhcp ? `${network.hostname} · ${t('review.automatic')}` : `${network.address} · ${network.hostname}`, true],
    ...fans.supply_volts.map((v, i): [string, string] => [t('review.fanVoltage', { n: i + 1 }), reading(v, 'voltage')]),
    [t('review.schedule'), PROFILE_NAMES[schedule.active_profile - 1] ?? ''],
  ];
  return (
    <Stack>
      <Title order={1}>{t('review.title')}</Title>
      <Text>
        {imported
          ? t('review.takenOver')
          : t('review.defaults')}
      </Text>
      <Table highlightOnHover={false}>
        <Table.Tbody>
          {rows.map(([k, v, mono]) => (
            <Table.Tr key={k}>
              <Table.Td c="dimmed">{k}</Table.Td>
              <Table.Td ta="right" ff={mono ? 'monospace' : undefined}>
                {v}
              </Table.Td>
            </Table.Tr>
          ))}
        </Table.Tbody>
      </Table>
      <Text size="xs" c="dimmed">
        {t('review.later')}
      </Text>
      <Group justify="space-between">
        <Button variant="subtle" color="gray" onClick={onBack}>
          {t('review.back')}
        </Button>
        <Button onClick={onNext}>{t('review.confirm')}</Button>
      </Group>
    </Stack>
  );
}
