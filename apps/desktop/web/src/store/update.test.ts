import { describe, expect, it } from 'vitest';
import type { UpdateInfo } from '@/api';
import { reduceUpdate, updatableVersion, type UpdateState } from './update';

const info: UpdateInfo = { version: '0.3.0', currentVersion: '0.2.0', notes: null };
const idle: UpdateState = { phase: 'idle' };

describe('checking for an update', () => {
  it('goes checking, then available when a newer version exists', () => {
    const checking = reduceUpdate(idle, { type: 'checkStarted' });
    expect(checking).toEqual({ phase: 'checking' });
    expect(reduceUpdate(checking, { type: 'checkFinished', info })).toEqual({ phase: 'available', info });
  });

  it('goes back to idle when this is the latest version', () => {
    expect(reduceUpdate({ phase: 'checking' }, { type: 'checkFinished', info: null })).toEqual(idle);
  });

  it('reports a failed check as an error without a version', () => {
    expect(reduceUpdate({ phase: 'checking' }, { type: 'checkFailed', message: 'offline' })).toEqual({ phase: 'error', message: 'offline' });
  });

  it('does not start a second check while one runs, nor forget a version already found', () => {
    expect(reduceUpdate({ phase: 'checking' }, { type: 'checkStarted' })).toEqual({ phase: 'checking' });
    const available: UpdateState = { phase: 'available', info };
    expect(reduceUpdate(available, { type: 'checkStarted' })).toBe(available);
  });
});

describe('installing', () => {
  it('only starts from a known version', () => {
    expect(reduceUpdate(idle, { type: 'installStarted' })).toBe(idle);
    expect(reduceUpdate({ phase: 'error', message: 'offline' }, { type: 'installStarted' })).toEqual({ phase: 'error', message: 'offline' });
    expect(reduceUpdate({ phase: 'available', info }, { type: 'installStarted' })).toEqual({ phase: 'installing', info });
  });

  it('keeps the version after a failed install so it can be retried', () => {
    const failed = reduceUpdate({ phase: 'installing', info }, { type: 'installFailed', message: 'disk full' });
    expect(failed).toEqual({ phase: 'error', message: 'disk full', info });
    expect(updatableVersion(failed)).toEqual(info);
    expect(reduceUpdate(failed, { type: 'installStarted' })).toEqual({ phase: 'installing', info });
  });

  it('ignores check results that arrive while installing', () => {
    const installing: UpdateState = { phase: 'installing', info };
    expect(reduceUpdate(installing, { type: 'checkFinished', info: null })).toBe(installing);
    expect(reduceUpdate(installing, { type: 'checkFailed', message: 'x' })).toBe(installing);
    expect(reduceUpdate(installing, { type: 'checkStarted' })).toBe(installing);
  });
});

describe('updatableVersion', () => {
  it('is null unless a version is known', () => {
    expect(updatableVersion(idle)).toBeNull();
    expect(updatableVersion({ phase: 'checking' })).toBeNull();
    expect(updatableVersion({ phase: 'error', message: 'x' })).toBeNull();
    expect(updatableVersion({ phase: 'available', info })).toEqual(info);
  });
});
