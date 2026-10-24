import { AppShell, Box, Center, Loader } from '@mantine/core';
import { useMediaQuery } from '@mantine/hooks';
import { lazy, Suspense, useEffect, useState } from 'react';
import { api, hasToken, onAuthChange, type Status } from './api/client';
import { AppHeader, type Attention, TabBar, useTabBar } from './components/AppNav';
import { usePoll } from './lib/hooks';
import { useMqtt, useSystem } from './lib/resources';
import { parseRoute, usePath } from './lib/router';
import { t } from './i18n';
import { documentTitle } from './lib/title';
import { Login } from './pages/Login';
import { HistoryPage } from './pages/History';
import { Overview } from './pages/Overview';
import { SchedulePage } from './pages/Schedule';
import { SettingsPage } from './pages/Settings';
import { Setup } from './pages/Setup';

/** /design: the component state gallery, only in `pnpm dev` (dropped from the build). */
const DesignPreview = import.meta.env.DEV ? lazy(() => import('./pages/DesignPreview').then((m) => ({ default: m.DesignPreview }))) : null;

/** Status poll interval (see AGENTS.md, "interactive"). */
const STATUS_POLL_MS = 1000;
/** Without an answer for this long the unit counts as offline. */
const OFFLINE_AFTER_MS = 10_000;

type Gate = 'loading' | 'setup' | 'login' | 'app';

export function App() {
  const [gate, setGate] = useState<Gate>('loading');
  const isPhone = !useMediaQuery('(min-width: 48em)', false, { getInitialValueInEffect: false });

  useEffect(() => {
    const decide = () =>
      api
        .setupStatus()
        .then((s) => setGate(s.setup_required ? 'setup' : hasToken() ? 'app' : 'login'))
        .catch(() => setGate(hasToken() ? 'app' : 'login'));
    void decide();
    // A new token moves from the login screen to the app; the setup wizard continues on its
    // own after setting the password. Losing the token always goes back to the login.
    const off = onAuthChange(() => setGate((g) => (!hasToken() ? 'login' : g === 'login' ? 'app' : g)));
    return () => {
      off();
    };
  }, []);

  useEffect(() => {
    if (gate === 'login' || gate === 'setup') document.title = documentTitle(gate === 'login' ? t('app:title.login') : t('app:title.setup'));
  }, [gate]);

  if (DesignPreview && window.location.pathname === '/design') {
    return (
      <Suspense>
        <DesignPreview />
      </Suspense>
    );
  }

  return (
    <>
      {gate === 'loading' ? (
        <Center h="60vh">
          <Loader />
        </Center>
      ) : gate === 'setup' ? (
        <Setup onDone={() => setGate('app')} />
      ) : gate === 'login' ? (
        <Login />
      ) : (
        <Shell isPhone={isPhone} />
      )}
    </>
  );
}

/** Offline once the status poll has failed for 10 s (or never answered within 10 s). */
function useOffline(error: unknown, updatedAt: number | null): boolean {
  const [since] = useState(Date.now);
  return error !== null && Date.now() - (updatedAt ?? since) > OFFLINE_AFTER_MS;
}

/** The pages that need a look, from the same data as their row badges. */
function useAttention(status: Status | null): Attention {
  const system = useSystem();
  const mqtt = useMqtt();
  const has = (f: Status['faults'][number]) => status?.faults.includes(f) ?? false;
  const haFailing = mqtt.data?.enabled === true && system.data?.mqtt_connected === false;
  return {
    schedule: has('time_not_set'),
    settings: (status?.filter.due ?? false) || has('sd_card') || has('config_invalid') || haFailing,
  };
}

function Shell({ isPhone }: { isPhone: boolean }) {
  const route = parseRoute(usePath());
  const tabBar = useTabBar();
  // Once a second while the tab is visible: the unit updates its state at 1 Hz, so changes
  // (mode, level, bypass, faults) show within a second.
  const status = usePoll(api.status, STATUS_POLL_MS);
  const offline = useOffline(status.error, status.updatedAt);
  const attention = useAttention(status.data);

  useEffect(() => {
    if (route.page === 'overview') document.title = documentTitle(t('app:nav.overview'));
  }, [route.page]);

  return (
    <AppShell
      header={{ height: { base: 52, sm: 56 } }}
      footer={{ height: 'calc(64px + env(safe-area-inset-bottom))', collapsed: !tabBar }}
      padding={{ base: 'md', sm: 'xl' }}
    >
      <AppShell.Header>
        <AppHeader page={route.page} status={status.data} offline={offline} attention={attention} isPhone={isPhone} tabs={!tabBar} />
      </AppShell.Header>

      <AppShell.Main className="app-main">
        <Box maw={1040} mx="auto">
          {route.page === 'schedule' ? (
            <SchedulePage section={route.section} status={status.data} />
          ) : route.page === 'history' ? (
            <HistoryPage view={route.section} status={status.data} />
          ) : route.page === 'settings' ? (
            <SettingsPage section={route.section} status={status.data} onChanged={status.refresh} />
          ) : (
            <Overview status={status.data} offline={offline} onChanged={status.refresh} />
          )}
        </Box>
      </AppShell.Main>

      {tabBar && (
        <AppShell.Footer pb="env(safe-area-inset-bottom)">
          <TabBar page={route.page} attention={attention} />
        </AppShell.Footer>
      )}
    </AppShell>
  );
}
