// What the unit needs people to know, in StatusBadge priority order (KWL StatusBadge and
// FaultAlert): red = not ventilating properly or unreachable, yellow = do something soon,
// blue = the unit is doing something special on its own. Covered by faults.test.ts.
import type { Schemas, Status } from '../api/client';
import { t } from '../i18n';
import { clock } from '../i18n/format';

type Fault = Schemas['Fault'];

/** Faults that mean the unit is not ventilating as it should. */
export const STOPPING_FAULTS: readonly Fault[] = ['supply_fan_stall', 'exhaust_fan_stall'];

export type ConditionColor = 'red' | 'yellow' | 'blue';

export interface Condition {
  key: string;
  /** The word on the header badge. */
  badge: string;
  color: ConditionColor;
  /** Alert title and body; holiday has none (the Now card says it). */
  title?: string;
  body?: string;
  action?: { label: string; to: string };
}

// Read at call time, so the text follows the current language.
const restart = () => ({ label: t('faults:action.restart'), to: '/settings/restart' });

function sensorFault(f: Fault): Condition | null {
  const m = /^(outdoor|supply|extract|exhaust)_sensor_(open|short)$/.exec(f);
  if (!m) return null;
  const sensor = t(`faults:sensor.${m[1] as 'outdoor' | 'supply' | 'extract' | 'exhaust'}`);
  return {
    key: f,
    badge: t('faults:badge.checkUnit'),
    color: 'yellow',
    title: m[2] === 'open' ? t('faults:sensorOpen', { sensor }) : t('faults:sensorShort', { sensor }),
    body: t('faults:sensorBody'),
  };
}

/**
 * Every active condition, highest priority first. `lastSeen` is the time of the last answer
 * ("HH:MM"), for the offline alert.
 */
export function conditions(status: Status | null, offline: boolean, lastSeen: string | null = null): Condition[] {
  const out: Condition[] = [];
  if (offline) {
    out.push({
      key: 'offline',
      badge: t('faults:badge.offline'),
      color: 'red',
      title: t('faults:offline.title'),
      body: lastSeen ? t('faults:offline.bodySince', { time: clock(lastSeen) }) : t('faults:offline.body'),
    });
  }
  if (!status) return out;
  const has = (f: Fault) => status.faults.includes(f);
  if (has('supply_fan_stall') || status.supply_fan.stalled) {
    out.push({
      key: 'supply_fan_stall',
      badge: t('faults:badge.fault'),
      color: 'red',
      title: t('faults:supplyStall.title'),
      body: t('faults:supplyStall.body'),
      action: restart(),
    });
  }
  if (has('exhaust_fan_stall') || status.exhaust_fan.stalled) {
    out.push({
      key: 'exhaust_fan_stall',
      badge: t('faults:badge.fault'),
      color: 'red',
      title: t('faults:exhaustStall.title'),
      body: t('faults:exhaustStall.body'),
      action: restart(),
    });
  }
  for (const f of status.faults) {
    const s = sensorFault(f);
    if (s) out.push(s);
  }
  if (has('sd_card')) {
    out.push({
      key: 'sd_card',
      badge: t('faults:badge.checkUnit'),
      color: 'yellow',
      title: t('faults:sdCard.title'),
      body: t('faults:sdCard.body'),
    });
  }
  if (has('config_invalid')) {
    out.push({
      key: 'config_invalid',
      badge: t('faults:badge.checkUnit'),
      color: 'yellow',
      title: t('faults:configInvalid.title'),
      body: t('faults:configInvalid.body'),
      action: { label: t('faults:action.backup'), to: '/settings/backup' },
    });
  }
  if (has('bus_overcurrent')) {
    out.push({
      key: 'bus_overcurrent',
      badge: t('faults:badge.checkUnit'),
      color: 'yellow',
      title: t('faults:busOvercurrent.title'),
      body: t('faults:busOvercurrent.body'),
      action: restart(),
    });
  }
  if (status.filter.due || has('filter_change')) {
    out.push({
      key: 'filter',
      badge: t('faults:badge.filterDue'),
      color: 'yellow',
      title: t('faults:filter.title'),
      body: t('faults:filter.body'),
      action: { label: t('faults:action.confirmFilters'), to: '/settings/filters' },
    });
  }
  if (has('time_not_set')) {
    out.push({
      key: 'clock',
      badge: t('faults:badge.clockNotSet'),
      color: 'yellow',
      title: t('faults:clock.title'),
      body: t('faults:clock.body'),
      action: { label: t('faults:action.setClock'), to: '/settings/clock' },
    });
  }
  if (status.frost.defrost) {
    out.push({
      key: 'defrost',
      badge: t('faults:badge.defrosting'),
      color: 'blue',
      title: t('faults:defrost.title'),
      body: t('faults:defrost.body'),
    });
  } else if (status.frost.active) {
    out.push({
      key: 'frost',
      badge: t('faults:badge.frost'),
      color: 'blue',
      title: t('faults:frost.title'),
      body: t('faults:frost.body'),
    });
  }
  if (status.control.mode === 'holiday') out.push({ key: 'holiday', badge: t('common:mode.holiday'), color: 'blue' });
  return out;
}

export type BadgeColor = ConditionColor | 'green' | 'gray';

/** The header badge: the most important condition, how many more, or Off / Running. */
export function headline(status: Status | null, offline: boolean): { word: string; color: BadgeColor; more: number } | null {
  const all = conditions(status, offline);
  const [first] = all;
  if (first) return { word: first.badge, color: first.color, more: all.length - 1 };
  if (!status) return null;
  return status.control.level === 0 ? { word: t('common:off'), color: 'gray', more: 0 } : { word: t('faults:badge.running'), color: 'green', more: 0 };
}
