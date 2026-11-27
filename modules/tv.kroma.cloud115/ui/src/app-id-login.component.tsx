import { useEffect, useRef, useState } from 'react';
import { useT } from '@kromatv/module-sdk';
import { Box, Button, Field, Row, Text } from '@kromatv/ui/kit';
import { useCloud115Api } from './api.hook';

const digits = (value: string) => value.length > 0 && value.length <= 32 && !/[^0-9]/.test(value);
type Notice = 'qrWaiting' | 'qrScanned' | 'qrExpired' | 'qrFailed' | 'qrCancelled';

export function AppIdLogin({ initialId, authorized, disabled, onConfirmed }: {
  initialId: string; authorized: boolean; disabled: boolean; onConfirmed: (id: string) => void;
}) {
  const api = useCloud115Api();
  const t = useT();
  const [id, setId] = useState(digits(initialId) ? initialId : '');
  const [invalid, setInvalid] = useState(false);
  const [busy, setBusy] = useState(false);
  const [reason, setReason] = useState('');
  // Surface only the module's fixed, sanitized provider diagnostic, never an HTML response.
  const showReason = (err: unknown) => {
    const msg = err instanceof Error ? err.message : String(err);
    setReason(msg.match(/115 授权：[\u3400-\u9fffA-Za-z0-9 ，、]+（HTTP \d{3}，代码 \d+）/u)?.[0] ?? '');
  };
  const [image, setImage] = useState('');
  const [notice, setNotice] = useState<Notice | null>(null);
  const current = useRef<AbortController | null>(null);
  useEffect(() => () => current.current?.abort(), []);

  async function start() {
    if (!digits(id) || invalid || disabled || current.current) return;
    const attempt = new AbortController();
    current.current = attempt;
    let flow = '';
    let timer: ReturnType<typeof setTimeout>;
    const cancelRemote = () => { if (flow) void api.loginCancel(flow).catch(() => undefined); };
    const expiry = setTimeout(() => stop('qrExpired'), 5 * 60_000);
    attempt.signal.addEventListener('abort', () => {
      clearTimeout(timer);
      clearTimeout(expiry);
      cancelRemote();
    }, { once: true });
    setBusy(true);
    setReason('');
    setNotice(null);
    try {
      const started = await api.loginStart(id);
      flow = started.flow;
      if (attempt.signal.aborted) { cancelRemote(); return; }
      if (started.appId !== id || !flow || !started.qr || !started.svg) throw new Error();
      // The module encodes the official payload locally with Rust qrcode. Keep its
      // generated SVG in an isolated image, never document markup or an external URL.
      setImage(`data:image/svg+xml;charset=utf-8,${encodeURIComponent(started.svg)}`);
      setNotice('qrWaiting');
      const poll = async () => {
        try {
          const state = await api.loginState(flow);
          if (attempt.signal.aborted) return;
          if (state.status === 'confirmed' && state.authorized) {
            flow = '';
            stop(null);
            onConfirmed(id);
          } else if (state.status === 'pending' || state.status === 'scanned') {
            setNotice(state.status === 'pending' ? 'qrWaiting' : 'qrScanned');
            timer = setTimeout(poll, 2000);
          } else {
            stop(state.status === 'expired' ? 'qrExpired' : state.status === 'cancelled' ? 'qrCancelled' : 'qrFailed');
          }
        } catch { if (!attempt.signal.aborted) stop('qrFailed'); }
      };
      timer = setTimeout(poll, 2000);
    } catch (err) { if (!attempt.signal.aborted) { showReason(err); stop('qrFailed'); } }
  }
  function stop(next: Notice | null) {
    current.current?.abort();
    current.current = null;
    setBusy(false);
    setImage('');
    setNotice(next);
  }
  return <Box gap={10} mt={20}>
    <Field.Root label="AppID" error={invalid ? t('cloud115.qrHint') : undefined} value={id} onValueChange={(value) => {
      // Reject a pasted credential as a whole; never echo it or extract its digits.
      setInvalid(Boolean(value) && !digits(value));
      setId(digits(value) ? value : '');
      setNotice(null);
    }}>
      <Field.Input physicalKeyboard type="number" readOnly={busy || disabled} autoComplete="off" />
      <Field.Hint>{t('cloud115.qrHint')}</Field.Hint>
    </Field.Root>
    <Row wrap gap={8}>
      <Button label={t(authorized ? 'cloud115.relogin' : 'cloud115.qrLogin')} icon="qrcode"
        variant="glass" size="sm" disabled={disabled || busy || !digits(id) || invalid} onPress={() => void start()} />
      {busy && <Button label={t('cloud115.qrCancel')} size="sm" variant="ghost" onPress={() => stop('qrCancelled')} />}
    </Row>
    {image && <img width={220} height={220} src={image} alt={t('cloud115.qrLogin')} onError={() => stop('qrFailed')} />}
    {notice && <Box accessibilityLiveRegion="polite"><Text variant="meta">{notice === 'qrFailed' && reason ? reason : t(`cloud115.${notice}`)}</Text></Box>}
  </Box>;
}
