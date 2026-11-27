import { describe, expect, it } from 'vitest';
import {
  hasPermission,
  HomeLayout,
  InviteCheck,
  PairingStatus,
  ResetCreated,
  SidebarLayout,
  User,
} from './schemas';

const user = {
  id: 'u1',
  email: 'max@kroma.tv',
  username: 'max',
  permissions: ['playback', 'users.manage'],
  createdAt: '2026-01-01',
  hasPin: false,
};

describe('hasPermission', () => {
  it('reads a capability the account holds', () => {
    expect(hasPermission(User.parse(user), 'users.manage')).toBe(true);
  });

  it('refuses one it does not', () => {
    expect(hasPermission(User.parse(user), 'settings.manage')).toBe(false);
  });

  it('reads a session persisted before capabilities existed as holding none', () => {
    expect(hasPermission({ permissions: undefined as unknown as string[] }, 'playback')).toBe(
      false,
    );
  });
});

describe('the account schema', () => {
  it('keeps a capability this build has never heard of, rather than throwing', () => {
    const ahead = { ...user, permissions: ['playback', 'holodeck.manage'] };

    expect(User.parse(ahead).permissions).toContain('holodeck.manage');
  });

  it('refuses a body that is not an account at all', () => {
    expect(() => User.parse({ ...user, id: 42 })).toThrow();
  });
});

describe('the account home layout', () => {
  it('keeps the order and accepts a hidden component with or without an explicit position', () => {
    const layout = { order: ['genre:28', 'hero'], hidden: ['hero', 'for-you'] };
    expect(HomeLayout.parse(layout)).toEqual(layout);
    expect(HomeLayout.parse({ order: [], hidden: [] })).toEqual({ order: [], hidden: [] });
  });

  it.each(['order', 'hidden'])('bounds and validates every id in %s', (field) => {
    const ids = Array.from({ length: 1024 }, (_, n) => `widget.${n}`);
    const parses = (list: string[]) =>
      HomeLayout.safeParse({ order: [], hidden: [], [field]: list }).success;
    expect(parses(ids)).toBe(true);
    expect(parses(['a'.repeat(64)])).toBe(true);
    for (const bad of [[...ids, 'other'], ['a'.repeat(65)], [''], ['#row'], ['a/b'], ['a', 'a']]) {
      expect(parses(bad)).toBe(false);
    }
  });

  it('requires both lists and does not accept an account selector', () => {
    expect(HomeLayout.safeParse({ order: [] }).success).toBe(false);
    expect(HomeLayout.safeParse({ order: [], hidden: [], userId: 'other' }).success).toBe(false);
  });
});

it('accepts bounded sidebar ids, not home fields or an account selector', () => {
  const hidden = Array.from({ length: 1024 }, (_, n) => `library:${n}`);
  expect(SidebarLayout.parse({ hidden })).toEqual({ hidden });
  expect(SidebarLayout.parse({ hidden: [] })).toEqual({ hidden: [] });
  for (const invalid of [
    {}, { hidden, order: [] }, { hidden, userId: 'other' }, { hidden: [...hidden, 'extra'] },
    { hidden: ['x', 'x'] }, { hidden: ['library/1'] }, { hidden: ['a'.repeat(65)] },
  ]) {
    expect(SidebarLayout.safeParse(invalid).success).toBe(false);
  }
});

describe('the pairing status', () => {
  it('reads each arm of the union the two polls answer with', () => {
    expect(PairingStatus.parse({ status: 'pending' })).toEqual({ status: 'pending' });
    expect(PairingStatus.parse({ status: 'expired' })).toEqual({ status: 'expired' });
    expect(
      PairingStatus.parse({
        status: 'authorized',
        token: 't',
        accessToken: 'a',
        user: User.parse(user),
      }).status,
    ).toBe('authorized');
  });

  it('refuses a status nothing answers with', () => {
    expect(() => PairingStatus.parse({ status: 'sideways' })).toThrow();
  });
});

describe('a minted reset', () => {
  it('reads a delivery this build does not know as a manual one', () => {
    const reset = { token: 't', code: '123456', url: null, expiresAt: 1, delivered: 'pigeon' };

    expect(ResetCreated.parse(reset).delivered).toBe('manual');
  });
});

describe('the check of an invitation', () => {
  it('names the account that waits on it, for the join form to start from', () => {
    const checked = InviteCheck.parse({
      valid: true,
      expiresAt: 1,
      username: 'ana',
      email: 'ana@old.test',
    });

    expect(checked.username).toBe('ana');
    expect(checked.email).toBe('ana@old.test');
  });

  it('names nobody for an invitation that creates an account, and for a dead one', () => {
    expect(InviteCheck.parse({ valid: true, expiresAt: 1 }).username).toBeUndefined();
    expect(InviteCheck.parse({ valid: false })).toEqual({ valid: false });
  });
});
