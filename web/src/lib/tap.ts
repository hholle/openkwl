import type { CSSProperties } from 'react';

/**
 * Enlarges a small link's hit area to 42px+ tall (KWL: tap targets of at least 42px on phones)
 * without moving anything: padding in, the same margin out.
 */
export const TAP: CSSProperties = { display: 'inline-block', paddingBlock: 12, marginBlock: -12 };

/** The same for a Switch: its whole row is a label, so pad that row to 42px+ tall. */
export const SWITCH_TAP = { body: { paddingBlock: 9 }, root: { marginBlock: -9 } } satisfies Record<string, CSSProperties>;

/** PasswordInput's show/hide button at 42px; transparent, so the overhang past a 36px field never shows. */
export const PASSWORD_TAP = {
  rightSectionWidth: 42,
  visibilityToggleButtonProps: { variant: 'transparent' },
  styles: { visibilityToggle: { width: 42, height: 42, minWidth: 42, minHeight: 42 } },
} as const;

/** A Checkbox: only its label is clickable next to the box, so pad the label and take the space back. */
export const CHECKBOX_TAP = { label: { paddingBlock: 11, marginBlock: -11 } } satisfies Record<string, CSSProperties>;
