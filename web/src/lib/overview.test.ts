import { describe, expect, it } from 'vitest';
import { clockAfter, hintLine, manualReturn, nextChange, offConfirmLine, pausedLine, quietOffLine, stateSentence, subLine, todayChanges, todaySlots } from './overview';

const T = '2026-10-17T14:32:05'; // a Saturday
const day = [
  { from: '00:00', level: 1 },
  { from: '06:30', level: 2 },
  { from: '08:00', level: 1 },
  { from: '16:00', level: 3 },
  { from: '22:00', level: 1 },
];
const midnight = { auto_at_midnight: true, auto_return_min: null };
const auto = { mode: 'auto' as const, level: 2, remaining_s: null, auto_return_s: null, base: 'auto' as const, manual_level: 2 };
const manual3 = { ...auto, mode: 'manual' as const, level: 3, base: 'manual' as const, manual_level: 3 };

describe('Now card', () => {
  it('says the state in a sentence', () => {
    expect(stateSentence(auto)).toBe('Running at level 2');
    expect(stateSentence({ ...auto, level: 0 })).toBe('Fans off');
    expect(stateSentence({ ...auto, mode: 'party', level: 4 })).toBe('Party: level 4');
    expect(stateSentence({ ...auto, mode: 'quiet', level: 1 })).toBe('Quiet: level 1');
    expect(stateSentence({ ...auto, mode: 'holiday' })).toBe('Holiday');
  });

  it('shows the next change the unit reports', () => {
    const next = nextChange({ schedule: { profile: 1, today: day, next: { at: '16:00', level: 3, tomorrow: false } } });
    expect(next).toEqual({ level: 3, at: '16:00' });
    expect(nextChange({ schedule: null })).toBeNull();
    expect(subLine({ control: auto, time: T, next })).toBe('Auto · next: level 3 at 16:00');
  });

  it('says how long a manual choice lasts', () => {
    expect(subLine({ control: manual3, time: T, schedule: midnight, next: null })).toBe('Until midnight, then Auto');
    expect(subLine({ control: { ...manual3, auto_return_s: 105 * 60 }, time: T, schedule: { auto_at_midnight: true, auto_return_min: 120 }, next: null })).toBe(
      'For 1 h 45 min more, then Auto',
    );
    expect(subLine({ control: manual3, time: T, schedule: { auto_at_midnight: false, auto_return_min: null }, next: null })).toBe(
      'Until you choose Auto',
    );
    expect(hintLine(manual3, midnight, T)).toBe('Level 3 until midnight (manual), then Auto.');
    expect(hintLine(manual3, { auto_at_midnight: false, auto_return_min: null }, T)).toBe('Level 3 until you choose Auto.');
    expect(hintLine({ ...manual3, level: 0 }, midnight, T)).toBe('Fans off until midnight, then Auto.');
    expect(hintLine(auto, midnight, T)).toBe('Follows your schedule. Choosing a level overrides it until midnight.');
    expect(hintLine(auto, { auto_at_midnight: false, auto_return_min: 120 }, T)).toBe(
      'Follows your schedule. Choosing a level overrides it for 2 hours.',
    );
  });

  it('prefers the earlier of timer and midnight', () => {
    expect(manualReturn({ auto_at_midnight: true, auto_return_min: 120 }, 7200, T)).toEqual({ kind: 'timer', min: 120 });
    expect(manualReturn({ auto_at_midnight: true, auto_return_min: 720 }, 720 * 60, T)).toEqual({ kind: 'midnight' });
  });

  it('times Party and Quiet', () => {
    const party = { ...auto, mode: 'party' as const, level: 4, remaining_s: 488 * 60 };
    expect(clockAfter(T, 488 * 60)).toBe('22:40');
    expect(subLine({ control: party, time: T, next: null })).toBe('Until 22:40, then Auto');
    const fromManual = { ...party, base: 'manual' as const };
    expect(subLine({ control: fromManual, time: T, next: null })).toBe('Until 22:40, then level 2');
    expect(pausedLine(party, midnight, T)).toBe('Paused until 22:40. Auto picks up after Party.');
    expect(pausedLine(fromManual, midnight, T)).toBe('Paused until you choose Auto.');
  });

  it('describes the holiday', () => {
    const holiday = { enabled: true, start: '2026-10-12', end: '2026-10-24', level: 1, interval_h: 2, active_min: 60 };
    expect(subLine({ control: { ...auto, mode: 'holiday' }, time: T, next: null, holiday })).toBe(
      'Until 24 Oct · level 1 for 60 min every 2 h',
    );
    expect(subLine({ control: { ...auto, mode: 'holiday' }, time: T, next: null, holidayUntil: '2026-10-25' })).toBe('Until 25 Oct');
  });
});

describe('Today card', () => {
  it('lists the changes and marks the past ones', () => {
    expect(todaySlots({ schedule: { profile: 1, today: day.slice(1), next: null } })?.[0]).toEqual({ from: '00:00', level: 0 });
    expect(todayChanges(day, T)).toEqual([
      { at: '06:30', level: 2, past: true },
      { at: '08:00', level: 1, past: true },
      { at: '16:00', level: 3, past: false },
      { at: '22:00', level: 1, past: false },
    ]);
    expect(pausedLine(manual3, midnight, T)).toBe('Paused until midnight. Auto picks up at the next change.');
  });
});

describe('stop confirms', () => {
  it('says how long Off stops the ventilation', () => {
    expect(offConfirmLine(midnight, T)).toBe('Ventilation stops until midnight, then Auto takes over.');
    expect(offConfirmLine({ auto_at_midnight: true, auto_return_min: 120 }, T)).toBe('Ventilation stops for 2 hours, then Auto takes over.');
    expect(offConfirmLine({ auto_at_midnight: false, auto_return_min: 45 }, T)).toBe('Ventilation stops for 45 min, then Auto takes over.');
    // The timer would outlast midnight: midnight comes first.
    expect(offConfirmLine({ auto_at_midnight: true, auto_return_min: 600 }, T)).toBe('Ventilation stops until midnight, then Auto takes over.');
    expect(offConfirmLine({ auto_at_midnight: false, auto_return_min: null }, T)).toBe('Ventilation stops until you choose another level.');
  });

  it('says how long Quiet at level 0 stops the ventilation and what follows', () => {
    expect(quietOffLine(auto, 180)).toBe('Ventilation stops for 3 hours, then Auto.');
    expect(quietOffLine(manual3, 45)).toBe('Ventilation stops for 45 min, then level 3.');
  });
});
