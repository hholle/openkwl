import { ActionIcon, Alert, Anchor, Group, Stack, Text, Tooltip, UnstyledButton } from '@mantine/core';
import { IconAlertTriangle, IconChevronDown, IconChevronUp, IconCircleX, IconSnowflake } from '@tabler/icons-react';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { Condition } from '../lib/faults';
import { link } from '../lib/router';
import { TAP } from '../lib/tap';

const ICON = {
  red: <IconCircleX size={20} stroke={1.6} />,
  yellow: <IconAlertTriangle size={20} stroke={1.6} />,
  blue: <IconSnowflake size={20} stroke={1.6} />,
};

/**
 * Overview's alert stack, in StatusBadge priority order. Red and yellow alerts stay until the
 * unit clears them; blue ones (automatic behaviour) fold to one line. From three alerts on,
 * the first two show and "2 more ›" opens the rest.
 */
export function FaultAlerts({ items }: { items: Condition[] }) {
  const { t } = useTranslation('faults');
  const [all, setAll] = useState(false);
  const alerts = items.filter((c) => c.title);
  if (alerts.length === 0) return null;
  const shown = all || alerts.length < 3 ? alerts : alerts.slice(0, 2);
  return (
    <Stack gap="sm" id="alerts" style={{ scrollMarginTop: 72 }}>
      {shown.map((c) => (
        <FaultAlert key={c.key} c={c} />
      ))}
      {shown.length < alerts.length && (
        <UnstyledButton onClick={() => setAll(true)} mih={42} fz="sm" fw={500} c="var(--app-brand-text)">
          {t('more', { n: alerts.length - shown.length })}
        </UnstyledButton>
      )}
    </Stack>
  );
}

function FaultAlert({ c }: { c: Condition }) {
  const { t } = useTranslation('faults');
  const [folded, setFolded] = useState(false);
  const foldable = c.color === 'blue';
  const title = foldable ? (
    <Group justify="space-between" wrap="nowrap" gap="xs">
      <span>{c.title}</span>
      <Tooltip label={folded ? t('showMore') : t('showLess')}>
        <ActionIcon variant="subtle" color="blue" size={42} my={-12} onClick={() => setFolded(!folded)} aria-label={folded ? t('showMore') : t('showLess')} aria-expanded={!folded}>
          {folded ? <IconChevronDown size={16} stroke={1.6} /> : <IconChevronUp size={16} stroke={1.6} />}
        </ActionIcon>
      </Tooltip>
    </Group>
  ) : (
    c.title
  );
  return (
    <Alert color={c.color} title={title} icon={ICON[c.color]} styles={foldable ? { label: { flex: 1 } } : undefined}>
      {!folded && (
        <Text size="md">
          {c.body}
          {c.action && (
            <>
              {' '}
              <Anchor {...link(c.action.to)} style={TAP} fw={600} c={`var(--app-${c.color === 'red' ? 'danger' : c.color === 'yellow' ? 'warning' : 'info'})`}>
                {c.action.label}
              </Anchor>
            </>
          )}
        </Text>
      )}
    </Alert>
  );
}
