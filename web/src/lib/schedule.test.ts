import { describe, expect, it } from 'vitest';
import {
  addBlock,
  applyGrouping,
  blocks,
  type Day,
  detectGrouping,
  MAX_SLOTS,
  moveBoundary,
  normalize,
  normalizeWeek,
  overwrittenBy,
  removeBlock,
  setLevel,
  setStart,
  groupingQuestion,
} from './schedule';

const s = (from: string, level: number) => ({ from, level });
// The owner's "Standard 2": identical weekdays, different Saturday and Sunday.
const WEEKDAY: Day = [s('00:00', 1), s('06:30', 3), s('08:00', 1), s('16:00', 2), s('18:00', 3), s('20:30', 2), s('22:30', 1)];
const SAT: Day = [s('00:00', 1), s('08:00', 2), s('08:30', 3), s('10:00', 1), s('18:00', 3), s('21:00', 2)];
const SUN: Day = [s('00:00', 1), s('09:00', 2), s('10:30', 3), s('15:00', 2), s('22:00', 1)];
const WEEK = [WEEKDAY, WEEKDAY, WEEKDAY, WEEKDAY, WEEKDAY, SAT, SUN];

describe('normalize', () => {
  it('starts at 00:00, sorts and merges equal neighbours', () => {
    expect(normalize([s('08:00', 2), s('06:00', 2), s('10:00', 3)], 1)).toEqual([s('00:00', 1), s('06:00', 2), s('10:00', 3)]);
    expect(normalize([s('00:00', 1), s('06:00', 1)])).toEqual([s('00:00', 1)]);
    expect(normalize([])).toEqual([s('00:00', 0)]);
  });

  it('takes the previous day over midnight', () => {
    const week = normalizeWeek([[s('06:00', 2)], [], [], [], [], [], [s('00:00', 1), s('22:00', 4)]]);
    expect(week[0]).toEqual([s('00:00', 4), s('06:00', 2)]);
  });
});

describe('editing', () => {
  it('moves a boundary in 15-minute steps between its neighbours', () => {
    expect(moveBoundary(WEEKDAY, 1, 6 * 60 + 52)[1]).toEqual(s('06:45', 3));
    expect(moveBoundary(WEEKDAY, 1, 0)[1]!.from).toBe('00:15', );
    expect(moveBoundary(WEEKDAY, 1, 23 * 60)[1]!.from).toBe('07:45');
    expect(moveBoundary(WEEKDAY, 0, 300)).toBe(WEEKDAY);
  });

  it('sets exact times and levels, merging where needed', () => {
    expect(setStart(WEEKDAY, 1, '07:00')[1]).toEqual(s('07:00', 3));
    expect(setStart(WEEKDAY, 1, '07:07')[1]).toEqual(s('07:07', 3));
    expect(setStart(WEEKDAY, 0, '01:00')).toBe(WEEKDAY);
    expect(setStart(WEEKDAY, 1, '7:00')).toBe(WEEKDAY);
    expect(setLevel(WEEKDAY, 1, 1)).toEqual([s('00:00', 1), s('16:00', 2), s('18:00', 3), s('20:30', 2), s('22:30', 1)]);
  });

  it('keeps a typed start time between its neighbours', () => {
    // Block 1 (06:30) lies between 00:00 and 08:00: never passes or merges into them.
    expect(setStart(WEEKDAY, 1, '01:00')[1]).toEqual(s('01:00', 3));
    expect(setStart(WEEKDAY, 1, '00:00')[1]).toEqual(s('00:15', 3));
    expect(setStart(WEEKDAY, 1, '18:00')).toEqual([WEEKDAY[0], s('07:45', 3), ...WEEKDAY.slice(2)]);
    expect(setStart(WEEKDAY, 1, '18:00')).toHaveLength(WEEKDAY.length);
    // The last block ends at 24:00.
    expect(setStart(WEEKDAY, 6, '23:59')[6]).toEqual(s('23:45', 1));
    expect(setStart(WEEKDAY, 7, '12:00')).toBe(WEEKDAY);
  });

  it('removes blocks into their predecessor', () => {
    expect(removeBlock(WEEKDAY, 1)).toEqual([s('00:00', 1), s('16:00', 2), s('18:00', 3), s('20:30', 2), s('22:30', 1)]);
    expect(removeBlock([s('00:00', 1), s('12:00', 3)], 0)).toEqual([s('00:00', 3)]);
    expect(removeBlock([s('00:00', 1)], 0)).toEqual([s('00:00', 1)]);
  });

  it('adds a block in the longest gap and respects the limit', () => {
    expect(addBlock([s('00:00', 1)])).toEqual([s('00:00', 1), s('08:00', 2), s('16:00', 1)]);
    // Longest block 08:00–16:00 (level 1, followed by level 2): new block in its middle third.
    expect(addBlock(WEEKDAY)).toEqual([...WEEKDAY.slice(0, 3), s('10:45', 2), s('13:15', 1), ...WEEKDAY.slice(3)]);
    // One slot left: split the longest block (08:00–24:00, level 1, last) in half.
    const nine = [s('00:00', 1), s('01:00', 2), s('02:00', 1), s('03:00', 2), s('04:00', 1), s('05:00', 2), s('06:00', 1), s('07:00', 2), s('08:00', 1)];
    expect(addBlock(nine)).toEqual([...nine, s('16:00', 2)]);
    // …and avoids the next block's level when there is one (block 3 of 9: level 1, next level 2).
    const nineB = [s('00:00', 1), s('01:00', 2), s('02:00', 1), s('12:00', 2), s('13:00', 1), s('14:00', 2), s('15:00', 1), s('16:00', 2), s('17:00', 1)];
    expect(addBlock(nineB)).toEqual(normalize([...nineB, s('07:00', 0)]));
    const full = Array.from({ length: MAX_SLOTS }, (_, i) => s(`${String(i * 2).padStart(2, '0')}:00`, i % 2));
    expect(addBlock(full)).toBeNull();
  });

  it('describes blocks and now/next', () => {
    expect(blocks(SUN).at(-1)).toEqual({ start: 22 * 60, end: 1440, level: 1 });
  });
});

describe('grouping', () => {
  it('detects the coarsest lossless grouping', () => {
    expect(detectGrouping(WEEK)).toBe('weekdays');
    expect(detectGrouping(Array(7).fill(SAT))).toBe('all');
    expect(detectGrouping([SUN, ...WEEK.slice(1)])).toBe('each');
  });

  it('reports and applies what a coarser grouping overwrites', () => {
    expect(overwrittenBy(WEEK, 'weekdays')).toEqual([]);
    expect(overwrittenBy(WEEK, 'all')).toEqual([5, 6]);
    const all = applyGrouping(WEEK, 'all');
    expect(all.every((d) => d === all[0] || JSON.stringify(d) === JSON.stringify(WEEKDAY))).toBe(true);
  });
});

describe('grouping question', () => {
  it('names the days that will be replaced', () => {
    const odd = WEEK.map((d, i) => (i === 0 ? d : SUN));
    expect(groupingQuestion(odd, 'all')).toEqual({
      title: "Use Monday's times for every day?",
      body: 'The times for Tuesday to Sunday are replaced. Nothing is saved until you choose Save schedule.',
    });
    expect(groupingQuestion(WEEK, 'all')).toEqual({
      title: "Use Monday's times for every day?",
      body: 'The times for Saturday and Sunday are replaced. Nothing is saved until you choose Save schedule.',
    });
    expect(groupingQuestion(WEEK, 'weekdays')).toBeNull();
  });
});
