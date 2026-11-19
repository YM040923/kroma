// @vitest-environment jsdom

import { ConfirmHost } from '@kromatv/ui/kit';
import { createInputTestHost, onScreen } from '@kromatv/ui/testing';
import { cleanup, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { afterAll, afterEach, describe, expect, it, vi } from 'vitest';
import { SeasonAsk } from '#web/features/catalog/request-bar.component';

vi.hoisted(() => {
  vi.stubEnv('NODE_ENV', 'development');
});

afterAll(() => {
  vi.unstubAllEnvs();
});

afterEach(cleanup);

const inputHost = createInputTestHost();

function ask(
  over: Partial<Parameters<typeof SeasonAsk>[0]> = {},
  callbacks: { onRequestSeason?: () => void; onRequestAll?: () => void } = {},
) {
  return inputHost.render(
    onScreen(
      <ConfirmHost>
        <SeasonAsk
          season={2}
          seasonMissing={3}
          seasonRequestable
          allMissing={[{ number: 2, count: 3 }]}
          busy={false}
          onRequestSeason={callbacks.onRequestSeason ?? (() => {})}
          onRequestAll={callbacks.onRequestAll ?? (() => {})}
          {...over}
        />
      </ConfirmHost>,
    ),
  );
}

describe('the missing-season request actions', () => {
  it('names the whole-season request by its episode scope', () => {
    ask();

    expect(screen.getByText('Request season 2 · 3 episodes')).toBeTruthy();
    expect(screen.getByText('Request all missing seasons · 1 season')).toBeTruthy();
  });

  it('shows neither action when nothing is requestable', () => {
    ask({ seasonRequestable: false, allMissing: [] });

    expect(screen.queryByText(/Request season/)).toBeNull();
    expect(screen.queryByText(/Request all missing seasons/)).toBeNull();
  });

  it('confirms the season scope and the skip rule before submitting', async () => {
    const onRequestSeason = vi.fn();
    ask({}, { onRequestSeason });

    inputHost.clearPressGuard();
    fireEvent.click(screen.getByText('Request season 2 · 3 episodes'));
    const dialog = await screen.findByRole('dialog');
    expect(
      within(dialog).getByText(/This asks for the 3 missing episodes of season 2\./),
    ).toBeTruthy();
    expect(
      within(dialog).getByText(/Episodes already owned or requested are skipped\./),
    ).toBeTruthy();
    expect(onRequestSeason).not.toHaveBeenCalled();

    inputHost.clearPressGuard();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Send request' }));
    await waitFor(() => expect(onRequestSeason).toHaveBeenCalledTimes(1));
  });

  it('submits nothing when the confirmation is dismissed', async () => {
    const onRequestSeason = vi.fn();
    ask({}, { onRequestSeason });

    inputHost.clearPressGuard();
    fireEvent.click(screen.getByText('Request season 2 · 3 episodes'));
    const dialog = await screen.findByRole('dialog');
    inputHost.clearPressGuard();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }));

    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(onRequestSeason).not.toHaveBeenCalled();
  });

  it('lists every missing season in the all-seasons confirmation', async () => {
    const onRequestAll = vi.fn();
    ask(
      {
        seasonRequestable: false,
        allMissing: [
          { number: 2, count: 3 },
          { number: 5, count: 1 },
        ],
      },
      { onRequestAll },
    );

    inputHost.clearPressGuard();
    fireEvent.click(screen.getByText('Request all missing seasons · 2 seasons'));
    const dialog = await screen.findByRole('dialog');
    expect(
      within(dialog).getByText(
        /Missing seasons: season 2 \(3 episodes\) · season 5 \(1 episode\)/,
      ),
    ).toBeTruthy();

    inputHost.clearPressGuard();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Send request' }));
    await waitFor(() => expect(onRequestAll).toHaveBeenCalledTimes(1));
  });
});
