import { describe, expect, it } from 'vitest';
import { documentTitle } from './title';

describe('titles', () => {
  it('reads "Page · openKWL"', () => {
    expect(documentTitle('Schedule')).toBe('Schedule · openKWL');
    expect(documentTitle(null)).toBe('openKWL');
  });
});
