import { Box, Group, Text, Tooltip, UnstyledButton, useComputedColorScheme } from '@mantine/core';
import { useElementSize, useMediaQuery } from '@mantine/hooks';
import { type KeyboardEvent, type PointerEvent, useMemo, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { clock } from '../i18n/format';
import { blocks, type Day, DAY_MIN, fromMin, moveBoundary, STEP_MIN, toMin } from '../lib/schedule';
import { levelWord } from '../lib/summaries';

// KWL ScheduleBar: a day 00–24 h as blocks shaded by fan level (grey ramp, Off hatched), the
// brand-coloured "now" line on today, and on precise pointers draggable boundaries. Keyboard: arrows move
// the focused boundary by 15 min, Shift by an hour.

const pct = (m: number) => `${(m / DAY_MIN) * 100}%`;

export function DayBar({
  day,
  label,
  nowMin = null,
  height = 28,
  dim = false,
  selected = false,
  onSelect,
  onChange,
}: {
  day: Day;
  /** "Mon–Fri", for screen readers. */
  label: string;
  nowMin?: number | null;
  height?: number;
  dim?: boolean;
  selected?: boolean;
  onSelect?: () => void;
  /** Boundaries become editable (keyboard; dragging on precise pointers). */
  onChange?: (d: Day) => void;
}) {
  const { t } = useTranslation(['schedule', 'common']);
  const { ref, width } = useElementSize();
  const bar = useRef<HTMLDivElement>(null);
  const dragging = useRef<number | null>(null);
  const fine = useMediaQuery('(pointer: fine)', true, { getInitialValueInEffect: false });
  const bs = useMemo(() => blocks(day), [day]);

  const minuteAt = (clientX: number) => {
    const r = bar.current!.getBoundingClientRect();
    return ((clientX - r.left) / r.width) * DAY_MIN;
  };
  const onPointerDown = (i: number) => (e: PointerEvent) => {
    if (!fine || e.pointerType === 'touch') return;
    dragging.current = i;
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
    onSelect?.();
  };
  const onPointerMove = (e: PointerEvent) => {
    if (dragging.current !== null && onChange) onChange(moveBoundary(day, dragging.current, minuteAt(e.clientX)));
  };
  // Also on cancel (pen, OS gesture) and lost capture, or hovering would keep moving the boundary.
  const endDrag = () => {
    dragging.current = null;
  };
  const onKey = (i: number) => (e: KeyboardEvent) => {
    if (!onChange) return;
    const cur = toMin(day[i]!.from);
    const step = e.shiftKey ? 60 : STEP_MIN;
    const target = { ArrowLeft: cur - step, ArrowDown: cur - step, ArrowRight: cur + step, ArrowUp: cur + step, Home: 0, End: DAY_MIN }[e.key];
    if (target === undefined) return;
    e.preventDefault();
    onChange(moveBoundary(day, i, target));
  };

  return (
    <Box
      ref={bar}
      pos="relative"
      h={height}
      opacity={dim ? 0.5 : undefined}
      onPointerMove={onPointerMove}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
      onLostPointerCapture={endDrag}
      style={{
        outline: selected ? '2px solid var(--mantine-color-text)' : undefined,
        outlineOffset: 2,
        borderRadius: 6,
        touchAction: 'pan-y',
      }}
    >
      <UnstyledButton
        ref={ref}
        onClick={onSelect}
        aria-label={t('bar.label', { label, blocks: bs.map((b) => `${clock(fromMin(b.start))} ${levelWord(b.level)}`).join(', ') })}
        pos="absolute"
        inset={0}
        bg="var(--mantine-color-body)"
        style={{ borderRadius: 6, overflow: 'hidden', cursor: onSelect ? 'pointer' : 'default' }}
      >
        {bs.map((b) => {
          const px = ((b.end - b.start) / DAY_MIN) * width;
          const text = b.level === 0 ? t('common:off') : String(b.level);
          const block = (
            <Box
              key={b.start}
              className={`kwl-level-${b.level}`}
              pos="absolute"
              top={0}
              bottom={0}
              left={pct(b.start)}
              w={`calc(${pct(b.end - b.start)} - 1px)`}
              fz={11}
              fw={600}
              style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', overflow: 'hidden' }}
            >
              {px > (b.level === 0 ? 26 : 18) ? text : ''}
            </Box>
          );
          return px > 18 ? (
            block
          ) : (
            <Tooltip key={b.start} label={`${clock(fromMin(b.start))} ${levelWord(b.level)}`}>
              {block}
            </Tooltip>
          );
        })}
      </UnstyledButton>
      {nowMin !== null && (
        <Box
          aria-hidden="true"
          pos="absolute"
          top={-3}
          bottom={-3}
          left={pct(nowMin)}
          w={2}
          ml={-1}
          bg="var(--app-now-marker)"
          style={{ boxShadow: '0 0 0 1px var(--mantine-color-body)', pointerEvents: 'none' }}
        />
      )}
      {onChange &&
        day.slice(1).map((s, k) => {
          const i = k + 1;
          const m = toMin(s.from);
          return (
            <Box
              key={i}
              role="slider"
              tabIndex={0}
              className="mantine-focus-auto"
              aria-label={t('bar.startOf', { label, level: levelWord(s.level) })}
              aria-valuemin={toMin(day[i - 1]!.from) + STEP_MIN}
              aria-valuemax={(i + 1 < day.length ? toMin(day[i + 1]!.from) : DAY_MIN) - STEP_MIN}
              aria-valuenow={m}
              aria-valuetext={`${clock(s.from)}, ${levelWord(s.level)}`}
              pos="absolute"
              top={-4}
              bottom={-4}
              left={pct(m)}
              w={14}
              ml={-7}
              onPointerDown={onPointerDown(i)}
              onKeyDown={onKey(i)}
              onFocus={onSelect}
              style={{ cursor: fine ? 'ew-resize' : 'default', touchAction: 'none', zIndex: 1, borderRadius: 4 }}
            >
              <Box pos="absolute" left={6} top={4} bottom={4} w={2} bg="var(--mantine-color-text)" opacity={0.45} />
            </Box>
          );
        })}
    </Box>
  );
}

/** 00 / 06 / 12 / 18 / 24 under the last bar. */
export function HourAxis({ offset = 0 }: { offset?: number }) {
  return (
    <Group justify="space-between" pl={offset} mt={4} aria-hidden="true">
      {['00', '06', '12', '18', '24'].map((h) => (
        <Text key={h} fz={11} c="var(--mantine-color-placeholder)" className="tabular">
          {h}
        </Text>
      ))}
    </Group>
  );
}

/** Off / 1 / 2 / 3 / 4 with a swatch; the selected chip has a 2px ink border (not red). */
export function LevelChips({ value, onChange, label }: { value: number; onChange: (l: number) => void; label: string }) {
  const { t } = useTranslation(['schedule', 'common']);
  const outline = useBorderStrong();
  return (
    <Group gap={4} role="radiogroup" aria-label={label} wrap="nowrap">
      {[0, 1, 2, 3, 4].map((l) => {
        const on = value === l;
        return (
          <UnstyledButton
            key={l}
            role="radio"
            aria-checked={on}
            onClick={() => onChange(l)}
            h={{ base: 42, sm: 32 }}
            px={on ? 9 : 10}
            fz="sm"
            fw={on ? 600 : 500}
            bg="var(--mantine-color-body)"
            style={{
              flex: 1,
              display: 'inline-flex',
              alignItems: 'center',
              justifyContent: 'center',
              gap: 6,
              borderRadius: 'var(--mantine-radius-md)',
              border: on ? '2px solid var(--mantine-color-text)' : `1px solid ${outline}`,
            }}
          >
            <Box component="span" className={`kwl-level-${l}`} w={10} h={10} style={{ borderRadius: 3 }} />
            {l === 0 ? t('common:off') : l}
          </UnstyledButton>
        );
      })}
    </Group>
  );
}

/** The `border-strong` token (control outlines, 3:1) in the current colour scheme. */
export function useBorderStrong() {
  return useComputedColorScheme('light') === 'dark' ? 'var(--mantine-color-dark-4)' : 'var(--mantine-color-gray-4)';
}
