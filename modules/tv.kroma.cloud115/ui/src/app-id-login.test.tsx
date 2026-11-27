// @vitest-environment jsdom
import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { AppIdLogin } from './app-id-login.component';
import { CloudLibrary, type LoginState } from './schemas';

const api = vi.hoisted(() => ({ loginStart: vi.fn(), loginCancel: vi.fn(), loginState: vi.fn() }));
vi.mock('./api.hook', () => ({ useCloud115Api: () => api }));
vi.mock('@kromatv/module-sdk', () => ({ useT: () => (key: string) => key }));
const start = { flow: 'test-flow', qr: 'non-display-payload', svg: '<svg/>', appId: '0012' };
function setup() {
  vi.useFakeTimers();
  api.loginStart.mockResolvedValue(start);
  api.loginCancel.mockResolvedValue({});
  const confirmed = vi.fn();
  const view = render(<AppIdLogin initialId="" authorized={false} disabled={false} onConfirmed={confirmed} />);
  const scan = () => screen.getByRole('button', { name: 'cloud115.qrLogin' });
  const input = screen.getByLabelText('AppID', { selector: 'input' });
  return { confirmed, view, scan, input };
}
afterEach(() => { vi.clearAllMocks(); vi.useRealTimers(); });

it('rejects mixed and non-ASCII input without sending it; preserves leading zeros', async () => {
  const { scan, input } = setup();
  for (const value of ['１２', 'A12', '1'.repeat(33)]) {
    fireEvent.change(input, { target: { value } });
    fireEvent.click(scan());
    expect(api.loginStart).not.toHaveBeenCalled();
  }
  fireEvent.change(input, { target: { value: '0012' } });
  await act(async () => fireEvent.click(scan()));
  expect(api.loginStart).toHaveBeenCalledWith('0012');
  expect(screen.getByRole('img').getAttribute('src')).toMatch(/^data:image\/svg\+xml/);
  expect(screen.queryByText(start.qr)).toBeNull();
});

it('ignores late confirmation after cancel, and can retry an expired scan', async () => {
  const { confirmed, scan, input } = setup();
  let resolve!: (state: LoginState) => void;
  api.loginState.mockImplementationOnce(() => new Promise(r => { resolve = r; }));
  fireEvent.change(input, { target: { value: '0012' } });
  await act(async () => fireEvent.click(scan()));
  await act(async () => vi.advanceTimersByTimeAsync(2000));
  fireEvent.click(screen.getByRole('button', { name: 'cloud115.qrCancel' }));
  expect(api.loginCancel).toHaveBeenCalledWith(start.flow);
  await act(async () => resolve({ status: 'confirmed', authorized: true, message: '' }));
  expect(confirmed).not.toHaveBeenCalled();
  api.loginState.mockResolvedValueOnce({ status: 'expired', authorized: false, message: 'hidden' });
  await act(async () => fireEvent.click(scan()));
  await act(async () => vi.advanceTimersByTimeAsync(2000));
  screen.getByText('cloud115.qrExpired');
  expect(screen.queryByRole('img')).toBeNull();
  expect(screen.queryByText('hidden')).toBeNull();
  api.loginState.mockResolvedValueOnce({ status: 'confirmed', authorized: true });
  await act(async () => fireEvent.click(scan()));
  await act(async () => vi.advanceTimersByTimeAsync(2000));
  expect(confirmed).toHaveBeenCalledWith('0012');
});

it('includes existing mappings in quick checks unless explicitly disabled', () => {
  const mapping = { remote: '/Movies', local: '/selected/folder' };
  expect(CloudLibrary.parse(mapping).fastSync).toBe(true);
  expect(CloudLibrary.parse({ ...mapping, fastSync: false }).fastSync).toBe(false);
});
