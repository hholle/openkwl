import { describe, expect, it } from 'vitest';
import { describeMonthDay, formatMonthDay, parseMonthDay } from './monthday';

describe('monthday', () => {
  it('parses and clamps', () => {
    expect(parseMonthDay('05-01')).toEqual({ month: 5, day: 1 });
    expect(parseMonthDay('02-31')).toEqual({ month: 2, day: 29 });
    expect(parseMonthDay('13-01')).toEqual({ month: 1, day: 1 });
    expect(parseMonthDay('')).toEqual({ month: 1, day: 1 });
  });

  it('formats with a valid day', () => {
    expect(formatMonthDay(4, 31)).toBe('04-30');
    expect(formatMonthDay(10, 2)).toBe('10-02');
  });

  it('describes', () => {
    expect(describeMonthDay('03-01')).toBe('1 March');
  });
});
