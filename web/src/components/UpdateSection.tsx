import { Alert, Button, FileButton, Group, Progress, Stack, Text } from '@mantine/core';
import { useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ApiError, api } from '../api/client';
import { number } from '../i18n/format';
import { useSystem } from '../lib/resources';
import { INSTALL_S, RestartModal } from '../pages/Settings';
import { ConfirmModal } from './ConfirmModal';
import { SettingsSection } from './SettingsSection';
import { t } from '../i18n';

// Firmware update: one confirmation, then everything runs by itself. The unit checks the
// update package (`openkwl-pack bundle`, .okwl) while it arrives and arms the bootloader only if
// everything is right; then it restarts at once and the bootloader installs it.

type Stage = { kind: 'idle' } | { kind: 'uploading'; share: number } | { kind: 'ready' } | { kind: 'failed'; message: string };

function reason(e: unknown): string {
  if (e instanceof ApiError) {
    if (e.status === 0) return e.describe();
    // An armed, not yet installed update (the unit refuses a second one until the restart).
    if (e.status === 409 && e.body?.message?.includes('restart')) return t('update:armed');
    // The unit's own message says what was wrong with the file.
    if (e.body?.message && e.status !== 401) return `${e.body.message.charAt(0).toUpperCase()}${e.body.message.slice(1)}.`;
    return e.describe();
  }
  return String(e);
}

export function UpdateSection() {
  const { t } = useTranslation('update');
  const sys = useSystem();
  const [stage, setStage] = useState<Stage>({ kind: 'idle' });
  const [file, setFile] = useState<File | null>(null);
  // The restart dialog; `auto` right after a successful upload (already confirmed).
  const [install, setInstall] = useState<{ auto: boolean } | null>(null);
  // Lets the same file be chosen again after a failure.
  const resetFile = useRef<() => void>(null);

  async function upload(f: File) {
    setFile(null);
    setStage({ kind: 'uploading', share: 0 });
    try {
      await api.uploadUpdate(f, (share) => setStage({ kind: 'uploading', share }));
      setStage({ kind: 'ready' });
      setInstall({ auto: true });
    } catch (e) {
      setStage({ kind: 'failed', message: reason(e) });
    } finally {
      resetFile.current?.();
    }
  }

  const uploading = stage.kind === 'uploading';
  return (
    <SettingsSection
      title={t('title')}
      intro={t('intro')}
    >
      <Stack gap="md">
        <Text size="sm">
          {t('installed')} <span className="tabular">openKWL {sys.data?.firmware_version ?? '—'}</span>
        </Text>
        {uploading && (
          <Stack gap={4}>
            <Text size="sm">{stage.share < 1 ? t('uploading', { pct: Math.round(stage.share * 100) }) : t('checking')}</Text>
            <Progress value={stage.share * 100} aria-label={t('progress')} />
          </Stack>
        )}
        {stage.kind === 'failed' && (
          <Alert color="red" title={t('failedTitle')}>
            {stage.message} {t('nothingChanged')}
          </Alert>
        )}
        {stage.kind === 'ready' ? (
          // Only seen if the automatic restart didn't happen (it failed, or the dialog was closed).
          <Alert color="green" title={t('readyTitle')}>
            <Stack gap="sm">
              <Text size="sm">{t('readyText')}</Text>
              <Group>
                <Button onClick={() => setInstall({ auto: false })}>{t('installNow')}</Button>
              </Group>
            </Stack>
          </Alert>
        ) : (
          <Group>
            <FileButton onChange={setFile} accept=".okwl" disabled={uploading} resetRef={resetFile}>
              {(props) => (
                <Button variant="default" loading={uploading} {...props}>
                  {t('choose')}
                </Button>
              )}
            </FileButton>
          </Group>
        )}
      </Stack>
      <ConfirmModal
        opened={file !== null}
        title={t('askTitle')}
        confirmLabel={t('askConfirm')}
        summary={file ? [[t('file'), file.name], [t('size'), `${number(file.size / 1_000_000, 1)} MB`]] : undefined}
        onConfirm={() => (file ? upload(file) : undefined)}
        onClose={() => {
          setFile(null);
          resetFile.current?.();
        }}
      >
        {t('askText', { s: INSTALL_S })}
      </ConfirmModal>
      <RestartModal opened={install !== null} onClose={() => setInstall(null)} install confirmed={install?.auto ?? false} />
    </SettingsSection>
  );
}
