import { Text } from '@mantine/core';
import { type CSSProperties, useEffect, useId, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { t } from '../i18n';
import { clock } from '../i18n/format';
import { reading, splitUnit } from '../lib/readings';
import './AirflowSchematic.css';

// The product's picture (KWL AirflowSchematic): fresh air in along the top lane, stale air out
// along the bottom, the heat exchanger between them, the bypass as one arc. Keep the geometry
// in step with the design system so the HTML labels sit exactly on their lanes.

export type BypassState = 'closed' | 'moving' | 'open';
export type FrostState = 'off' | 'boost' | 'defrost';

export interface FanState {
  rpm: number;
  stalled: boolean;
}

export interface AirflowProps {
  outdoor: number | null;
  supply: number | null;
  extract: number | null;
  exhaust: number | null;
  efficiency: number | null;
  bypass: BypassState;
  frost: FrostState;
  supplyFan: FanState;
  exhaustFan: FanState;
  /** null while reachable; else the time of the last answer ("14:31"). */
  offlineSince?: string | null;
  size: 'desktop' | 'phone';
}

const BLADES =
  'M0 -6.5 C3.3 -5 2.6 -1.6 0 0 C-2.6 -1.6 -3.3 -5 0 -6.5 Z M5.6 3.3 C4.9 6.6 1.6 5.8 0 0 C2.6 -0.8 5.8 0 5.6 3.3 Z M-5.6 3.3 C-5.8 0 -2.6 -0.8 0 0 C-1.6 5.8 -4.9 6.6 -5.6 3.3 Z';
const ARC = 'M66 36 V16 A10 10 0 0 1 76 6 H164 A10 10 0 0 1 174 16 V36';

/** Flow paths for the dots, in flow direction, with their lengths. */
const FLOW = {
  fresh: { d: 'M6 36 H222', len: 216 },
  freshBypass: { d: `M6 36 H66 V16 A10 10 0 0 1 76 6 H164 A10 10 0 0 1 174 16 V36 H222`, len: 60 + 20 + 15.7 + 88 + 15.7 + 20 + 48 },
  freshIn: { d: 'M6 36 H66', len: 60 },
  freshOut: { d: 'M174 36 H222', len: 48 },
  stale: { d: 'M234 100 H18', len: 216 },
} as const;

/** Spacing of the dots along a lane (viewBox units). */
const DOT_GAP = 34;
/** One lap per 6 s at level 1 down to 2 s at level 4: rpm of those levels on a KWL EC 300 W. */
const RPM_SLOW = 900;
const RPM_FAST = 2700;

/** Seconds a dot needs for one lane at this fan speed; null when the fan stands still. */
export function lapSeconds(rpm: number): number | null {
  if (rpm <= 0) return null;
  const t = Math.min(1, Math.max(0, (rpm - RPM_SLOW) / (RPM_FAST - RPM_SLOW)));
  return 6 - 4 * t;
}

type CentreInput = Pick<AirflowProps, 'efficiency' | 'bypass' | 'frost' | 'supplyFan' | 'exhaustFan'>;

/** Whether the centre shows the efficiency (heat recovery or frost protection) rather than a state. */
function showsEfficiency(p: CentreInput): boolean {
  const off = p.supplyFan.rpm <= 0 && p.exhaustFan.rpm <= 0 && !p.supplyFan.stalled && !p.exhaustFan.stalled;
  return !off && p.frost !== 'defrost' && p.bypass === 'closed';
}

/** The centre label: state words win over the efficiency. */
export function centre(p: CentreInput): [string, string] {
  if (p.supplyFan.rpm <= 0 && p.exhaustFan.rpm <= 0 && !p.supplyFan.stalled && !p.exhaustFan.stalled) {
    return [t('common:off'), t('overview:schematic.fansStopped')];
  }
  if (p.frost === 'defrost') return [t('overview:schematic.defrost'), t('overview:schematic.freshAirPaused')];
  if (p.bypass === 'open') return [t('overview:schematic.bypass'), t('overview:schematic.cooling')];
  if (p.bypass === 'moving') return [t('overview:schematic.bypass'), t('overview:schematic.moving')];
  const eff = reading(p.efficiency, 'percent');
  return [eff, p.frost === 'boost' ? t('overview:schematic.frostProtection') : t('overview:schematic.heatRecovery')];
}

const say = (v: number | null) => (v === null ? t('overview:schematic.say.noReading') : reading(v, 'temperature'));

/** The diagram as sentences for screen readers. */
export function describe(p: AirflowProps): string {
  const parts: string[] = [];
  if (p.bypass === 'open') {
    parts.push(t('overview:schematic.say.bypassOpen', { outdoor: say(p.outdoor), supply: say(p.supply) }));
  } else {
    parts.push(t('overview:schematic.say.warmed', { outdoor: say(p.outdoor), supply: say(p.supply) }));
  }
  parts.push(t('overview:schematic.say.extract', { extract: say(p.extract), exhaust: say(p.exhaust) }));
  const [title, sub] = centre(p);
  if (showsEfficiency(p)) parts.push(t('overview:schematic.say.recovery', { value: title === '—' ? t('overview:schematic.say.unknown') : title }));
  else parts.push(t('overview:schematic.say.state', { title, sub }));
  if (p.supplyFan.stalled) parts.push(t('overview:schematic.say.supplyStall'));
  if (p.exhaustFan.stalled) parts.push(t('overview:schematic.say.exhaustStall'));
  if (p.frost === 'boost') parts.push(t('overview:schematic.say.frost'));
  if (p.offlineSince) parts.push(t('overview:schematic.say.lastKnown', { time: clock(p.offlineSince) }));
  return parts.join(' ');
}

function usePageVisible() {
  const [visible, setVisible] = useState(() => typeof document === 'undefined' || !document.hidden);
  useEffect(() => {
    const on = () => setVisible(!document.hidden);
    document.addEventListener('visibilitychange', on);
    return () => document.removeEventListener('visibilitychange', on);
  }, []);
  return visible;
}

function Dots({ d, len, lap }: { d: string; len: number; lap: number }) {
  // The pattern repeats every DOT_GAP; a dot covers the lane in `lap` seconds.
  const style = { '--af-cycle': `${((DOT_GAP / len) * lap).toFixed(3)}s` } as CSSProperties;
  return <path className="kwl-flow-dot" d={d} style={style} />;
}

function Fan({ x, y, lap, fault, off }: { x: number; y: number; lap: number | null; fault: boolean; off: boolean }) {
  const spin = lap !== null && !fault && !off;
  return (
    <g transform={`translate(${x} ${y})`}>
      <circle className="kwl-af-fan" r={10} data-fault={fault || undefined} />
      <g className={spin ? 'kwl-af-blades' : undefined} style={spin ? ({ '--af-spin': `${(lap / 2).toFixed(2)}s` } as CSSProperties) : undefined}>
        <path className="kwl-af-blade" d={BLADES} data-off={off || undefined} />
      </g>
    </g>
  );
}

function Port({ name, dir, value, lane }: { name: string; dir: string; value: number | null; lane: 'top' | 'bot' }) {
  const [n, unit] = splitUnit(reading(value, 'temperature'));
  return (
    <div className="kwl-af-port" data-lane={lane}>
      <span className="kwl-af-name">
        {name}
        <span className="kwl-af-dir">{dir}</span>
      </span>
      <span className="kwl-display">
        {n}
        {unit && <span className="kwl-unit">{unit}</span>}
      </span>
    </div>
  );
}

export function AirflowSchematic(p: AirflowProps) {
  const { t } = useTranslation('overview');
  const pattern = `kwl-frost-${useId().replace(/:/g, '')}`;
  const visible = usePageVisible();
  const offline = p.offlineSince != null;
  const stopped = p.supplyFan.rpm <= 0 && p.exhaustFan.rpm <= 0;
  const defrost = p.frost === 'defrost';
  const hatch = p.frost !== 'off';

  const supplyLap = offline || p.supplyFan.stalled || defrost ? null : lapSeconds(p.supplyFan.rpm);
  const exhaustLap = offline || p.exhaustFan.stalled ? null : lapSeconds(p.exhaustFan.rpm);
  // Lanes fade when their fan stands still (fault, defrost, fans off).
  const freshDim = p.supplyFan.stalled || defrost || stopped;
  const staleDim = p.exhaustFan.stalled || stopped;
  const bypass = p.bypass !== 'closed';
  const [title, sub] = centre(p);

  return (
    <div>
      <div
        className="kwl-af"
        data-size={p.size}
        data-offline={offline || undefined}
        data-paused={!visible || undefined}
        role="img"
        aria-label={describe(p)}
      >
        <div className="kwl-af-side" data-side="l" aria-hidden="true">
          {t('schematic.outside')}
        </div>
        <div />
        <div className="kwl-af-side" data-side="r" aria-hidden="true">
          {t('schematic.inside')}
        </div>

        <div className="kwl-af-col" data-side="l" aria-hidden="true">
          <Port name={t('schematic.outdoor')} dir={t('schematic.in')} value={p.outdoor} lane="top" />
          <Port name={t('schematic.exhaust')} dir={t('schematic.out')} value={p.exhaust} lane="bot" />
        </div>

        <div className="kwl-af-mid" aria-hidden="true">
          <svg className="kwl-af-svg" viewBox="0 0 240 136">
            <defs>
              <pattern id={pattern} width="6" height="6" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
                <rect width="6" height="6" className="kwl-af-hx-fill" />
                <line x1="0" y1="0" x2="0" y2="6" className="kwl-af-frost-line" />
              </pattern>
            </defs>

            {/* Fresh-air lane, left → right (y = 36); keyed so a bypass change fades in. */}
            <g key={p.bypass} className="kwl-af-fade" opacity={freshDim ? 0.35 : undefined}>
              {bypass ? (
                <>
                  <path className="kwl-af-ln kwl-af-cold" d="M6 36 H66" />
                  <path className="kwl-af-ln kwl-af-cold" opacity={0.25} d="M66 36 H88" />
                  <path className="kwl-af-ln kwl-af-warm" opacity={0.25} d="M152 36 H174" />
                  <path className={`kwl-af-ln kwl-af-cold${p.bypass === 'moving' ? ' kwl-af-moving' : ''}`} d={ARC} />
                  <path className="kwl-af-ln kwl-af-cold" d="M174 36 H222" />
                  <path className="kwl-af-cold-f" d="M222 27 L236 36 L222 45 Z" />
                </>
              ) : (
                <>
                  <path className="kwl-af-ln kwl-af-cold" d="M6 36 H88" />
                  <path className="kwl-af-ln kwl-af-warm" d="M152 36 H222" />
                  <path className="kwl-af-warm-f" d="M222 27 L236 36 L222 45 Z" />
                </>
              )}
            </g>

            {/* Stale-air lane, right → left (y = 100). */}
            <g opacity={staleDim ? 0.35 : undefined}>
              <path className="kwl-af-ln kwl-af-warm" d="M234 100 H152" />
              <path className="kwl-af-ln kwl-af-cold" d="M88 100 H18" />
              <path className="kwl-af-cold-f" d="M18 91 L4 100 L18 109 Z" />
            </g>

            {supplyLap !== null &&
              (p.bypass === 'open' ? (
                <Dots {...FLOW.freshBypass} lap={supplyLap} />
              ) : p.bypass === 'moving' ? (
                <>
                  <Dots {...FLOW.freshIn} lap={supplyLap * (FLOW.freshIn.len / FLOW.fresh.len)} />
                  <Dots {...FLOW.freshOut} lap={supplyLap * (FLOW.freshOut.len / FLOW.fresh.len)} />
                </>
              ) : (
                <Dots {...FLOW.fresh} lap={supplyLap} />
              ))}
            {exhaustLap !== null && <Dots {...FLOW.stale} lap={exhaustLap} />}

            <rect
              className="kwl-af-hx"
              x="88"
              y="18"
              width="64"
              height="100"
              rx="10"
              data-frost={hatch || undefined}
              style={hatch ? { fill: `url(#${pattern})` } : undefined}
            />
            <g opacity={defrost ? 0.35 : undefined}>
              <Fan x={194} y={36} lap={supplyLap} fault={p.supplyFan.stalled} off={defrost || (stopped && !p.supplyFan.stalled)} />
            </g>
            <Fan x={48} y={100} lap={exhaustLap} fault={p.exhaustFan.stalled} off={stopped && !p.exhaustFan.stalled} />
          </svg>
          <div className="kwl-af-ctr">
            <div className="kwl-af-ctr-in">
              <b>{title}</b>
              <span>{sub}</span>
            </div>
          </div>
        </div>

        <div className="kwl-af-col" data-side="r" aria-hidden="true">
          <Port name={t('schematic.supply')} dir={t('schematic.toRooms')} value={p.supply} lane="top" />
          <Port name={t('schematic.extract')} dir={t('schematic.fromRooms')} value={p.extract} lane="bot" />
        </div>
      </div>
      {offline && (
        <Text size="xs" c="var(--mantine-color-placeholder)" mt={6}>
          {t('schematic.lastKnown', { time: clock(p.offlineSince!) })}
        </Text>
      )}
    </div>
  );
}
