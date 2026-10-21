import { Alert, Button, Card, Checkbox, PasswordInput, Stack, Title } from '@mantine/core';
import { type FormEvent, useEffect, useRef, useState } from 'react';
import { ApiError, api, setToken } from '../api/client';
import { Centered } from '../components/Centered';
import { CHECKBOX_TAP, PASSWORD_TAP } from '../lib/tap';
import { t } from '../i18n';

type Problem = { kind: 'wrong' } | { kind: 'limited'; until: number } | { kind: 'unreachable' } | { kind: 'other'; text: string };

/** KWL Login: one password, "keep me logged in" ticked by default. */
export function Login() {
  const [password, setPassword] = useState('');
  const [remember, setRemember] = useState(true);
  const [empty, setEmpty] = useState(false);
  const [problem, setProblem] = useState<Problem | null>(null);
  const [busy, setBusy] = useState(false);
  const [now, setNow] = useState(Date.now());
  const field = useRef<HTMLInputElement>(null);
  const waitS = problem?.kind === 'limited' ? Math.max(0, Math.ceil((problem.until - now) / 1000)) : 0;

  useEffect(() => {
    if (problem?.kind !== 'limited') return;
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [problem]);

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!password) {
      setEmpty(true);
      field.current?.focus();
      return;
    }
    setBusy(true);
    try {
      const r = await api.login(password, remember);
      setToken(r.token, remember);
    } catch (err) {
      if (err instanceof ApiError && err.status === 401) {
        setProblem({ kind: 'wrong' });
        setPassword('');
        field.current?.focus();
      } else if (err instanceof ApiError && err.status === 429) {
        setNow(Date.now());
        setProblem({ kind: 'limited', until: Date.now() + (err.body?.retry_after_s ?? 60) * 1000 });
      } else if (err instanceof ApiError && err.status === 0) {
        setProblem({ kind: 'unreachable' });
      } else {
        setProblem({ kind: 'other', text: err instanceof ApiError ? err.describe() : String(err) });
      }
    } finally {
      setBusy(false);
    }
  }

  return (
    <Centered width={360}>
      <Card component="form" onSubmit={submit} noValidate>
        <Stack>
          <Title order={1}>{t('app:login.title')}</Title>
          {problem?.kind === 'wrong' && <Alert color="red" title={t('app:login.wrong')} />}
          {problem?.kind === 'limited' && waitS > 0 && <Alert color="yellow" title={t('app:login.limited')} />}
          {problem?.kind === 'unreachable' && <Alert color="red" title={t('app:login.unreachable')} />}
          {problem?.kind === 'other' && <Alert color="red">{problem.text}</Alert>}
          <PasswordInput {...PASSWORD_TAP}
            ref={field}
            label={t('app:login.password')}
            size="md"
            value={password}
            error={empty && !password ? t('app:login.enterPassword') : null}
            onChange={(e) => {
              setPassword(e.currentTarget.value);
              setEmpty(false);
            }}
            autoFocus
            autoComplete="current-password"
          />
          <Checkbox
            styles={CHECKBOX_TAP}
            label={t('app:login.remember')}
            description={t('app:login.rememberHint')}
            checked={remember}
            onChange={(e) => setRemember(e.currentTarget.checked)}
          />
          <Button type="submit" size="md" fullWidth loading={busy} disabled={waitS > 0}>
            {waitS > 0 ? t('app:login.tryAgainIn', { s: waitS }) : t('app:login.submit')}
          </Button>
        </Stack>
      </Card>
    </Centered>
  );
}
