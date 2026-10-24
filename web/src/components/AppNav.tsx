import { Anchor, Box, Group, SimpleGrid, Text, UnstyledButton, VisuallyHidden } from '@mantine/core';
import { useHover, useMediaQuery } from '@mantine/hooks';
import { IconCalendarTime, IconChartLine, IconSettings, IconWind } from '@tabler/icons-react';
import type { ComponentType } from 'react';
import type { Status } from '../api/client';
import { link, type Page, pathOf } from '../lib/router';
import { StatusBadge } from './StatusBadge';
import { Wordmark } from './Wordmark';
import { t } from '../i18n';

const PAGES: { page: Page; label: () => string; icon: ComponentType<{ size?: number; stroke?: number }> }[] = [
  { page: 'overview', label: () => t('app:nav.overview'), icon: IconWind },
  { page: 'schedule', label: () => t('app:nav.schedule'), icon: IconCalendarTime },
  { page: 'history', label: () => t('app:nav.history'), icon: IconChartLine },
  { page: 'settings', label: () => t('app:nav.settings'), icon: IconSettings },
];

/**
 * The bottom tab bar is only for screens too narrow for the header tabs (below 640px, where the
 * wordmark, four tabs (German: Übersicht, Zeitplan, Verlauf, Einstellungen) and a long status
 * badge don't fit in one line).
 */
export function useTabBar(): boolean {
  return useMediaQuery('(max-width: 39.99em)', false, { getInitialValueInEffect: false });
}

/** Pages that need a look: a small brand-coloured dot on their tab (never for blue, automatic states). */
export type Attention = Partial<Record<Page, boolean>>;

function Dot({ label }: { label: string }) {
  return (
    <>
      <Box
        component="span"
        w={8}
        h={8}
        bg="var(--app-brand)"
        style={{ borderRadius: 999, boxShadow: '0 0 0 2px var(--mantine-color-body)', display: 'inline-block' }}
      />
      <VisuallyHidden>{t('app:nav.attention', { page: label })}</VisuallyHidden>
    </>
  );
}

/** Header (56px desktop, 52px phone): wordmark, the four tabs (unless the tab bar shows), clock from `sm` and StatusBadge. */
export function AppHeader({
  page,
  status,
  offline,
  attention,
  isPhone,
  tabs,
}: {
  page: Page;
  status: Status | null;
  offline: boolean;
  attention: Attention;
  isPhone: boolean;
  tabs: boolean;
}) {
  return (
    <Group h="100%" px={{ base: 'md', sm: 'xl' }} justify="space-between" wrap="nowrap">
      <Group gap={0} h="100%" wrap="nowrap">
        <Anchor {...link('/')} aria-label={`openKWL, ${t('app:nav.overview')}`} display="flex" py={11}>
          <Wordmark height={20} />
        </Anchor>
        {tabs && (
          <Group component="nav" aria-label={t('app:nav.pages')} gap={isPhone ? 0 : 4} ml={isPhone ? 'xs' : 'lg'} h="100%" wrap="nowrap">
            {PAGES.map((p) => (
              <HeaderTab key={p.page} label={p.label()} to={pathOf(p.page)} active={page === p.page} dot={attention[p.page] === true} compact={isPhone} />
            ))}
          </Group>
        )}
      </Group>
      <Group gap="md" wrap="nowrap">
        {!isPhone && status?.time && (
          <Text size="md" c="dimmed" className="tabular" title={status.time.slice(0, 10)}>
            {status.time.slice(11, 16)}
          </Text>
        )}
        <StatusBadge status={status} offline={offline} />
      </Group>
    </Group>
  );
}

function HeaderTab({ label, to, active, dot, compact }: { label: string; to: string; active: boolean; dot: boolean; compact: boolean }) {
  const { hovered, ref } = useHover<HTMLAnchorElement>();
  const line = active ? 'var(--app-brand)' : hovered ? 'var(--mantine-color-placeholder)' : 'transparent';
  return (
    <UnstyledButton
      ref={ref}
      component="a"
      {...link(to)}
      aria-current={active ? 'page' : undefined}
      h="100%"
      px={compact ? 8 : 'sm'}
      fz="md"
      fw={active ? 600 : 500}
      c={active ? undefined : 'dimmed'}
      style={{ display: 'flex', alignItems: 'center', gap: 6, borderBottom: `2px solid ${line}`, borderTop: '2px solid transparent' }}
    >
      {label}
      {dot && <Dot label={label} />}
    </UnstyledButton>
  );
}

/** Phone tab bar (64px + safe area): icon over label, brand-coloured icon on the active page. */
export function TabBar({ page, attention }: { page: Page; attention: Attention }) {
  return (
    <SimpleGrid component="nav" aria-label={t('app:nav.pages')} cols={PAGES.length} spacing={0} h={64}>
      {PAGES.map(({ page: p, label: labelOf, icon: Icon }) => {
        const label = labelOf();
        const active = page === p;
        return (
          <UnstyledButton
            key={p}
            component="a"
            {...link(pathOf(p))}
            aria-current={active ? 'page' : undefined}
            h={64}
            style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', gap: 4 }}
          >
            <Box pos="relative" c={active ? 'var(--app-brand)' : 'dimmed'} h={24}>
              <Icon size={24} stroke={1.6} />
              {attention[p] && (
                <Box pos="absolute" top={-2} right={-8} lh={0}>
                  <Dot label={label} />
                </Box>
              )}
            </Box>
            <Text size="xs" fw={active ? 600 : 500} c={active ? undefined : 'dimmed'} lh={1}>
              {label}
            </Text>
          </UnstyledButton>
        );
      })}
    </SimpleGrid>
  );
}
