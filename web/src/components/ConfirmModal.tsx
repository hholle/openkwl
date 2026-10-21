import { Box, Button, Group, Modal, Stack, Text, Title } from '@mantine/core';
import { type ReactNode, useState } from 'react';
import { t } from '../i18n';

/**
 * KWL ConfirmModal: the title asks the question, one sentence names the consequence, the
 * optional summary block lists what changes. Cancel (subtle) sits left of the confirm.
 * While `running` is set the modal can't be closed and shows that content instead.
 */
export function ConfirmModal({
  opened,
  title,
  children,
  summary,
  confirmLabel,
  danger = false,
  confirmDisabled = false,
  running,
  onConfirm,
  onClose,
}: {
  opened: boolean;
  title: string;
  children?: ReactNode;
  summary?: [string, ReactNode][];
  confirmLabel: string;
  danger?: boolean;
  confirmDisabled?: boolean;
  running?: ReactNode;
  onConfirm: () => Promise<void> | void;
  onClose: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const locked = running !== undefined && running !== null;
  async function confirm() {
    setBusy(true);
    try {
      await onConfirm();
    } finally {
      setBusy(false);
    }
  }
  return (
    <Modal
      opened={opened}
      onClose={() => !locked && !busy && onClose()}
      withCloseButton={!locked}
      closeOnClickOutside={!locked}
      closeOnEscape={!locked}
      title={<Title order={3}>{title}</Title>}
    >
      {locked ? (
        running
      ) : (
        <Stack gap="md">
          {children && <Box>{typeof children === 'string' ? <Text>{children}</Text> : children}</Box>}
          {summary && (
            <Box bg="var(--app-surface-2)" p="sm" style={{ borderRadius: 'var(--mantine-radius-md)' }}>
              {summary.map(([k, v]) => (
                <Group key={k} justify="space-between" gap="md">
                  <Text size="sm" c="dimmed">
                    {k}
                  </Text>
                  <Text size="sm" className="tabular">
                    {v}
                  </Text>
                </Group>
              ))}
            </Box>
          )}
          <Group justify="space-between">
            <Button variant="subtle" color="gray" onClick={onClose} disabled={busy}>
              {t('cancel')}
            </Button>
            <Button color={danger ? 'red' : undefined} loading={busy} disabled={confirmDisabled} onClick={confirm}>
              {confirmLabel}
            </Button>
          </Group>
        </Stack>
      )}
    </Modal>
  );
}
