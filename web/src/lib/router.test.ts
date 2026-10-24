import { describe, expect, it } from 'vitest';
import { parseRoute, pathOf, redirectFor } from './router';

describe('routes', () => {
  it('parses the four pages and their sections', () => {
    expect(parseRoute('/')).toEqual({ page: 'overview', section: null });
    expect(parseRoute('/schedule/week')).toEqual({ page: 'schedule', section: 'week' });
    expect(parseRoute('/settings')).toEqual({ page: 'settings', section: null });
    expect(parseRoute('/history')).toEqual({ page: 'history', section: null });
    expect(parseRoute('/history/month')).toEqual({ page: 'history', section: 'month' });
    expect(pathOf('history', 'day')).toBe('/history/day');
    expect(parseRoute('/nonsense')).toEqual({ page: 'overview', section: null });
    expect(pathOf('settings', 'clock')).toBe('/settings/clock');
    expect(pathOf('overview')).toBe('/');
  });

  it('redirects old URLs to their new place', () => {
    expect(redirectFor('/dashboard')).toBe('/');
    expect(redirectFor('/maintenance')).toBe('/settings/filters');
    expect(redirectFor('/settings/holiday')).toBe('/schedule/holiday');
    expect(redirectFor('/settings/party-quiet')).toBe('/schedule/party-quiet');
    expect(redirectFor('/settings/bypass')).toBe('/schedule/summer-cooling');
    expect(redirectFor('/settings/clock')).toBeNull();
    expect(redirectFor('/schedule/week/')).toBe('/schedule/week');
  });

  it('redirects the previous hash routes', () => {
    expect(redirectFor('/', '#/dashboard')).toBe('/');
    expect(redirectFor('/', '#/schedule')).toBe('/schedule/week');
    expect(redirectFor('/', '#/maintenance')).toBe('/settings/filters');
    expect(redirectFor('/', '#/settings')).toBe('/settings');
    expect(redirectFor('/', '')).toBeNull();
  });
});
