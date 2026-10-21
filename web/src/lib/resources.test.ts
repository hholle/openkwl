import { describe, expect, it } from 'vitest';
import { clearResources, peekResource, reload, setResource } from './resources';

describe('shared resources', () => {
  it('keeps the loaded copy and forgets it on clear', async () => {
    await reload('a', () => Promise.resolve(1));
    expect(peekResource('a')).toBe(1);
    setResource('a', 2);
    expect(peekResource('a')).toBe(2);
    clearResources();
    expect(peekResource('a')).toBeUndefined();
  });

  it('ignores a load from before the clear', async () => {
    let finish: (v: string) => void = () => undefined;
    const old = reload('b', () => new Promise<string>((r) => (finish = r)));
    clearResources();
    const fresh = reload('b', () => Promise.resolve('new'));
    finish('old');
    await Promise.all([old, fresh]);
    expect(peekResource('b')).toBe('new');
  });

  it('loads again after a failure', async () => {
    await reload('c', () => Promise.reject(new Error('offline')));
    expect(peekResource('c')).toBeUndefined();
    await reload('c', () => Promise.resolve('back'));
    expect(peekResource('c')).toBe('back');
  });
});
