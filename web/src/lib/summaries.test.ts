import { describe, expect, it } from 'vitest';
import {
  backToAutoSummary,
  coolingSummary,
  dateRange,
  filterSummary,
  holidaySummary,
  minutes,
  programsSummary,
  uptime,
} from './summaries';

describe('summaries', () => {
  it('writes durations and dates the short way', () => {
    expect(minutes(45)).toBe('45 min');
    expect(minutes(120)).toBe('2 h');
    expect(minutes(105)).toBe('1 h 45 min');
    expect(dateRange('2026-10-12', '2026-10-24')).toBe('12\u2009–\u200924 Oct');
    expect(dateRange('2026-10-28', '2026-11-03')).toBe('28 Oct\u2009–\u20093 Nov');
  });

  it('summarises the Schedule rows', () => {
    expect(holidaySummary({ enabled: true, start: '2026-10-12', end: '2026-10-24', level: 1, interval_h: 2, active_min: 60 })).toBe('12\u2009–\u200924 Oct');
    expect(holidaySummary({ enabled: false, start: '', end: '', level: 1, interval_h: 2, active_min: 60 })).toBe('Off');
    expect(programsSummary({ party_level: 4, party_min: 120, quiet_level: 1, quiet_min: 60 })).toBe(
      'Party: level 4 for 2 h · Quiet: level 1 for 1 h',
    );
    const bypass = {
      enabled: true,
      target_c: 23,
      min_outdoor_c: 12,
      close_if_outdoor_warmer: true,
      actuator: 'motor' as const,
      run_time_s: 120,
      season: { start: '05-01', end: '09-30' },
      supply_offset_k: 0,
    };
    expect(coolingSummary(bypass)).toBe('On, May–Sept, above 23\u00a0°C');
    expect(coolingSummary({ ...bypass, enabled: false })).toBe('Off');
    const sched = { active_profile: 1, auto_at_midnight: true, auto_return_min: null, fallback_level: 2 };
    expect(backToAutoSummary(sched)).toBe('At midnight');
    expect(backToAutoSummary({ ...sched, auto_at_midnight: false, auto_return_min: 120 })).toBe('After 2 h');
    expect(backToAutoSummary({ ...sched, auto_at_midnight: false })).toBe('Never');
  });

  it('summarises the Settings rows', () => {
    expect(filterSummary({ remaining_days: 50, due: false }, 3)).toBe('50 days left');
    expect(filterSummary({ remaining_days: 0, due: true }, 3)).toBe('Due');
    expect(filterSummary({ remaining_days: 0, due: false }, 0)).toBe('Reminder off');
    expect(uptime(12 * 86400 + 5)).toBe('Running for 12 days');
    expect(uptime(7200)).toBe('Running for 2 h');
  });
});
