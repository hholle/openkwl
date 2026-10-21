import { Alert, Box, Button, Card, Collapse, Group, Skeleton, Stack, Text, Title } from '@mantine/core';
import { useMediaQuery } from '@mantine/hooks';
import { IconChevronDown, IconChevronUp } from '@tabler/icons-react';
import { type ReactNode, useEffect, useState } from 'react';
import type { ApiError } from '../api/client';
import { api, type ConfigKey, type ConfigTypes } from '../api/client';
import { useConfig } from '../lib/resources';
import { setLeaveGuard } from '../lib/router';
import { useTabBar } from './AppNav';
import { ConfirmModal } from './ConfirmModal';
import { DoneNote, saveError, useFlash } from './Feedback';
import { t } from '../i18n';

export { saveError };

export interface FormState {
  dirty: boolean;
  saving: boolean;
  error: string | null;
  /** Just saved: the bar says so for a moment instead of a toast. */
  saved?: boolean;
  save: () => Promise<void>;
  discard: () => void;
}

/**
 * Editing one config object: a draft on top of the shared copy. Saving replaces the shared
 * copy, so row summaries and the Overview update at once. `beforeSave` may veto (the plug modal).
 */
export function useConfigForm<K extends ConfigKey>(key: K, beforeSave?: (next: ConfigTypes[K], saved: ConfigTypes[K]) => Promise<boolean>) {
  const res = useConfig(key);
  const [draft, setDraft] = useState<ConfigTypes[K] | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, flash] = useFlash();
  const value = draft ?? res.data ?? null;
  const dirty = draft !== null && res.data !== undefined && JSON.stringify(draft) !== JSON.stringify(res.data);

  async function save() {
    if (!draft || !res.data) return;
    if (beforeSave && !(await beforeSave(draft, res.data))) return;
    setSaving(true);
    setError(null);
    try {
      res.set(await api.put(key, draft));
      setDraft(null);
      flash();
    } catch (e) {
      setError(saveError(e));
    } finally {
      setSaving(false);
    }
  }

  const form: FormState = {
    dirty,
    saving,
    error,
    saved,
    save,
    discard: () => {
      setDraft(null);
      setError(null);
    },
  };
  return { value, set: (v: ConfigTypes[K]) => setDraft(v), loadError: res.error, form };
}

/** Asks before leaving a section with unsaved changes. */
export function useLeaveGuard(dirty: boolean, title: string) {
  const [pending, setPending] = useState<(() => void) | null>(null);
  useEffect(() => {
    if (!dirty) return;
    setLeaveGuard((go) => setPending(() => go));
    const onUnload = (e: BeforeUnloadEvent) => e.preventDefault();
    window.addEventListener('beforeunload', onUnload);
    return () => {
      setLeaveGuard(null);
      window.removeEventListener('beforeunload', onUnload);
    };
  }, [dirty]);
  return (
    <ConfirmModal
      opened={pending !== null}
      title={t('discardTitle', { section: title })}
      confirmLabel={t('discardConfirm')}
      onConfirm={() => {
        const go = pending;
        setPending(null);
        setLeaveGuard(null);
        go?.();
      }}
      onClose={() => setPending(null)}
    />
  );
}

/**
 * Save bar: sticks to the bottom of the screen (above the tab bar) while there are changes, and
 * says "Saved" for a moment after saving; hidden otherwise.
 */
export function SaveBar({ form, saveLabel = t('save'), discardLabel = t('discard') }: { form: FormState; saveLabel?: string; discardLabel?: string }) {
  const tabBar = useTabBar();
  if (!form.dirty && !form.saved) return null;
  return (
    <Box
      role="region"
      aria-label={form.dirty ? t('unsavedChanges') : t('saved')}
      pos="sticky"
      bottom={tabBar ? 'calc(64px + env(safe-area-inset-bottom))' : 0}
      mx="calc(var(--card-padding) * -1)"
      mb="calc(var(--card-padding) * -1)"
      mt="md"
      px="var(--card-padding)"
      py="sm"
      bg="var(--mantine-color-body)"
      style={{ borderTop: '1px solid var(--mantine-color-default-border)', zIndex: 3, borderRadius: '0 0 var(--mantine-radius-lg) var(--mantine-radius-lg)' }}
    >
      {/* The same height in both states, so nothing moves when the buttons go. */}
      <Group justify="space-between" gap="sm" mih={36}>
        {form.dirty ? (
          <>
            <Text size="sm" fw={500} c={form.error ? 'var(--app-danger)' : undefined} style={{ flex: '1 1 160px' }}>
              {form.error ?? t('unsavedChanges')}
            </Text>
            <Group gap="xs" wrap="nowrap">
              <Button variant="subtle" color="gray" onClick={form.discard} disabled={form.saving}>
                {discardLabel}
              </Button>
              <Button onClick={() => void form.save()} loading={form.saving}>
                {saveLabel}
              </Button>
            </Group>
          </>
        ) : (
          <DoneNote>{t('saved')}</DoneNote>
        )}
      </Group>
    </Box>
  );
}

const ADV_KEY = 'kwl.advanced';
function readAdvanced(id: string): boolean {
  try {
    return (JSON.parse(localStorage.getItem(ADV_KEY) ?? '[]') as string[]).includes(id);
  } catch {
    return false;
  }
}
function writeAdvanced(id: string, open: boolean) {
  try {
    const all = new Set(JSON.parse(localStorage.getItem(ADV_KEY) ?? '[]') as string[]);
    if (open) all.add(id);
    else all.delete(id);
    localStorage.setItem(ADV_KEY, JSON.stringify([...all]));
  } catch {
    // Private window: the disclosure starts closed next time.
  }
}

/**
 * KWL SettingsSection: one topic as a card (max 640px) with its title (desktop; on phones the
 * page title names it), an intro, the fields, an optional "Advanced" disclosure and the save bar.
 */
export function SettingsSection({
  title,
  intro,
  children,
  advanced,
  form,
  loading = false,
  loadError,
  saveLabel,
  discardLabel,
  footer,
}: {
  title: string;
  intro?: ReactNode;
  children?: ReactNode;
  advanced?: ReactNode;
  form?: FormState;
  loading?: boolean;
  loadError?: ApiError | null;
  saveLabel?: string;
  discardLabel?: string;
  /** Content below the fields that isn't saved with them (buttons with their own action). */
  footer?: ReactNode;
}) {
  const wide = useMediaQuery('(min-width: 62em)', true, { getInitialValueInEffect: false });
  const [adv, setAdv] = useState(() => readAdvanced(title));
  const guard = useLeaveGuard(form?.dirty ?? false, title);
  return (
    <Card maw={640} component="section" aria-label={title} p={{ base: 'md', sm: 'lg' }} style={{ overflow: 'visible' }}>
      <Stack gap="md">
        {(wide || intro) && (
          <div>
            {wide && <Title order={2}>{title}</Title>}
            {intro && (
              <Text c="dimmed" mt={wide ? 4 : 0}>
                {intro}
              </Text>
            )}
          </div>
        )}
        {loadError && !loading ? (
          <Alert color="red" title={t('loadFailed')}>
            {saveError(loadError)}
          </Alert>
        ) : loading ? (
          <Skeleton height={160} />
        ) : (
          <>
            {children}
            {advanced && (
              <div>
                <Button
                  variant="subtle"
                  color="gray"
                  px={0}
                  rightSection={adv ? <IconChevronUp size={16} stroke={1.6} /> : <IconChevronDown size={16} stroke={1.6} />}
                  onClick={() => {
                    writeAdvanced(title, !adv);
                    setAdv(!adv);
                  }}
                  aria-expanded={adv}
                >
                  {adv ? t('hideAdvanced') : t('showAdvanced')}
                </Button>
                <Collapse expanded={adv}>
                  <Stack gap="md" pt="sm">
                    <Text size="xs" c="var(--mantine-color-placeholder)">
                      {t('advancedNote')}
                    </Text>
                    {advanced}
                  </Stack>
                </Collapse>
              </div>
            )}
            {footer}
          </>
        )}
      </Stack>
      {form && <SaveBar form={form} saveLabel={saveLabel} discardLabel={discardLabel} />}
      {guard}
    </Card>
  );
}
