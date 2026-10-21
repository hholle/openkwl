import { describe, expect, it } from 'vitest';
import { duration } from './format';

describe('format', () => {
  it('durations', () => {
    expect(duration(45)).toBe('45 s');
    expect(duration(61)).toBe('2 min');
    expect(duration(3600)).toBe('1 h');
    expect(duration(7500)).toBe('2 h 05 min');
  });
});

describe('technical formats', () => {
  it('bytes', async () => {
    const { bytes } = await import('./format');
    expect(bytes(512)).toBe('512 B');
    expect(bytes(229_888)).toBe('224.5 KB');
    expect(bytes(16 * 1024 * 1024)).toBe('16.0 MB');
  });

  it('hex', async () => {
    const { hex } = await import('./format');
    expect(hex(0x411)).toBe('0x411');
    expect(hex(0x2000, 4)).toBe('0x2000');
  });
});
