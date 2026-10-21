import { Alert, Group, Text } from '@mantine/core';
import { IconAlertTriangle, IconCheck } from '@tabler/icons-react';
import { useEffect, useRef, useState } from 'react';
import { ApiError } from '../api/client';
import { t } from '../i18n';

// Feedback where the action happened, instead of toasts: a button shows that it's busy, a short
// "✓ Done" next to it afterwards, and a failure stays as an alert in the same place until the
// next try or until it's closed.

const DONE_MS = 2500;

/** A save that failed: the API's reason, or the offline sentence when the unit didn't answer. */
export function saveError(e: unknown): string {
  if (e instanceof ApiError && e.status === 0) return t('error.keptOffline');
  return e instanceof ApiError ? e.describe() : String(e);
}

/** An action that failed (not a form: nothing is kept to try again with). */
export function actionError(e: unknown): string {
  if (e instanceof ApiError && e.status === 0) return t('error.noAnswer');
  return e instanceof ApiError ? e.describe() : String(e);
}

/** `true` for a moment after `flash()`: the "✓ Saved" state of a save bar or button. */
export function useFlash(): [boolean, () => void] {
  const [on, setOn] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(timer.current), []);
  return [
    on,
    () => {
      window.clearTimeout(timer.current);
      setOn(true);
      timer.current = window.setTimeout(() => setOn(false), DONE_MS);
    },
  ];
}

/** A button's own action (not a form save): busy, then done or failed. */
export function useAction() {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, flash] = useFlash();
  async function run(fn: () => Promise<unknown>, failed: string): Promise<boolean> {
    setBusy(true);
    setError(null);
    try {
      await fn();
      flash();
      return true;
    } catch (e) {
      setError(`${failed} ${actionError(e)}`);
      return false;
    } finally {
      setBusy(false);
    }
  }
  return { busy, done, error, run, clear: () => setError(null) };
}
export type Action = ReturnType<typeof useAction>;

/** "✓ Saved": success colour plus icon plus word. */
export function DoneNote({ children }: { children: string }) {
  return (
    <Group gap={4} wrap="nowrap" c="var(--app-success)" role="status">
      <IconCheck size={16} stroke={2} aria-hidden />
      <Text size="sm" fw={500} c="inherit">
        {children}
      </Text>
    </Group>
  );
}

/** Shown next to an action's button for a moment after it worked. */
export function ActionDone({ action, children }: { action: Action; children: string }) {
  return action.done ? <DoneNote>{children}</DoneNote> : null;
}

/** An action's failure, where it happened. */
export function ActionError({ action }: { action: Action }) {
  if (!action.error) return null;
  return (
    <Alert color="red" variant="light" icon={<IconAlertTriangle size={18} stroke={1.8} />} withCloseButton closeButtonLabel={t('close')} onClose={action.clear}>
      {action.error}
    </Alert>
  );
}
