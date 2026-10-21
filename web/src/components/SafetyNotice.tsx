import { Alert, Button, Code, Group, Stack, Text } from '@mantine/core';
import { IconShieldCheck } from '@tabler/icons-react';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';

// Once per browser: shown until "Got it", for new and existing installations alike.
const STORAGE_KEY = 'openkwl.safety-notice';

function readSeen(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) === 'seen';
  } catch {
    return false;
  }
}

function writeSeen() {
  try {
    localStorage.setItem(STORAGE_KEY, 'seen');
  } catch {
    // Private window or blocked storage: the notice comes back next time, which is harmless.
  }
}

/** The essentials of docs/INSTALL.md's Safety section, on the Overview until dismissed. */
export function SafetyNotice() {
  const { t } = useTranslation('overview');
  const [seen, setSeen] = useState(readSeen);
  if (seen) return null;
  return (
    <Alert color="brand" title={t('safety.title')} icon={<IconShieldCheck size={20} stroke={1.6} />}>
      <Stack gap="sm">
        <Text size="md">{t('safety.check')}</Text>
        <Text size="md">{t('safety.combustion')}</Text>
        <Text size="md">{t('safety.rollback')}</Text>
        <Text size="md" c="dimmed">
          {t('safety.more')} <Code>docs/INSTALL.md</Code>
        </Text>
        <Group>
          <Button
            onClick={() => {
              writeSeen();
              setSeen(true);
            }}
          >
            {t('safety.gotIt')}
          </Button>
        </Group>
      </Stack>
    </Alert>
  );
}
