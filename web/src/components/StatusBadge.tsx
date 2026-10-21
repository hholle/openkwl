import { Badge, Box, UnstyledButton } from '@mantine/core';
import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { Status } from '../api/client';
import { type BadgeColor, headline } from '../lib/faults';
import { navigate } from '../lib/router';

/** A light Badge with the leading dot (KWL Badge): the word carries the meaning, not the colour. */
export function DotBadge({ color, children }: { color: BadgeColor; children: ReactNode }) {
  return (
    <Badge color={color} leftSection={<Box w={6} h={6} bg="currentColor" style={{ borderRadius: 999 }} />}>
      {children}
    </Badge>
  );
}

/** Opens Overview at the alert stack. */
export function showAlerts() {
  navigate('/');
  requestAnimationFrame(() => document.getElementById('alerts')?.scrollIntoView({ block: 'start' }));
}

/**
 * The header pill: the most important condition and how many more ("Filter due · +1"), else
 * Running or Off. Screen readers hear changes; it never animates.
 */
export function StatusBadge({ status, offline }: { status: Status | null; offline: boolean }) {
  const { t } = useTranslation('faults');
  const h = headline(status, offline);
  return (
    <Box aria-live="polite" aria-atomic="true">
      {h && (
        <UnstyledButton onClick={showAlerts} py={10} aria-label={h.more ? t('statusLabelMore', { word: h.word, more: h.more }) : t('statusLabel', { word: h.word })}>
          <DotBadge color={h.color}>
            {h.word}
            {h.more > 0 && ` · +${h.more}`}
          </DotBadge>
        </UnstyledButton>
      )}
    </Box>
  );
}
