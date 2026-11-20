// Admin "115 Cloud" page: the account, the folder mappings and the mirror's
// progress. Backed by this module's `/settings`, `/status` and `/sync` routes.
//
// The page never sees the stored cookie: it is told only whether one is set. A
// blank field keeps it, and clearing takes an explicit button, so autofill or a
// stray save cannot sign the module out of the account.
import { Denied, ModuleFailed, ModuleLoading, useCap, useT } from '@kromatv/module-sdk';
import {
  Badge,
  Box,
  Button,
  Field,
  Icon,
  PageHeader,
  Progress,
  Row,
  Section,
  Surface,
  Switch,
  Text,
} from '@kromatv/ui/kit';
import { type CSSProperties, useEffect, useRef, useState } from 'react';
import { useCloud115Api } from './api.hook';
import type {
  BrowseEntry,
  BrowseView,
  CloudAccount,
  CloudLibrary,
  CloudOptions,
  CloudStatusView,
  LoginStarted,
  LoginState,
  SyncReport,
} from './schemas';

const LOGIN_APPS = [
  { value: 'alipaymini', label: '支付宝小程序' },
  { value: 'wechatmini', label: '微信小程序' },
  { value: 'tv', label: '电视端' },
  { value: 'apple_tv', label: 'Apple TV 端' },
  { value: 'harmony', label: '鸿蒙端' },
  { value: 'os_linux', label: 'Linux 客户端' },
  { value: 'os_mac', label: 'Mac 客户端' },
  { value: 'os_windows', label: 'Windows 客户端' },
  { value: 'web', label: '网页端' },
] as const;

const APP_SELECT: CSSProperties = {
  background: 'var(--kroma-surface-1)',
  border: '1px solid var(--kroma-border)',
  borderRadius: 'var(--radius-md)',
  padding: '10px 12px',
  color: 'var(--kroma-text)',
  font: 'var(--type-body)',
  minWidth: 180,
};

const ERRORS_PANE: CSSProperties = {
  margin: 0,
  marginTop: 12,
  maxHeight: 220,
  overflow: 'auto',
  padding: 12,
  borderRadius: 'var(--radius-sm)',
  border: '1px solid var(--kroma-border)',
  background: 'var(--kroma-bg)',
  font: 'var(--type-meta)',
  fontFamily: 'var(--font-mono)',
  color: 'var(--kroma-text-muted)',
  whiteSpace: 'pre-wrap',
};

const GRID_ROW: CSSProperties = {
  display: 'flex',
  flexWrap: 'wrap',
  gap: 12,
  alignItems: 'flex-end',
  marginBottom: 12,
};

const CELL: CSSProperties = { flex: '1 1 180px', minWidth: 160 };
const SUMMARY_GRID: CSSProperties = {
  display: 'grid',
  gridTemplateColumns: 'minmax(220px, 1.2fr) repeat(3, minmax(140px, 1fr))',
  gap: 22,
  alignItems: 'start',
};
const MAPPING_CARD: CSSProperties = {
  border: '1px solid var(--kroma-border)',
  borderRadius: 'var(--radius-md)',
  padding: '16px 18px',
  background: 'var(--kroma-surface-2)',
};

const MAPPING_GRID: CSSProperties = {
  display: 'grid',
  gridTemplateColumns: 'repeat(auto-fit, minmax(240px, 1fr))',
  gap: 18,
  alignItems: 'start',
};

const READONLY_VALUE: CSSProperties = {
  border: '1px solid var(--kroma-border)',
  borderRadius: 'var(--radius-md)',
  padding: '10px 12px',
  background: 'var(--kroma-surface-1)',
  minHeight: 40,
  display: 'flex',
  alignItems: 'center',
  marginTop: 6,
  overflow: 'hidden',
};

const READONLY_TEXT: CSSProperties = {
  minWidth: 0,
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
  font: 'var(--type-body)',
  color: 'var(--kroma-text)',
};

const LINK_TEXT: CSSProperties = {
  marginTop: 6,
  font: 'var(--type-meta)',
  fontFamily: 'var(--font-mono)',
  color: 'var(--kroma-text-muted)',
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
};

const SAVE_BAR: CSSProperties = {
  position: 'sticky',
  bottom: 0,
  zIndex: 5,
  display: 'flex',
  flexWrap: 'wrap',
  alignItems: 'center',
  gap: 12,
  marginTop: 26,
  padding: '14px 0',
  borderTop: '1px solid var(--kroma-border)',
  background: 'var(--kroma-bg)',
};

const EMPTY_LIBRARY: CloudLibrary = { remote: '', local: '' };

/** The account, the mappings, the pacing and the run controls. Contributed both
 *  as this module's settings panel (inside its card on the modules page) and as
 *  the top half of its own page, so the same controls are never built twice. */
export function Cloud115Settings() {
  const t = useT();
  const api = useCloud115Api();
  const canManage = useCap('settings.manage');

  const [status, setStatus] = useState<CloudStatusView | null>(null);
  const [account, setAccount] = useState<CloudAccount | null>(null);
  const [baseUrl, setBaseUrl] = useState('');
  const [cookie, setCookie] = useState('');
  const [hasCookie, setHasCookie] = useState(false);
  const [libraries, setLibraries] = useState<CloudLibrary[]>([]);
  const [options, setOptions] = useState<CloudOptions | null>(null);
  const [resolved, setResolved] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState(false);
  const [savedAt, setSavedAt] = useState<Date | null>(null);
  const [dirty, setDirty] = useState(false);
  const [copiedLink, setCopiedLink] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const loaded = useRef(false);
  const [picker, setPicker] = useState<{ at: number; mode: 'remote' | 'local' } | null>(null);
  const [loginApp, setLoginApp] = useState<string>('alipaymini');
  const [login, setLogin] = useState<LoginStarted | null>(null);
  const [loginState, setLoginState] = useState<LoginState | null>(null);
  const [loginBusy, setLoginBusy] = useState(false);
  const [uploadRemote, setUploadRemote] = useState('');
  const [uploadPath, setUploadPath] = useState('');
  const [uploadMsg, setUploadMsg] = useState<string | null>(null);
  const [uploadBusy, setUploadBusy] = useState(false);
  const origin = typeof window === 'undefined' ? '' : window.location.origin;
  const baseMismatch = Boolean(baseUrl) && baseUrl.replace(/\/$/, '') !== origin;

  useEffect(() => {
    api
      .settings()
      .then((view) => {
        if (loaded.current) {
          return;
        }
        loaded.current = true;
        setBaseUrl(view.baseUrl);
        setHasCookie(view.hasCookie);
        setLibraries(view.libraries.length ? view.libraries : [{ ...EMPTY_LIBRARY }]);
        setOptions(view.options);
        setResolved(view.resolvedLocal);
      })
      .catch(() => setError(t('cloud115.loadFailed')));

    const pull = () => {
      api
        .status()
        .then((view) => {
          setStatus(view);
          setHasCookie(view.cookieSet);
        })
        .catch(() => undefined);
    };
    pull();
    const id = setInterval(pull, 3000);
    api
      .account()
      .then(setAccount)
      .catch(() => undefined);
    return () => clearInterval(id);
  }, [api, t]);

  // Poll a scan while it is open; a confirmed one has already stored the
  // cookies server-side, so the page only has to re-read its settings.
  useEffect(() => {
    if (!login) {
      return;
    }
    const id = setInterval(() => {
      api
        .loginState(login.flow)
        .then((view) => {
          setLoginState(view);
          if (view.status === 'confirmed') {
            setLogin(null);
            setHasCookie(true);
            setSaved(false);
            setDirty(false);
            api.settings().then((settings) => setResolved(settings.resolvedLocal)).catch(() => undefined);
            api.account().then(setAccount).catch(() => undefined);
          } else if (view.status === 'expired' || view.status === 'failed') {
            setLogin(null);
          }
        })
        .catch(() => undefined);
    }, 2000);
    return () => clearInterval(id);
  }, [api, login]);

  const startLogin = () => {
    setLoginBusy(true);
    setLoginState(null);
    api
      .loginStart(loginApp)
      .then(setLogin)
      .catch((reason) => setError(reason instanceof Error ? reason.message : String(reason)))
      .finally(() => setLoginBusy(false));
  };

  const cancelLogin = () => {
    if (login) {
      void api.loginCancel(login.flow).catch(() => undefined);
    }
    setLogin(null);
  };

  const rapidCheck = async () => {
    const remote = uploadRemote.trim();
    const path = uploadPath.trim();
    if (!remote || !path) {
      setUploadMsg(t('cloud115.uploadMissing'));
      return;
    }
    setUploadBusy(true);
    setUploadMsg(null);
    try {
      const result = await api.uploadRapid(remote, path);
      setUploadMsg(
        result.hit
          ? t('cloud115.uploadHit', { pickcode: result.pickcode ?? '' })
          : t('cloud115.uploadRejected', { status: result.status }),
      );
    } catch (reason) {
      setUploadMsg(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setUploadBusy(false);
    }
  };

  const uploadFile = async () => {
    const remote = uploadRemote.trim();
    const path = uploadPath.trim();
    if (!remote || !path) {
      setUploadMsg(t('cloud115.uploadMissing'));
      return;
    }
    setUploadBusy(true);
    setUploadMsg(t('cloud115.uploadStarting'));
    try {
      const started = await api.uploadStart(remote, path);
      if (started.hit) {
        setUploadMsg(t('cloud115.uploadHit', { pickcode: started.pickcode ?? '' }));
        return;
      }
      if (!started.upload) {
        setUploadMsg(t('cloud115.uploadRejected', { status: -1 }));
        return;
      }
      for (let index = 0; index < started.parts; index += 1) {
        setUploadMsg(
          t('cloud115.uploadProgress', {
            done: index + 1,
            total: started.parts,
          }),
        );
        await api.uploadPart(started.upload, index);
      }
      setUploadMsg(t('cloud115.uploadMerging'));
      const done = await api.uploadFinish(started.upload);
      setUploadMsg(t('cloud115.uploadDone', { pickcode: done.pickcode }));
    } catch (reason) {
      setUploadMsg(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setUploadBusy(false);
    }
  };

  if (!canManage) {
    return <Denied />;
  }
  if (!options || !status) {
    return error ? <ModuleFailed /> : <ModuleLoading panels={3} />;
  }

  const updateLibrary = (at: number, patch: Partial<CloudLibrary>) => {
    setDirty(true);
    setLibraries((current) => current.map((item, i) => (i === at ? { ...item, ...patch } : item)));
  };

  const patchOptions = (patch: Partial<CloudOptions>) => {
    setDirty(true);
    setOptions((current) => (current ? { ...current, ...patch } : current));
  };

  const save = async () => {
    setBusy(true);
    setSaved(false);
    setError(null);
    try {
      const kept = libraries.filter((l) => l.remote.trim() || l.local.trim());
      if (kept.length === 0) {
        // Never let a save erase every mapping: an empty list is what a page
        // that lost its rows submits by accident.
        setError(t('cloud115.cannotClearMappings'));
        return;
      }
      const result = await api.save({
        baseUrl,
        libraries: kept,
        options,
        ...(cookie.trim() ? { cookie: cookie.trim() } : {}),
      });
      setCookie('');
      setHasCookie(hasCookie || Boolean(cookie.trim()));
      setDirty(false);
      setSaved(true);
      setSavedAt(new Date());
      // The save also creates each mapping's folder; a path outside the storage
      // roots fails here, and that must be said now rather than at the next sync.
      const view = await api.settings();
      setResolved(view.resolvedLocal);
      if (result.errors.length) {
        setSaved(false);
        setError(result.errors.join('；'));
      } else if (view.resolvedLocal.some((path) => !path)) {
        setError(t('cloud115.unresolved'));
      }
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  };

  const importCookie = async () => {
    setBusy(true);
    setError(null);
    try {
      const result = await api.loginCookie(cookie.trim());
      if (result.ok) {
        setCookie('');
        setHasCookie(true);
        setSaved(true);
        setSavedAt(new Date());
      } else {
        setError(result.message);
      }
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  };

  const clearCookie = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.save({ clearCookie: true, baseUrl, libraries, options });
      setHasCookie(false);
      setDirty(false);
      setSaved(true);
      setSavedAt(new Date());
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  };

  const syncNow = async () => {
    setBusy(true);
    setError(null);
    try {
      // The browser is the one side that knows the origin the server is reached
      // at; the .strm entries are built from it.
      await api.sync(window.location.origin);
      api
        .account()
        .then(setAccount)
        .catch(() => undefined);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  };

  const report = status.report;

  return (
    <>
      {status.pausedSecs ? (
        <Surface elevated border="border" pad="none" px={22} py={16} mt={20}>
          <Text variant="meta" color="accentText">
            {t('cloud115.paused', { seconds: status.pausedSecs })}
          </Text>
        </Surface>
      ) : null}

      <Section.Root mt={26}>
        <Section.Header>
          <Section.Title>{t('cloud115.account')}</Section.Title>
        </Section.Header>
        <Surface elevated border="border" pad="none" px={22} py={20}>
          <div style={SUMMARY_GRID}>
            <Box>
              <Text variant="h2">115 Cloud</Text>
              <Row gap={8} center mt={8}>
                <Badge tone={hasCookie ? 'success' : 'warning'}>
                  {hasCookie ? t('cloud115.cookieSet') : t('cloud115.cookieMissing')}
                </Badge>
                <Text variant="meta" color="textDim">
                  {baseUrl || origin || '—'}
                </Text>
              </Row>
            </Box>
            <SummaryStat label={t('cloud115.mappings')} value={String(libraries.filter((l) => l.remote && l.local).length)} />
            <SummaryStat label={t('cloud115.indexedDirs')} value={String(status.indexedDirs)} />
            <SummaryStat label={t('cloud115.statLinks')} value={String(status.linkCache)} />
          </div>
          {account ? (
            <Row wrap gapX={32} gapY={8} mt={18}>
              <Stat label={t('cloud115.spaceTotal')} value={account.totalText} />
              <Stat label={t('cloud115.spaceUsed')} value={account.usedText} />
              <Stat label={t('cloud115.spaceRemain')} value={account.remainText} />
            </Row>
          ) : null}
          <div style={{ ...GRID_ROW, marginTop: 24, marginBottom: 0 }}>
            <div style={CELL}>
              <Field.Root label={t('cloud115.cookie')} value={cookie} onValueChange={(value) => { setDirty(true); setCookie(value); }}>
                <Field.Input
                  type="password"
                  placeholder={hasCookie ? t('cloud115.cookieKeep') : 'UID=…; CID=…; SEID=…'}
                />
                <Field.Hint>{t('cloud115.cookieHint')}</Field.Hint>
              </Field.Root>
            </div>
            <div style={CELL}>
              <Field.Root label={t('cloud115.baseUrl')} value={baseUrl} onValueChange={(value) => { setDirty(true); setBaseUrl(value); }}>
                <Field.Input placeholder="http://192.168.1.10:4040" />
                <Field.Hint>{t('cloud115.baseUrlHint')}</Field.Hint>
              </Field.Root>
            </div>
            <Row gap={8}>
              {cookie.trim() ? (
                <Button
                  label={t('cloud115.cookieImport')}
                  variant="glass"
                  size="sm"
                  onPress={() => void importCookie()}
                  disabled={busy}
                />
              ) : null}
              {hasCookie ? (
                <Button label={t('cloud115.clearCookie')} variant="ghost" size="sm" onPress={() => void clearCookie()} />
              ) : null}
            </Row>
          </div>
          <Row wrap gap={10} mt={18} center>
            <select
              value={loginApp}
              onChange={(event) => setLoginApp(event.target.value)}
              style={APP_SELECT}
              aria-label={t('cloud115.qrApp')}
            >
              {LOGIN_APPS.map((app) => (
                <option key={app.value} value={app.value}>
                  {app.label}
                </option>
              ))}
            </select>
            <Button
              label={t('cloud115.qrLogin')}
              icon="qrcode"
              variant="glass"
              size="sm"
              onPress={startLogin}
              disabled={loginBusy || login !== null}
            />
            <Text variant="meta" color="textDim">
              {t('cloud115.qrHint')}
            </Text>
          </Row>
          {login ? (
            <Row wrap gap={20} mt={14} align="flex-start">
              <img
                src={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(login.svg)}`}
                alt={t('cloud115.qrLogin')}
                style={{ width: 220, height: 220, background: '#fff', padding: 8, borderRadius: 12 }}
              />
              <Box gap={8}>
                <Text variant="meta" color="textDim">
                  {loginState?.message ?? t('cloud115.qrWaiting')}
                </Text>
                <Row gap={8}>
                  <Button
                    label={t('cloud115.qrCancel')}
                    variant="ghost"
                    size="sm"
                    onPress={cancelLogin}
                  />
                </Row>
                <div style={{ maxWidth: 320, wordBreak: 'break-all' }}>
                  <Text variant="meta" color="textDim">
                    {login.qr}
                  </Text>
                </div>
              </Box>
            </Row>
          ) : null}
          {baseMismatch ? (
            <Row wrap gap={10} mt={10} center>
              <Text variant="meta" color="accentText">{t('cloud115.baseMismatch')}</Text>
              <Button label={t('cloud115.useCurrent')} variant="ghost" size="sm" onPress={() => setBaseUrl(origin)} />
            </Row>
          ) : null}
        </Surface>
      </Section.Root>

      <Section.Root mt={28}>
        <Section.Header>
          <Section.Title>{t('cloud115.mappings')}</Section.Title>
        </Section.Header>
        <Text variant="meta" color="textDim" mt={-8} mb={16}>
          {t('cloud115.mappingsHint')}
        </Text>
        <Surface elevated border="border" pad="none" px={22} py={16}>
          <Box gap={12}>
            {libraries.map((library, i) => (
              <div style={MAPPING_CARD} key={i}>
                <div style={MAPPING_GRID}>
                  <div style={{ minWidth: 0 }}>
                    <Text variant="meta" color="textDim">{t('cloud115.remote')}</Text>
                    <div style={READONLY_VALUE} title={library.remote || '—'}>
                      <span style={READONLY_TEXT}>{library.remote || '—'}</span>
                    </div>
                    <Row mt={8}>
                      <Button label={t('cloud115.browseDrive')} icon="folder" variant="glass" size="sm" onPress={() => setPicker({ at: i, mode: 'remote' })} disabled={busy} />
                    </Row>
                  </div>
                  <div style={{ minWidth: 0 }}>
                    <Field.Root label={t('cloud115.local')} value={library.local} onValueChange={(value) => updateLibrary(i, { local: value })}>
                      <Field.Input placeholder="/media/115/电影" />
                      <Field.Hint>{t('cloud115.localHint')}</Field.Hint>
                    </Field.Root>
                    <Row mt={8}>
                      <Button label={t('cloud115.browseLocal')} icon="folder" variant="ghost" size="sm" onPress={() => setPicker({ at: i, mode: 'local' })} disabled={busy} />
                    </Row>
                    {!library.remote.trim() || !library.local.trim() ? null : i < resolved.length ? (
                      resolved[i] ? <Text variant="meta" color="textDim" mt={6}>{t('cloud115.resolved', { path: resolved[i] as string })}</Text> : <Text variant="meta" color="accentText" mt={6}>{t('cloud115.unresolved')}</Text>
                    ) : <Text variant="meta" color="textDim" mt={6}>{t('cloud115.willCreate')}</Text>}
                  </div>
                </div>
                <Row justify="flex-end" mt={10}>
                  <Button label={t('cloud115.remove')} variant="ghost" size="sm" onPress={() => { setDirty(true); setLibraries((current) => current.filter((_, at) => at !== i)); }} />
                </Row>
              </div>
            ))}
            <Row mt={4}>
              <Button label={t('cloud115.addMapping')} icon="plus" variant="ghost" size="sm" onPress={() => { setDirty(true); setLibraries((current) => [...current, { ...EMPTY_LIBRARY }]); }} disabled={busy} />
            </Row>
          </Box>
        </Surface>
      </Section.Root>

      <Section.Root mt={28}>
        <Section.Header>
          <Section.Title>{t('cloud115.upload')}</Section.Title>
        </Section.Header>
        <Text variant="meta" color="textDim" mt={-8} mb={16}>
          {t('cloud115.uploadHint')}
        </Text>
        <Surface elevated border="border" pad="none" px={22} py={20}>
          <div style={GRID_ROW}>
            <div style={CELL}>
              <Field.Root
                label={t('cloud115.uploadRemote')}
                value={uploadRemote}
                onValueChange={setUploadRemote}
              >
                <Field.Input placeholder="/电影" />
                <Field.Hint>{t('cloud115.uploadRemoteHint')}</Field.Hint>
              </Field.Root>
            </div>
            <div style={CELL}>
              <Field.Root
                label={t('cloud115.uploadPath')}
                value={uploadPath}
                onValueChange={setUploadPath}
              >
                <Field.Input placeholder="/media/电影/xxx.mkv" />
                <Field.Hint>{t('cloud115.uploadPathHint')}</Field.Hint>
              </Field.Root>
            </div>
          </div>
          <Row wrap gap={8} mt={10} center>
            <Button
              label={t('cloud115.uploadRapid')}
              variant="ghost"
              size="sm"
              onPress={() => void rapidCheck()}
              disabled={uploadBusy}
            />
            <Button
              label={t('cloud115.uploadFile')}
              icon="upload"
              variant="glass"
              size="sm"
              onPress={() => void uploadFile()}
              disabled={uploadBusy}
            />
            {uploadMsg ? (
              <Text variant="meta" color="textDim">
                {uploadMsg}
              </Text>
            ) : null}
          </Row>
        </Surface>
      </Section.Root>

      <Section.Root mt={28}>
        <Section.Header>
          <Section.Title>{t('cloud115.watch')}</Section.Title>
        </Section.Header>
        <Text variant="meta" color="textDim" mt={-8} mb={16}>
          {t('cloud115.watchHint')}
        </Text>
        <Surface elevated border="border" pad="none" px={22} py={20}>
          <Row between gap={16} mb={16}>
            <Box shrink={1} minW={0}>
              <Text variant="cardTitle">{t('cloud115.watch')}</Text>
              <Text variant="meta" color="textDim" mt={2}>
                {t('cloud115.watchHint')}
              </Text>
            </Box>
            <Switch
              checked={options.watchEnabled}
              onCheckedChange={(watchEnabled) => patchOptions({ watchEnabled })}
              label={t('cloud115.watch')}
            />
          </Row>
          <div style={CELL}>
            <NumberField
              label={t('cloud115.watchInterval')}
              value={options.watchIntervalSecs}
              onChange={(watchIntervalSecs) => patchOptions({ watchIntervalSecs })}
            />
          </div>
          <Text variant="meta" color={status.gateOk === false ? 'danger' : 'textDim'} mt={10}>
            {status.lastPollAt
              ? t('cloud115.lastPoll', {
                  time: new Date(status.lastPollAt * 1000).toLocaleTimeString(),
                })
              : t('cloud115.neverPolled')}
            {status.gateOk === false ? ` · ${t('cloud115.gateOff')}` : ''}
          </Text>
          {status.watchEnabled && status.indexedDirs === 0 ? (
            <Text variant="meta" color="accentText" mt={6}>
              {t('cloud115.indexNotReady')}
            </Text>
          ) : null}
        </Surface>
      </Section.Root>

      <Section.Root mt={28}>
        <Section.Header>
          <Section.Title>{t('cloud115.pacing')}</Section.Title>
        </Section.Header>
        <Text variant="meta" color="textDim" mt={-8} mb={16}>
          {t('cloud115.pacingHint')}
        </Text>
        <Surface elevated border="border" pad="none" px={22} py={20}>
          <div style={GRID_ROW}>
            <div style={CELL}>
              <NumberField
                label={t('cloud115.interval')}
                value={options.intervalMs}
                onChange={(intervalMs) => patchOptions({ intervalMs })}
              />
            </div>
            <div style={CELL}>
              <NumberField
                label={t('cloud115.budget')}
                value={options.maxCallsPerRun}
                onChange={(maxCallsPerRun) => patchOptions({ maxCallsPerRun })}
              />
            </div>
            <div style={CELL}>
              <NumberField
                label={t('cloud115.folderCap')}
                value={options.maxEntriesPerFolder}
                onChange={(maxEntriesPerFolder) => patchOptions({ maxEntriesPerFolder })}
              />
            </div>
            <div style={CELL}>
              <NumberField
                label={t('cloud115.linkTtl')}
                value={options.linkTtlSecs}
                onChange={(linkTtlSecs) => patchOptions({ linkTtlSecs })}
              />
            </div>
            <div style={CELL}>
              <NumberField
                label={t('cloud115.pageSize')}
                value={options.pageSize}
                onChange={(pageSize) => patchOptions({ pageSize })}
              />
            </div>
          </div>
          <Row between gap={16} mt={14}>
            <Box shrink={1} minW={0}>
              <Text variant="cardTitle">{t('cloud115.skipIso')}</Text>
              <Text variant="meta" color="textDim" mt={2}>
                {t('cloud115.skipIsoHint')}
              </Text>
            </Box>
            <Switch
              checked={options.skipIso}
              onCheckedChange={(skipIso) => patchOptions({ skipIso })}
              label={t('cloud115.skipIso')}
            />
          </Row>
          <Row between gap={16} mt={14}>
            <Box shrink={1} minW={0}>
              <Text variant="cardTitle">{t('cloud115.probe')}</Text>
              <Text variant="meta" color="textDim" mt={2}>
                {t('cloud115.probeHint')}
              </Text>
            </Box>
            <Switch
              checked={options.probeEnabled}
              onCheckedChange={(probeEnabled) => patchOptions({ probeEnabled })}
              label={t('cloud115.probe')}
            />
          </Row>
        </Surface>
      </Section.Root>

      <div style={SAVE_BAR}>
        <Button
          label={busy ? t('cloud115.saving') : t('cloud115.save')}
          icon="device-floppy"
          variant="primary"
          size="sm"
          onPress={() => void save()}
          disabled={busy || !dirty}
        />
        <Button
          label={t('cloud115.syncNow')}
          icon="refresh"
          variant="glass"
          size="sm"
          onPress={() => void syncNow()}
          disabled={busy || status.running}
        />
        {dirty ? (
          <Text variant="meta" color="accentText">
            {t('cloud115.unsaved')}
          </Text>
        ) : null}
        {!dirty && saved ? (
          <Text variant="meta" color="success">
            {t('cloud115.saved')}
            {savedAt ? ` · ${savedAt.toLocaleTimeString()}` : ''}
          </Text>
        ) : null}
        {error ? (
          <Text variant="meta" color="danger">
            {error}
          </Text>
        ) : null}
      </div>

      <Section.Root mt={28}>
        <Section.Header>
          <Section.Title>
            {status.running ? t('cloud115.syncProgress') : t('cloud115.lastRun')}
          </Section.Title>
        </Section.Header>
        <Surface elevated border="border" pad="none" px={22} py={20}>
          {status.running ? (
            <Row wrap gapX={18} gapY={4} mb={10}>
              <Text variant="meta" color="accentText">
                {t('cloud115.syncProgress')}
              </Text>
              {report.current ? (
                <Text variant="meta" color="textDim">
                  {t('cloud115.currentDir', { dir: report.current })}
                </Text>
              ) : null}
              {report.startedAt ? (
                <Text variant="meta" color="textDim">
                  {t('cloud115.elapsed', {
                    seconds: Math.max(0, Math.floor(Date.now() / 1000) - report.startedAt),
                  })}
                </Text>
              ) : null}
            </Row>
          ) : null}
          {status.running && report.callsTotal > 0 ? (
            <Box mb={10}>
              <Progress
                value={report.callsTotal > 0 ? report.calls / report.callsTotal : 0}
                label={t('cloud115.budgetProgress')}
              />
              <Row between mt={4}>
                <Text variant="meta" color="textDim">
                  {t('cloud115.budgetLine', {
                    calls: report.calls,
                    total: report.callsTotal,
                  })}
                </Text>
                <Text variant="meta" color="textDim">
                  {t('cloud115.statFilesRate', {
                    rate: (
                      report.startedAt
                        ? report.files / Math.max(1, Date.now() / 1000 - report.startedAt)
                        : 0
                    ).toFixed(1),
                  })}
                </Text>
              </Row>
            </Box>
          ) : null}
          <Row wrap gapX={24} gapY={6}>
            <Stat label={t('cloud115.statDirs')} value={report.dirs} />
            <Stat label={t('cloud115.statFiles')} value={report.files} />
            <Stat label={t('cloud115.statWritten')} value={report.written} />
            <Stat label={t('cloud115.statKept')} value={report.kept} />
            <Stat label={t('cloud115.statPruned')} value={report.pruned} />
            <Stat label={t('cloud115.statProbed')} value={report.probed} />
            <Stat label={t('cloud115.statPendingProbe')} value={report.pendingProbe} />
            <Stat label={t('cloud115.statAssets')} value={report.assets} />
            <Stat label={t('cloud115.statCalls')} value={report.calls} />
            <Stat label={t('cloud115.statLinks')} value={status.linkCache} />
          </Row>
          {status.history.length ? (
            <>
              <Text variant="cardTitle" mt={18}>
                {t('cloud115.history')}
              </Text>
              <Box gap={6} mt={8}>
                {[...status.history]
                  .reverse()
                  .slice(0, 10)
                  .map((entry) => (
                    <Row between wrap gap={12} key={entry.at}>
                      <Text variant="meta" color="textDim">
                        {new Date(entry.at * 1000).toLocaleString()}
                      </Text>
                      <Text variant="meta" color={entry.errors ? 'warning' : 'textDim'}>
                        {t('cloud115.historyLine', {
                          dirs: entry.dirs,
                          files: entry.files,
                          written: entry.written,
                          pruned: entry.pruned,
                          errors: entry.errors,
                        })}
                      </Text>
                    </Row>
                  ))}
              </Box>
            </>
          ) : null}
          {report.truncated ? (
            <Text variant="meta" color="warning" mt={8}>
              {t('cloud115.truncated')}
            </Text>
          ) : null}
          {status.lastPlayError ? (
            <Text variant="meta" color="danger" mt={8}>
              {t('cloud115.lastPlayError')}：{status.lastPlayError}
            </Text>
          ) : null}
          {report.errors.length ? (
            <pre style={ERRORS_PANE}>{report.errors.join('\n')}</pre>
          ) : (
            <Text variant="meta" color="textDim" mt={10}>
              {report.finishedAt ? t('cloud115.noErrors') : t('cloud115.never')}
            </Text>
          )}
        </Surface>
      </Section.Root>

      <Section.Root mt={28}>
        <Section.Header>
          <Section.Title>{t('cloud115.playLog')}</Section.Title>
        </Section.Header>
        <Text variant="meta" color="textDim" mt={-8} mb={16}>
          {t('cloud115.playLogHint')}
        </Text>
        <Surface elevated border="border" pad="none" px={22} py={20}>
          <Row wrap gapX={24} gapY={8}>
            <Stat label={t('cloud115.playMode')} value={status.lastPlayMode || '—'} />
            <Stat label={t('cloud115.playRequests')} value={status.playCount} />
            <Stat label={t('cloud115.signCount')} value={status.signCount} />
            <Stat
              label={t('cloud115.cacheHits')}
              value={Math.max(0, status.playCount - status.signCount)}
            />
            <Stat
              label={t('cloud115.lastPlay')}
              value={
                status.lastPlayAt
                  ? new Date(status.lastPlayAt * 1000).toLocaleString()
                  : t('cloud115.never')
              }
            />
          </Row>
          {status.lastPlayUrl ? (
            <Row between gap={12} mt={14} center>
              <Box minW={0} shrink={1}>
                <Text variant="meta" color="textDim">
                  {t('cloud115.lastLink')}
                </Text>
                <div style={LINK_TEXT} title={status.lastPlayUrl}>
                  {status.lastPlayUrl}
                </div>
              </Box>
              <Button
                label={copiedLink ? t('cloud115.copied') : t('cloud115.playCopy')}
                icon="copy"
                variant="ghost"
                size="sm"
                onPress={() => {
                  void navigator.clipboard
                    .writeText(status.lastPlayUrl)
                    .then(() => {
                      setCopiedLink(true);
                      setTimeout(() => setCopiedLink(false), 1500);
                    })
                    .catch(() => setError(t('cloud115.copyFailed')));
                }}
              />
            </Row>
          ) : null}
        </Surface>
      </Section.Root>

      {picker !== null ? (
        <FolderPicker
          mode={picker.mode}
          onClose={() => setPicker(null)}
          onPick={(path) => {
            updateLibrary(picker.at, picker.mode === 'remote' ? { remote: path } : { local: path });
            setPicker(null);
          }}
        />
      ) : null}
    </>
  );
}

function NumberField({
  label,
  value,
  onChange,
}: Readonly<{ label: string; value: number; onChange: (next: number) => void }>) {
  return (
    <Field.Root
      label={label}
      value={String(value)}
      onValueChange={(text) => {
        const parsed = Number.parseInt(text.replace(/[^0-9]/g, ''), 10);
        if (Number.isFinite(parsed)) {
          onChange(parsed);
        } else if (text.trim() === '') {
          onChange(0);
        }
      }}
    >
      <Field.Input />
    </Field.Root>
  );
}

function SummaryStat({ label, value }: Readonly<{ label: string; value: string }>) {
  return (
    <Box>
      <Text variant="meta" color="textDim">{label}</Text>
      <Text variant="h2" mt={4}>{value}</Text>
    </Box>
  );
}

function Stat({ label, value }: Readonly<{ label: string; value: number | string }>) {
  return (
    <Row gap={8} center>
      <Text variant="meta" color="textDim">
        {label}
      </Text>
      <Text variant="cardTitle">
        {typeof value === 'number' ? value.toLocaleString() : value}
      </Text>
    </Row>
  );
}

function StatusChip({ running, report }: Readonly<{ running: boolean; report: SyncReport }>) {
  const t = useT();
  if (running) {
    return <Badge tone="info">{t('cloud115.running')}</Badge>;
  }
  if (report.truncated) {
    return <Badge tone="warning">{t('cloud115.partial')}</Badge>;
  }
  if (report.finishedAt) {
    return <Badge tone="success">{t('cloud115.done')}</Badge>;
  }
  return <Badge tone="neutral">{t('cloud115.never')}</Badge>;
}

const EMPTY_REPORT: SyncReport = {
  startedAt: null,
  finishedAt: null,
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
  callsTotal: 0,
  truncated: false,
  errors: [],
};

const FILE_ROW: CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'space-between',
  gap: 12,
  padding: '7px 0',
};

const FILE_MAIN: CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  gap: 10,
  minWidth: 0,
};

const FILE_NAME: CSSProperties = {
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
  font: 'var(--type-meta)',
  color: 'var(--kroma-text)',
};

const FILE_ACTIONS: CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  gap: 10,
  flexShrink: 0,
};

const VIDEO_NAME = /\.(mkv|mp4|ts|m2ts|iso|avi|mov|wmv|flv|webm|rmvb|mpg|mpeg|m4v|rm)$/i;

/** The module's own page: the card holds the settings, this adds the drive. */
export default function Cloud115Page() {
  const t = useT();
  const api = useCloud115Api();
  const [running, setRunning] = useState(false);
  const [report, setReport] = useState<SyncReport>(EMPTY_REPORT);

  useEffect(() => {
    const pull = () => {
      api
        .status()
        .then((view) => {
          setRunning(view.running);
          setReport(view.report);
        })
        .catch(() => undefined);
    };
    pull();
    const id = setInterval(pull, 4000);
    return () => clearInterval(id);
  }, [api]);

  return (
    <>
      <PageHeader.Root>
        <PageHeader.Title>{t('cloud115.title')}</PageHeader.Title>
        <PageHeader.Subtitle>{t('cloud115.subtitle')}</PageHeader.Subtitle>
        <PageHeader.Actions>
          <Row gap={8} center>
            <Button label={t('nav.cloud115Files')} size="sm" variant="glass" icon="folder" asChild>
              <a href="/admin/cloud115-files" />
            </Button>
            <StatusChip running={running} report={report} />
          </Row>
        </PageHeader.Actions>
      </PageHeader.Root>

      <Cloud115Settings />
    </>
  );
}

/** A read-through of the drive, one folder at a time, so an operator can check
 *  what the mirror sees without opening 115 in another tab. Each folder costs
 *  one API call; the pacer applies. */
export function Cloud115Files() {
  const t = useT();
  const api = useCloud115Api();
  const [trail, setTrail] = useState<{ cid: string; name: string }[]>([{ cid: '0', name: '' }]);
  const [view, setView] = useState<BrowseView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState<string | null>(null);
  const current = trail[trail.length - 1] ?? { cid: '0', name: '' };

  const load = (cid: string) => {
    setBusy(true);
    setError(null);
    api
      .browse(cid)
      .then(setView)
      .catch((reason) => {
        setView(null);
        setError(reason instanceof Error ? reason.message : String(reason));
      })
      .finally(() => setBusy(false));
  };

  useEffect(() => {
    load('0');
    // eslint-disable-next-line react-hooks/exhaustive-deps -- one initial read per mount
  }, [api]);

  const enter = (entry: BrowseEntry) => {
    if (!entry.cid) {
      return;
    }
    setTrail((steps) => [...steps, { cid: entry.cid as string, name: entry.name }]);
    load(entry.cid);
  };

  const up = () => {
    if (trail.length <= 1) {
      return;
    }
    const next = trail.slice(0, -1);
    setTrail(next);
    load((next[next.length - 1] ?? { cid: '0' }).cid);
  };

  const copyLink = async (entry: BrowseEntry) => {
    const url = `${window.location.origin}/api/module/tv.kroma.cloud115/play?pc=${entry.pickCode.toLowerCase()}`;
    try {
      await navigator.clipboard.writeText(url);
      setCopied(entry.pickCode);
      setError(null);
    } catch {
      setError(`${t('cloud115.copyFailed')}：${url}`);
    }
  };

  const sorted = view
    ? [...view.entries].sort(
        (a, b) => Number(b.isDir) - Number(a.isDir) || a.name.localeCompare(b.name, 'zh-Hans'),
      )
    : [];

  return (
    <Section.Root mt={28}>
      <Section.Header>
        <Section.Title>{t('cloud115.files')}</Section.Title>
      </Section.Header>
      <Text variant="meta" color="textDim" mt={-8} mb={16}>
        {t('cloud115.filesHint')}
      </Text>
      <Surface elevated border="border" pad="none" px={22} py={20}>
        <Row wrap gap={10} center mb={14}>
          <Button
            label={t('cloud115.up')}
            icon="arrow-left"
            variant="ghost"
            size="sm"
            disabled={busy || trail.length <= 1}
            onPress={up}
          />
          <Text variant="meta" color="textDim">
            / {trail.slice(1).map((step) => step.name).join(' / ') || t('cloud115.root')}
          </Text>
          <Button
            label={t('cloud115.refresh')}
            icon="refresh"
            variant="ghost"
            size="sm"
            disabled={busy}
            onPress={() => load(current.cid)}
          />
        </Row>

        {error ? (
          <Text variant="meta" color="danger" mb={10}>
            {error}
          </Text>
        ) : null}
        {busy ? (
          <Text variant="meta" color="textDim">
            {t('cloud115.loading')}
          </Text>
        ) : null}
        {!busy && view && sorted.length === 0 ? (
          <Text variant="meta" color="textDim">
            {t('cloud115.emptyFolder')}
          </Text>
        ) : null}

        {sorted.map((entry) => {
          const isVideo = !entry.isDir && VIDEO_NAME.test(entry.name);
          return (
            <div style={FILE_ROW} key={entry.fid ?? entry.cid ?? entry.name}>
              <div style={FILE_MAIN}>
                <Icon
                  name={entry.isDir ? 'folder' : 'file'}
                  size={16}
                  thickness={1.8}
                  color={entry.isDir ? 'accent' : 'textDim'}
                />
                <span style={FILE_NAME} title={entry.name}>
                  {entry.name}
                </span>
              </div>
              <div style={FILE_ACTIONS}>
                {!entry.isDir ? (
                  <Text variant="meta" color="textDim">
                    {formatSize(entry.size)}
                  </Text>
                ) : null}
                {entry.isDir ? (
                  <Button
                    label={t('cloud115.open')}
                    variant="ghost"
                    size="sm"
                    disabled={!entry.cid}
                    onPress={() => enter(entry)}
                  />
                ) : isVideo ? (
                  <Button
                    label={t('cloud115.copyLink')}
                    icon="copy"
                    variant="ghost"
                    size="sm"
                    disabled={!entry.pickCode}
                    onPress={() => void copyLink(entry)}
                  />
                ) : null}
              </div>
            </div>
          );
        })}

        {copied ? (
          <Text variant="meta" color="success" mt={8}>
            {t('cloud115.copied')}
          </Text>
        ) : null}
      </Surface>
    </Section.Root>
  );
}

const OVERLAY: CSSProperties = {
  position: 'fixed',
  inset: 0,
  background: 'rgba(0, 0, 0, 0.55)',
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  zIndex: 50,
  padding: 16,
};

const DIALOG: CSSProperties = { width: 'min(760px, 96vw)', maxHeight: '84vh' };

const DIALOG_BODY: CSSProperties = { overflow: 'auto', maxHeight: '52vh', paddingRight: 4 };

/** Pick a drive folder by walking the tree, so a mapping's remote side is never
 *  typed from memory. Starts at the root and answers the path it ends on. */
function FolderPicker({
  mode,
  onClose,
  onPick,
}: Readonly<{ mode: 'remote' | 'local'; onClose: () => void; onPick: (path: string) => void }>) {
  const t = useT();
  const api = useCloud115Api();
  const start = mode === 'local' ? '/media' : '0';
  const [trail, setTrail] = useState<{ cid: string; name: string }[]>([
    mode === 'local' ? { cid: start, name: start } : { cid: start, name: '' },
  ]);
  const [view, setView] = useState<BrowseView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const current = trail[trail.length - 1] ?? { cid: start, name: '' };
  const path =
    mode === 'local'
      ? current.cid
      : `/${trail
          .slice(1)
          .map((step) => step.name)
          .join('/')}`;

  const load = (cid: string) => {
    setBusy(true);
    setError(null);
    (mode === 'local' ? api.browseLocal(cid) : api.browse(cid))
      .then(setView)
      .catch((reason) => {
        setView(null);
        setError(reason instanceof Error ? reason.message : String(reason));
      })
      .finally(() => setBusy(false));
  };

  useEffect(() => {
    load(start);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- one initial read per mount
  }, [api]);

  const enter = (entry: BrowseEntry) => {
    if (mode === 'local') {
      const next = `${current.cid.replace(/\/$/, '')}/${entry.name}`;
      setTrail((steps) => [...steps, { cid: next, name: entry.name }]);
      load(next);
      return;
    }
    if (!entry.cid) {
      return;
    }
    setTrail((steps) => [...steps, { cid: entry.cid as string, name: entry.name }]);
    load(entry.cid);
  };

  const up = () => {
    if (trail.length <= 1) {
      return;
    }
    const next = trail.slice(0, -1);
    setTrail(next);
    load((next[next.length - 1] ?? { cid: start }).cid);
  };

  const folders = view
    ? view.entries
        .filter((entry) => entry.isDir)
        .sort((a, b) => a.name.localeCompare(b.name, 'zh-Hans'))
    : [];

  return (
    <div style={OVERLAY}>
      <div style={DIALOG}>
        <Surface elevated border="border" pad="none" px={22} py={20}>
          <Row between gap={12} mb={12} center>
            <Text variant="cardTitle">
              {mode === 'local' ? t('cloud115.chooseLocalFolder') : t('cloud115.chooseFolder')}
            </Text>
            <Button label={t('cloud115.cancel')} variant="ghost" size="sm" onPress={onClose} />
          </Row>
          <Row wrap gap={10} center mb={10}>
            <Button
              label={t('cloud115.up')}
              icon="arrow-left"
              variant="ghost"
              size="sm"
              disabled={busy || trail.length <= 1}
              onPress={up}
            />
            <Text variant="meta" color="textDim">
              {mode === 'local' ? path : path === '/' ? t('cloud115.root') : path}
            </Text>
          </Row>
          {error ? (
            <Text variant="meta" color="danger" mb={8}>
              {error}
            </Text>
          ) : null}
          {busy ? (
            <Text variant="meta" color="textDim">
              {t('cloud115.loading')}
            </Text>
          ) : null}
          <div style={DIALOG_BODY}>
            {!busy && view
              ? folders.map((entry) => (
                  <div style={FILE_ROW} key={entry.cid ?? entry.name}>
                    <div style={FILE_MAIN}>
                      <Icon name="folder" size={16} thickness={1.8} color="accent" />
                      <span style={FILE_NAME} title={entry.name}>
                        {entry.name}
                      </span>
                    </div>
                    <Button
                      label={t('cloud115.open')}
                      variant="ghost"
                      size="sm"
                      onPress={() => enter(entry)}
                    />
                  </div>
                ))
              : null}
            {!busy && view && folders.length === 0 ? (
              <Text variant="meta" color="textDim">
                {t('cloud115.emptyFolder')}
              </Text>
            ) : null}
          </div>
          <Row between gap={12} mt={14} center>
            <Text variant="meta" color="textDim">
              {t('cloud115.pickHint')}
            </Text>
            <Button
              label={t('cloud115.pickThis')}
              variant="primary"
              size="sm"
              onPress={() => onPick(path)}
            />
          </Row>
        </Surface>
      </div>
    </div>
  );
}

function formatSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return '';
  }
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(unit === 0 || value >= 10 ? 0 : 1)} ${units[unit]}`;
}
