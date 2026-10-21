import { describe, expect, it } from 'vitest';
import { conditions, headline } from './faults';
import { SAMPLE_STATUS, statusWith } from './fixtures';

describe('conditions', () => {
  it('names the part and the next step', () => {
    const c = conditions(statusWith({ faults: ['supply_fan_stall', 'outdoor_sensor_open', 'exhaust_sensor_short'] }), false);
    expect(c.map((x) => x.title)).toEqual([
      'Fault: supply fan not turning',
      'Outdoor air sensor not connected',
      'Exhaust air sensor short-circuited',
    ]);
    expect(c[0]!.action?.to).toBe('/settings/restart');
  });

  it('orders by priority: offline, fault, check unit, filter, clock, frost, holiday', () => {
    const s = statusWith({
      faults: ['time_not_set', 'sd_card', 'exhaust_fan_stall'],
      filter: { remaining_days: 0, due: true },
      frost: { active: true },
      control: { mode: 'holiday' },
    });
    expect(conditions(s, true).map((c) => c.badge)).toEqual([
      'Offline',
      'Fault',
      'Check unit',
      'Filter due',
      'Clock not set',
      'Frost protection',
      'Holiday',
    ]);
  });

  it('shows defrosting instead of frost protection', () => {
    expect(conditions(statusWith({ frost: { active: true, defrost: true } }), false).map((c) => c.key)).toEqual(['defrost']);
  });

  it('mentions the last values while offline', () => {
    expect(conditions(null, true, '14:31')[0]!.body).toBe('Showing the last values from 14:31. Trying again every few seconds.');
  });
});

describe('headline', () => {
  it('shows the top condition and counts the rest', () => {
    const s = statusWith({ faults: ['supply_fan_stall'], filter: { remaining_days: 0, due: true } });
    expect(headline(s, false)).toEqual({ word: 'Fault', color: 'red', more: 1 });
  });

  it('is Running, Off or nothing without data', () => {
    expect(headline(SAMPLE_STATUS, false)).toEqual({ word: 'Running', color: 'green', more: 0 });
    expect(headline(statusWith({ control: { level: 0 } }), false)?.word).toBe('Off');
    expect(headline(null, false)).toBeNull();
    expect(headline(null, true)?.word).toBe('Offline');
  });
});
