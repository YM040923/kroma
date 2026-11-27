// @vitest-environment jsdom
import { fireEvent, render, screen } from '@testing-library/react';
import { expect, it, vi } from 'vitest';
import en from '../../locales/en.json';
import { CloudHistoryEntry, type MappingProgress, SyncReport } from './schemas';
import { SyncBoard } from './sync-board.component';

vi.mock('@kromatv/module-sdk', () => ({
  useT:
    () =>
    (key: keyof typeof en, args: Record<string, number> = {}) =>
      en[key].replace(/\{(\w+)\}/g, (_, name: string) => String(args[name])),
}));

const empty = SyncReport.parse({
  current: '',
  elapsedMs: 0,
  dirs: 0,
  files: 0,
  written: 0,
  kept: 0,
  pruned: 0,
  probed: 0,
  pendingProbe: 0,
  assets: 0,
  calls: 0,
  callsTotal: 1000,
  truncated: false,
  errors: [],
});
const first: MappingProgress = {
  remote: '/电影',
  localDisplay: 'D:\\媒体库\\电影',
  state: 'discovering',
  discoveredDirs: 7,
  totalDirs: null,
  doneDirs: 0,
  files: 30,
  written: 0,
  errorCount: 0,
  errors: [],
};
const next: MappingProgress = {
  ...first,
  remote: '/剧集',
  localDisplay: 'D:\\媒体库\\剧集',
  state: 'queued',
  discoveredDirs: 0,
  files: 0,
};
const partial: MappingProgress = {
  ...first,
  state: 'partial',
  totalDirs: 10,
  doneDirs: 8,
  errorCount: 12,
  errors: [{ path: '/电影/A.strm', stage: 'strm', message: 'Disk full' }],
};
const totals = {
  at: 1700000000,
  dirs: 10,
  files: 83,
  written: 52,
  pruned: 1,
  errors: 12,
  truncated: false,
};

it('keeps both roots through discovery, processing and partial completion', () => {
  const board = (mappings: MappingProgress[], running = true) => (
    <SyncBoard running={running} report={SyncReport.parse({ ...empty, mappings })} history={[]} />
  );
  const view = render(board([first, next]));
  const source = screen.getByText('Drive folder: /电影');
  screen.getByText('Local folder: D:\\媒体库\\剧集');
  screen.getByText('Queued');
  screen.getByText('Mapped roots completed: 0/2');
  screen.getByText('Folders discovered: 7 (total not yet known)');
  expect(screen.queryByRole('progressbar')).toBeNull();
  view.rerender(board([{ ...first, state: 'processing', doneDirs: 3, totalDirs: 10 }, next]));
  screen.getByText('Folders processed: 3/10');
  expect(screen.getByText('Drive folder: /电影')).toBe(source);
  view.rerender(
    board([partial, { ...next, state: 'completed', totalDirs: 1, doneDirs: 1 }], false),
  );
  screen.getByText('Mapped roots completed: 1/2');
  screen.getByText('Partially completed');
  const summary = screen.getByText('Errors: 12');
  fireEvent.click(summary);
  expect(summary.closest('details')?.open).toBe(true);
  screen.getByText('Recorded details: 1 of 12 errors');
  screen.getByText('/电影/A.strm · Write STRM · Disk full');
  fireEvent.click(summary);
  expect(summary.closest('details')?.open).toBe(false);
});

it('restores saved rows after reload and opens error details inside history', () => {
  const entry = CloudHistoryEntry.parse({ ...totals, mappings: [partial] });
  const { container } = render(<SyncBoard running={false} report={empty} history={[entry]} />);
  const [current, history, historicalError] = container.querySelectorAll('summary');
  expect(current?.textContent).toBe('Errors: 12');
  fireEvent.click(history!);
  fireEvent.click(historicalError!);
  expect(history?.closest('details')?.open).toBe(true);
  expect(historicalError?.closest('details')?.open).toBe(true);
  expect(historicalError?.closest('details')?.textContent).toContain(
    '/电影/A.strm · Write STRM · Disk full',
  );
});

it('keeps an error count without recorded details non-interactive', () => {
  const report = SyncReport.parse({ ...empty, mappings: [{ ...partial, errors: [] }] });
  const { container } = render(<SyncBoard running={false} report={report} history={[]} />);
  screen.getByText('Errors: 12 · No error details recorded.');
  expect(container.querySelector('summary, button')).toBeNull();
});

it.each([undefined, []])(
  'uses recorded totals without inventing legacy rows or actions',
  (mappings) => {
    const entry = CloudHistoryEntry.parse({ ...totals, mappings });
    const { container } = render(<SyncBoard running={false} report={empty} history={[entry]} />);
    expect(
      screen.getAllByText('dirs 10 · files 83 · written 52 · pruned 1 · errors 12'),
    ).toHaveLength(2);
    expect(screen.getAllByText('No per-mapping details recorded for this run.')).toHaveLength(2);
    expect(container.querySelector('summary, button')).toBeNull();
    expect(container.textContent).not.toContain('dirs 0');
  },
);
