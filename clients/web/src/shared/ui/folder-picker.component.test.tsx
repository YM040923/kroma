// @vitest-environment jsdom
import { KromaApiError } from '@kromatv/client';
import { I18nProvider } from '@kromatv/ui';
import { createInputTestHost } from '@kromatv/ui/testing';
import { cleanup, fireEvent, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { FolderPicker } from './folder-picker.component';

const { library } = vi.hoisted(() => ({ library: { browse: vi.fn() } }));
vi.mock('#web/shared/lib/web-runtime', () => ({ useWebRuntime: () => ({ client: { library } }) }));
const host = createInputTestHost();
afterEach(() => {
  cleanup();
  library.browse.mockReset();
});

function show(value: string, onChange = vi.fn()) {
  host.render(
    <I18nProvider locale="en">
      <FolderPicker value={value} onChange={onChange} />
    </I18nProvider>,
  );
  return onChange;
}

async function click(text: string) {
  host.clearPressGuard();
  fireEvent.click(await screen.findByText(text));
}

it('can go up to volumes, switch drives and commit the path confirmed by the server', async () => {
  const leaf = 'E:\\电影 Films';
  library.browse.mockImplementation(async (path: string) => {
    if (path === '')
      return {
        path,
        parent: null,
        entries: ['C:\\', 'E:\\'].map((name) => ({ name, path: name })),
      };
    if (path === 'E:\\') return { path, parent: '', entries: [{ name: '电影 Films', path: leaf }] };
    return { path, parent: path === leaf ? 'E:\\' : '', entries: [] };
  });
  const choose = show('C:\\');
  await screen.findByText('No subfolders');
  host.clearPressGuard();
  fireEvent.click(screen.getByRole('button', { name: 'Parent folder' }));
  await click('E:\\');
  await click('电影 Films');
  await screen.findByText('No subfolders');
  await click('Use this folder');
  expect(choose).toHaveBeenCalledWith(leaf);
});

it('does not commit a missing folder; the error allows a return to drives', async () => {
  library.browse
    .mockRejectedValueOnce(new KromaApiError(404, 'Not found', { error: 'Drive unavailable' }))
    .mockResolvedValue({ path: '', parent: null, entries: [{ name: 'E:\\', path: 'E:\\' }] });
  const choose = show('Z:\\missing');
  await screen.findByText('Drive unavailable');
  await click('Use this folder');
  expect(choose).not.toHaveBeenCalled();
  host.clearPressGuard();
  fireEvent.click(screen.getByRole('button', { name: 'Parent folder' }));
  expect(await screen.findByText('E:\\')).toBeTruthy();
});
