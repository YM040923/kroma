// Admin "115 Cloud" page, in five tabs over ONE settings store: the account and
// its pacing, ingest (share transfer + upload), STRM mappings, the watcher and
// its sync board, and the logs. Backed by this module's `/settings`, `/status`,
// `/sync`, `/transfer/*` and `/browse` routes.
//
// The page never sees the stored cookie: it is told only whether one is set. A
// blank field keeps it, and clearing takes an explicit button, so autofill or a
// stray save cannot sign the module out of the account.
//
// The five tabs share the state above them, so the save bar is one bar with one
// dirty flag rather than a copy per tab, and switching tabs never discards an
// edit.
import { Denied, ModuleFailed, ModuleLoading, useCap, useT } from '@kromatv/module-sdk';
import {
  Badge,
  Box,
  Button,
  Callout,
  ChoiceList,
  Dialog,
  Field,
  Icon,
  NumberField,
  PageHeader,
  Progress,
  Row,
  SegmentGroup,
  Select,
  Surface,
  Switch,
  styles,
  Text,
  useBreakpoint,
  useStyles,
} from '@kromatv/ui/kit';
import { type CSSProperties, type ReactNode, useEffect, useRef, useState } from 'react';
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
  ShareView,
  SyncReport,
  TransferEntryView,
} from './schemas';

// The clients a 115 login can be made from. Each is a distinct session, so the
// operator picks one neither they nor another tool is holding.
const LOGIN_APPS = [
  { value: 'alipaymini', label: 'cloud115.loginApp.alipaymini' },
  { value: 'wechatmini', label: 'cloud115.loginApp.wechatmini' },
  { value: 'tv', label: 'cloud115.loginApp.tv' },
  { value: 'apple_tv', label: 'cloud115.loginApp.apple_tv' },
  { value: 'harmony', label: 'cloud115.loginApp.harmony' },
  { value: 'os_linux', label: 'cloud115.loginApp.os_linux' },
  { value: 'os_mac', label: 'cloud115.loginApp.os_mac' },
  { value: 'os_windows', label: 'cloud115.loginApp.os_windows' },
  { value: 'web', label: 'cloud115.loginApp.web' },
] as const;

const TABS = ['account', 'ingest', 'strm', 'sync', 'logs'] as const;
type Tab = (typeof TABS)[number];

const TAB_LABEL: Record<Tab, string> = {
  account: 'cloud115.tabAccount',
  ingest: 'cloud115.tabIngest',
  strm: 'cloud115.tabStrm',
  sync: 'cloud115.tabSync',
  logs: 'cloud115.tabLogs',
};

const ERRORS_PANE: CSSProperties = {
  margin: 0,
  maxHeight: 220,
  overflow: 'auto',
  padding: 12,
  borderRadius: 'var(--radius-sm)',
  background: 'var(--kroma-surface-2)',
  font: 'var(--type-meta)',
  fontFamily: 'var(--font-mono)',
  color: 'var(--kroma-text-muted)',
  whiteSpace: 'pre-wrap',
};

// The save bar rides the bottom of the viewport: a settings tab can be long,
// and the unsaved state must stay visible while the form is scrolled.
const sDefinition = styles({
  sticky: { position: 'sticky', bottom: 0, zIndex: 5 },
});

const EMPTY_LIBRARY: CloudLibrary = { remote: '', local: '' };

// The backend's own fallback when a share carries no title; rendering it would
// print Chinese in every other language, so it is replaced at the edge.
const UNTITLED_SHARE = '115 分享';

type Target = { cid: string; name: string };

function errorText(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason);
}

/** The five tabs share one loaded settings view and one dirty flag. */
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
  const [copyError, setCopyError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const loaded = useRef(false);
  // What the server held when this page loaded: a save is allowed to carry zero
  // mappings only when there were none to begin with.
  const savedMappings = useRef(0);
  const [picker, setPicker] = useState<{ at: number; mode: 'remote' | 'local' } | null>(null);
  const [loginApp, setLoginApp] = useState<string>('alipaymini');
  const [login, setLogin] = useState<LoginStarted | null>(null);
  const [loginState, setLoginState] = useState<LoginState | null>(null);
  const [loginBusy, setLoginBusy] = useState(false);
  const [uploadRemote, setUploadRemote] = useState('');
  const [uploadPath, setUploadPath] = useState('');
  const [uploadMsg, setUploadMsg] = useState<string | null>(null);
  const [uploadBusy, setUploadBusy] = useState(false);
  const [tab, setTab] = useState<Tab>('account');
  const [showAdvanced, setShowAdvanced] = useState(false);
  // Share transfer: the pasted text, the preview it produced, the entries
  // ticked per share, the destination folder and the last receipt.
  const [shareText, setShareText] = useState('');
  const [shares, setShares] = useState<ShareView[]>([]);
  const [shareBusy, setShareBusy] = useState(false);
  const [shareError, setShareError] = useState<string | null>(null);
  const [picked, setPicked] = useState<Record<string, string[]>>({});
  const [target, setTarget] = useState<Target>({ cid: '0', name: '' });
  const [targetOpen, setTargetOpen] = useState(false);
  const [replaceBusy, setReplaceBusy] = useState(false);
  const [replaceCount, setReplaceCount] = useState<number | null>(null);
  const [replaceMsg, setReplaceMsg] = useState<string | null>(null);
  const [receiveBusy, setReceiveBusy] = useState(false);
  const [receiveFiles, setReceiveFiles] = useState<number | null>(null);
  const [receiveErrors, setReceiveErrors] = useState<string[]>([]);
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
        savedMappings.current = view.libraries.filter(
          (library) => library.remote.trim() || library.local.trim(),
        ).length;
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
            api
              .settings()
              .then((settings) => setResolved(settings.resolvedLocal))
              .catch(() => undefined);
            api
              .account()
              .then(setAccount)
              .catch(() => undefined);
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
      .catch((reason) => setError(errorText(reason)))
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
      setUploadMsg(errorText(reason));
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
      setUploadMsg(errorText(reason));
    } finally {
      setUploadBusy(false);
    }
  };

  const previewShares = async () => {
    const text = shareText.trim();
    if (!text) {
      setShareError(t('cloud115.transferMissing'));
      return;
    }
    setShareBusy(true);
    setShareError(null);
    setReceiveFiles(null);
    setReceiveErrors([]);
    try {
      const list = await api.transferPreview(text);
      if (list.length === 0) {
        setShares([]);
        setPicked({});
        setShareError(t('cloud115.transferEmpty'));
        return;
      }
      setShares(list);
      // Everything is ticked on arrival; unticking is the narrower gesture.
      setPicked(
        Object.fromEntries(
          list.map((share) => [share.code, share.entries.map((entry) => entry.id)]),
        ),
      );
    } catch (reason) {
      setShares([]);
      setPicked({});
      setShareError(errorText(reason));
    } finally {
      setShareBusy(false);
    }
  };

  const receiveSelected = async () => {
    const jobs = shares
      .map((share) => ({
        share,
        entries: share.entries.filter((entry) => (picked[share.code] ?? []).includes(entry.id)),
      }))
      .filter((job) => job.entries.length > 0);
    if (jobs.length === 0) {
      setReceiveErrors([t('cloud115.transferPickNone')]);
      return;
    }
    setReceiveBusy(true);
    setReceiveFiles(null);
    setReceiveErrors([]);
    const failures: string[] = [];
    const done: string[] = [];
    let files = 0;
    try {
      // One call per share: the API takes one code, and a batch can hold several.
      for (const job of jobs) {
        try {
          const report = await api.transferReceive({
            code: job.share.code,
            password: job.share.password,
            title: shareTitle(job.share),
            cid: target.cid,
            entries: job.entries.map((entry) => ({ id: entry.id, isDir: entry.isDir })),
          });
          files += report.files;
          done.push(job.share.code);
        } catch (reason) {
          failures.push(`${shareTitle(job.share)} · ${errorText(reason)}`);
        }
      }
      setReceiveFiles(done.length > 0 ? files : null);
      setReceiveErrors(failures);
      if (done.length > 0) {
        // Succeeded shares drop their ticks; a failed one keeps what to retry.
        setPicked((current) => {
          const next = { ...current };
          for (const code of done) {
            next[code] = [];
          }
          return next;
        });
        // Pull the new receipt immediately instead of waiting for the poll.
        api
          .status()
          .then((view) => {
            setStatus(view);
            setHasCookie(view.cookieSet);
          })
          .catch(() => undefined);
      }
    } finally {
      setReceiveBusy(false);
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
      if (kept.length === 0 && savedMappings.current > 0) {
        // Never let a save erase every mapping: an empty list is what a page
        // that lost its rows submits by accident. A fresh install saving only
        // the account settings has nothing to erase, so it is allowed through.
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
      savedMappings.current = kept.length;
      // The save also creates each mapping's folder; a path outside the storage
      // roots fails here, and that must be said now rather than at the next sync.
      const view = await api.settings();
      setResolved(view.resolvedLocal);
      if (result.errors.length) {
        setSaved(false);
        setError(result.errors.join(' · '));
      } else if (view.resolvedLocal.some((path) => !path)) {
        setError(t('cloud115.unresolved'));
      }
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      setBusy(false);
    }
  };

  // Discard re-reads the server instead of remembering a snapshot: the stored
  // settings are the only truth, and a poll may have moved underneath.
  const discardChanges = async () => {
    setBusy(true);
    setError(null);
    try {
      const view = await api.settings();
      savedMappings.current = view.libraries.filter(
        (library) => library.remote.trim() || library.local.trim(),
      ).length;
      setBaseUrl(view.baseUrl);
      setHasCookie(view.hasCookie);
      setLibraries(view.libraries.length ? view.libraries : [{ ...EMPTY_LIBRARY }]);
      setOptions(view.options);
      setResolved(view.resolvedLocal);
      setCookie('');
      setDirty(false);
      setSaved(false);
    } catch (reason) {
      setError(errorText(reason));
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
      setError(errorText(reason));
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
      setError(errorText(reason));
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
      setError(errorText(reason));
    } finally {
      setBusy(false);
    }
  };

  const replaceAddresses = async (preview: boolean) => {
    setReplaceBusy(true);
    setReplaceMsg(null);
    try {
      const result = preview ? await api.replacePreview() : await api.replaceRun();
      setReplaceCount(result.count);
      setReplaceMsg(
        preview
          ? t('cloud115.replacePreviewResult', { count: result.count })
          : t('cloud115.replaceRunResult', { count: result.count }),
      );
      if (!preview) {
        api.sync(window.location.origin).catch(() => undefined);
      }
    } catch (reason) {
      setReplaceMsg(errorText(reason));
    } finally {
      setReplaceBusy(false);
    }
  };

  const report = status.report;
  const usedText = account ? account.usedText || formatSize(account.used) || '0' : '—';
  const totalText = account ? account.totalText || formatSize(account.total) || '0' : '—';
  const capacity = account && account.total > 0 ? account.used / account.total : 0;
  const shareTitle = (share: ShareView) =>
    share.title && share.title !== UNTITLED_SHARE ? share.title : t('cloud115.transferUntitled');
  const selectedCount = shares.reduce(
    (total, share) => total + (picked[share.code]?.length ?? 0),
    0,
  );
  // Only the settings tabs carry the bar; STRM and Sync wait for an account,
  // while Account also owns the pacing and so always shows it.
  const showSaveBar = tab === 'account' || (hasCookie && (tab === 'strm' || tab === 'sync'));

  return (
    <>
      <Box gap={20} mt={24}>
        {status.pausedSecs ? (
          <Callout.Root tone="accent" icon="clock">
            <Callout.Title>{t('cloud115.paused', { seconds: status.pausedSecs })}</Callout.Title>
          </Callout.Root>
        ) : null}

        <TabPicker tab={tab} onPick={setTab} />

        {tab === 'account' ? (
          <>
            <Surface elevated border="border" pad="none" px={22} py={20}>
              <Row wrap gap={20} align="center" justify="space-between">
                <Row gap={14} align="center" flex minW={260}>
                  <Box w={48} h={48} radius="xl" bg="accentSoft" center shrink={0} overflow="hidden">
                    {account?.avatarUrl ? (
                      <img
                        src={account.avatarUrl}
                        alt=""
                        style={{ width: '100%', height: '100%', objectFit: 'cover' }}
                      />
                    ) : (
                      <Text variant="title" color="accentText">
                        {account?.userName?.slice(0, 1) || '1'}
                      </Text>
                    )}
                  </Box>
                  <Box shrink={1} minW={0} gap={4}>
                    <Row wrap gap={8} align="center">
                      <Text variant="title" lines={1}>
                        {account?.userName || '115 Cloud'}
                      </Text>
                      <Badge tone={hasCookie ? 'success' : 'warning'}>
                        {hasCookie ? t('cloud115.cookieSet') : t('cloud115.cookieMissing')}
                      </Badge>
                      {account?.vip ? (
                        <Badge tone="neutral">{t('cloud115.member')}</Badge>
                      ) : null}
                    </Row>
                    <Text variant="meta" color="textDim" lines={1}>
                      {account?.userId
                        ? t('cloud115.uid', { id: account.userId })
                        : t('cloud115.accountHint')}
                    </Text>
                  </Box>
                </Row>

                <Box flex minW={260} maxW={380} gap={6} px={16} py={12} bg="surface2" radius="lg">
                  <Row between gap={12} align="center">
                    <Text variant="meta" color="textDim">
                      {t('cloud115.capacity')}
                    </Text>
                    <Text variant="meta">
                      {usedText} / {totalText}
                    </Text>
                  </Row>
                  <Progress
                    value={capacity}
                    thickness={6}
                    color="textMuted"
                    trackColor="tint/12"
                    label={t('cloud115.capacity')}
                  />
                  {account?.remainText ? (
                    <Text variant="meta" color="textDim">
                      {t('cloud115.spaceRemain')} {account.remainText}
                    </Text>
                  ) : null}
                </Box>
              </Row>

              <Row wrap gap={12} align="center" mt={20}>
                <Text variant="meta" color="textDim">
                  {t('cloud115.qrApp')}
                </Text>
                <Select.Root
                  value={loginApp}
                  onValueChange={setLoginApp}
                  label={t('cloud115.qrApp')}
                >
                  <Select.Trigger size="sm" />
                  {LOGIN_APPS.map((app) => (
                    <Select.Item key={app.value} value={app.value} label={t(app.label)} />
                  ))}
                </Select.Root>
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
                <Row wrap gap={20} mt={16} align="flex-start">
                  <Box w={220} h={220} p={8} bg="surface2" radius="lg" shrink={0} overflow="hidden">
                    <img
                      src={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(login.svg)}`}
                      alt={t('cloud115.qrLogin')}
                      style={{ width: '100%', height: '100%', display: 'block' }}
                    />
                  </Box>
                  <Box gap={8} maxW={360}>
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
                    <Text variant="meta" color="textDim" font="mono" lines={1}>
                      {login.qr}
                    </Text>
                  </Box>
                </Row>
              ) : null}

              <Row mt={16}>
                <Button
                  label={t('cloud115.advanced')}
                  icon={showAdvanced ? 'chevron-up' : 'chevron-down'}
                  variant="ghost"
                  size="sm"
                  onPress={() => setShowAdvanced((open) => !open)}
                />
              </Row>

              {showAdvanced ? (
                <Box gap={16} mt={12} p={16} bg="surface2" radius="lg">
                  <Box row wrap gap={16}>
                    <Box flex minW={240}>
                      <Field.Root
                        label={t('cloud115.cookie')}
                        value={cookie}
                        onValueChange={(value) => {
                          setDirty(true);
                          setCookie(value);
                        }}
                      >
                        <Field.Input
                          type="password"
                          placeholder={
                            hasCookie ? t('cloud115.cookieKeep') : 'UID=…; CID=…; SEID=…'
                          }
                        />
                        <Field.Hint>{t('cloud115.cookieHint')}</Field.Hint>
                      </Field.Root>
                    </Box>
                    <Box flex minW={240}>
                      <Field.Root
                        label={t('cloud115.baseUrl')}
                        value={baseUrl}
                        onValueChange={(value) => {
                          setDirty(true);
                          setBaseUrl(value);
                        }}
                      >
                        <Field.Input placeholder="http://192.168.1.10:4040" />
                        <Field.Hint>{t('cloud115.baseUrlHint')}</Field.Hint>
                      </Field.Root>
                    </Box>
                  </Box>
                  <Row wrap gap={8} center>
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
                      <Button
                        label={t('cloud115.clearCookie')}
                        variant="ghost"
                        size="sm"
                        onPress={() => void clearCookie()}
                      />
                    ) : null}
                    {baseMismatch ? (
                      <>
                        <Text variant="meta" color="accentText">
                          {t('cloud115.baseMismatch')}
                        </Text>
                        <Button
                          label={t('cloud115.useCurrent')}
                          variant="ghost"
                          size="sm"
                          onPress={() => setBaseUrl(origin)}
                        />
                      </>
                    ) : null}
                  </Row>
                </Box>
              ) : null}
            </Surface>

            <Panel title={t('cloud115.pacing')} hint={t('cloud115.pacingHint')}>
              <Box gap={18}>
                <SettingRow label={t('cloud115.interval')} hint={t('cloud115.intervalHint')}>
                  <NumberField
                    label={t('cloud115.interval')}
                    value={options.intervalMs}
                    min={500}
                    max={60000}
                    step={100}
                    w={140}
                    onValueChange={(intervalMs) => patchOptions({ intervalMs })}
                  />
                </SettingRow>
                <SettingRow label={t('cloud115.budget')} hint={t('cloud115.budgetHint')}>
                  <NumberField
                    label={t('cloud115.budget')}
                    value={options.maxCallsPerRun}
                    min={50}
                    max={20000}
                    step={50}
                    w={140}
                    onValueChange={(maxCallsPerRun) => patchOptions({ maxCallsPerRun })}
                  />
                </SettingRow>
                <SettingRow label={t('cloud115.folderCap')} hint={t('cloud115.folderCapHint')}>
                  <NumberField
                    label={t('cloud115.folderCap')}
                    value={options.maxEntriesPerFolder}
                    min={200}
                    max={20000}
                    step={100}
                    w={140}
                    onValueChange={(maxEntriesPerFolder) => patchOptions({ maxEntriesPerFolder })}
                  />
                </SettingRow>
                <SettingRow label={t('cloud115.linkTtl')} hint={t('cloud115.linkTtlHint')}>
                  <NumberField
                    label={t('cloud115.linkTtl')}
                    value={options.linkTtlSecs}
                    min={60}
                    max={14400}
                    step={60}
                    w={140}
                    onValueChange={(linkTtlSecs) => patchOptions({ linkTtlSecs })}
                  />
                </SettingRow>
                <SettingRow label={t('cloud115.pageSize')} hint={t('cloud115.pageSizeHint')}>
                  <NumberField
                    label={t('cloud115.pageSize')}
                    value={options.pageSize}
                    min={100}
                    max={1150}
                    step={50}
                    w={140}
                    onValueChange={(pageSize) => patchOptions({ pageSize })}
                  />
                </SettingRow>
              </Box>
            </Panel>
          </>
        ) : !hasCookie ? (
          <LoginPrompt onGoAccount={() => setTab('account')} />
        ) : tab === 'ingest' ? (
          <>
            <Panel title={t('cloud115.transferTitle')} hint={t('cloud115.transferHint')}>
              <Box gap={14}>
                <Field.Root
                  label={t('cloud115.transferLinks')}
                  value={shareText}
                  onValueChange={setShareText}
                >
                  <Field.Textarea
                    rows={4}
                    maxRows={8}
                    placeholder={t('cloud115.transferPlaceholder')}
                  />
                </Field.Root>
                <Row wrap gap={10} align="center">
                  <Button
                    label={
                      shareBusy ? t('cloud115.transferPreviewing') : t('cloud115.transferPreview')
                    }
                    icon="eye"
                    variant="glass"
                    size="sm"
                    loading={shareBusy}
                    disabled={shareBusy}
                    onPress={() => void previewShares()}
                  />
                </Row>

                {shareError ? (
                  <Callout.Root tone="danger" icon="alert-triangle">
                    <Callout.Title>{t('cloud115.transferPreviewFailed')}</Callout.Title>
                    <Callout.Detail>{shareError}</Callout.Detail>
                  </Callout.Root>
                ) : null}

                {shares.length ? (
                  <Box gap={12}>
                    {shares.map((share) => {
                      const chosen = picked[share.code] ?? [];
                      const allChosen =
                        share.entries.length > 0 && chosen.length === share.entries.length;
                      return (
                        <Box key={share.code} gap={10} p={16} bg="surface2" radius="lg">
                          <Row between wrap gap={10} center>
                            <Box shrink={1} minW={0} gap={2}>
                              <Text variant="label" lines={1}>
                                {shareTitle(share)}
                              </Text>
                              <Text variant="meta" color="textDim">
                                {t('cloud115.transferEntryCount', {
                                  count: share.entries.length,
                                })}
                                {share.totalSize ? ` · ${formatSize(share.totalSize)}` : ''}
                                {` · ${share.code}`}
                              </Text>
                            </Box>
                            {share.entries.length ? (
                              <Button
                                label={
                                  allChosen
                                    ? t('cloud115.transferClearAll')
                                    : t('cloud115.transferSelectAll')
                                }
                                variant="ghost"
                                size="sm"
                                onPress={() =>
                                  setPicked((current) => ({
                                    ...current,
                                    [share.code]: allChosen
                                      ? []
                                      : share.entries.map((entry) => entry.id),
                                  }))
                                }
                              />
                            ) : null}
                          </Row>
                          {share.entries.length ? (
                            <Box maxH={320} overflow="scroll" pr={4}>
                              <ChoiceList.Root
                                mode="multiple"
                                label={shareTitle(share)}
                                value={chosen}
                                onValueChange={(next) =>
                                  setPicked((current) => ({ ...current, [share.code]: next }))
                                }
                              >
                                {share.entries.map((entry) => (
                                  <ChoiceList.Item
                                    key={entry.id}
                                    value={entry.id}
                                    actions={
                                      !entry.isDir && entry.size > 0 ? (
                                        <Text variant="meta" color="textDim">
                                          {formatSize(entry.size)}
                                        </Text>
                                      ) : undefined
                                    }
                                  >
                                    <Row gap={8} center>
                                      <Icon
                                        name={entry.isDir ? 'folder' : 'file'}
                                        size={15}
                                        color={entry.isDir ? 'accentText' : 'textDim'}
                                      />
                                      <ChoiceList.Label>{entry.name}</ChoiceList.Label>
                                    </Row>
                                  </ChoiceList.Item>
                                ))}
                              </ChoiceList.Root>
                            </Box>
                          ) : (
                            <Text variant="meta" color="textDim">
                              {t('cloud115.transferNoEntries')}
                            </Text>
                          )}
                        </Box>
                      );
                    })}

                    <Row between wrap gap={16} align="center" mt={4}>
                      <Row gap={10} center shrink={1} minW={240}>
                        <Icon name="folder" size={17} color="accentText" />
                        <Box shrink={1} minW={0}>
                          <Text variant="meta" color="textDim">
                            {t('cloud115.transferTarget')}
                          </Text>
                          <Text variant="label" lines={1}>
                            {target.name ? `/${target.name}` : t('cloud115.root')}
                          </Text>
                          <Text variant="meta" color="textDim">
                            {t('cloud115.transferTargetHint')}
                          </Text>
                        </Box>
                        <Button
                          label={t('cloud115.transferPickTarget')}
                          variant="ghost"
                          size="sm"
                          onPress={() => setTargetOpen(true)}
                        />
                      </Row>
                      <Row gap={10} center>
                        <Text variant="meta" color="textDim">
                          {t('cloud115.transferSelected', { count: selectedCount })}
                        </Text>
                        <Button
                          label={
                            receiveBusy
                              ? t('cloud115.transferReceiving')
                              : t('cloud115.transferReceive')
                          }
                          icon="download"
                          variant="primary"
                          size="sm"
                          loading={receiveBusy}
                          disabled={receiveBusy || selectedCount === 0}
                          onPress={() => void receiveSelected()}
                        />
                      </Row>
                    </Row>

                    {receiveErrors.length ? (
                      <Callout.Root tone="danger" icon="alert-triangle">
                        <Callout.Title>{t('cloud115.transferFailed')}</Callout.Title>
                        <Callout.Detail>{receiveErrors.join(' · ')}</Callout.Detail>
                      </Callout.Root>
                    ) : null}
                    {receiveFiles !== null ? (
                      <Callout.Root tone="success" icon="check">
                        <Callout.Title>
                          {t('cloud115.transferDone', { files: receiveFiles })}
                        </Callout.Title>
                      </Callout.Root>
                    ) : null}
                  </Box>
                ) : null}
              </Box>
            </Panel>

            <Panel title={t('cloud115.upload')} hint={t('cloud115.uploadHint')}>
              <Box gap={12}>
                <Box row wrap gap={16} align="flex-end">
                  <Box flex minW={220}>
                    <Field.Root
                      label={t('cloud115.uploadRemote')}
                      value={uploadRemote}
                      onValueChange={setUploadRemote}
                    >
                      <Field.Input placeholder={t('cloud115.uploadRemotePlaceholder')} />
                      <Field.Hint>{t('cloud115.uploadRemoteHint')}</Field.Hint>
                    </Field.Root>
                  </Box>
                  <Box flex minW={220}>
                    <Field.Root
                      label={t('cloud115.uploadPath')}
                      value={uploadPath}
                      onValueChange={setUploadPath}
                    >
                      <Field.Input placeholder={t('cloud115.uploadPathPlaceholder')} />
                      <Field.Hint>{t('cloud115.uploadPathHint')}</Field.Hint>
                    </Field.Root>
                  </Box>
                </Box>
                <Row wrap gap={8} align="center">
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
              </Box>
            </Panel>

            <Panel title={t('cloud115.transferHistory')} hint={t('cloud115.transferHistoryHint')}>
              <TransferHistory entries={status.transfers} />
            </Panel>
          </>
        ) : tab === 'strm' ? (
          <>
            <Panel title={t('cloud115.mappings')} hint={t('cloud115.mappingsHint')}>
              <Box gap={12}>
                {libraries.map((library, i) => (
                  <Box key={i} gap={12} p={16} bg="surface2" radius="lg">
                    <Box row wrap gap={18} align="flex-start">
                      <Box flex minW={240} gap={6}>
                        <Text variant="meta" color="textDim">
                          {t('cloud115.remote')}
                        </Text>
                        <Box minH={40} px={12} py={10} bg="surface1" radius="md" border="border">
                          <Text variant="body" lines={1}>
                            {library.remote || '—'}
                          </Text>
                        </Box>
                        <Row>
                          <Button
                            label={t('cloud115.browseDrive')}
                            icon="folder"
                            variant="glass"
                            size="sm"
                            onPress={() => setPicker({ at: i, mode: 'remote' })}
                            disabled={busy}
                          />
                        </Row>
                      </Box>
                      <Box flex minW={240}>
                        <Field.Root
                          label={t('cloud115.local')}
                          value={library.local}
                          onValueChange={(value) => updateLibrary(i, { local: value })}
                        >
                          <Field.Input placeholder={t('cloud115.localPlaceholder')} />
                          <Field.Hint>{t('cloud115.localHint')}</Field.Hint>
                        </Field.Root>
                        <Row mt={8}>
                          <Button
                            label={t('cloud115.browseLocal')}
                            icon="folder"
                            variant="ghost"
                            size="sm"
                            onPress={() => setPicker({ at: i, mode: 'local' })}
                            disabled={busy}
                          />
                        </Row>
                        {!library.remote.trim() || !library.local.trim() ? null : i <
                          resolved.length ? (
                          resolved[i] ? (
                            <Text variant="meta" color="textDim" mt={6}>
                              {t('cloud115.resolved', { path: resolved[i] as string })}
                            </Text>
                          ) : (
                            <Text variant="meta" color="accentText" mt={6}>
                              {t('cloud115.unresolved')}
                            </Text>
                          )
                        ) : (
                          <Text variant="meta" color="textDim" mt={6}>
                            {t('cloud115.willCreate')}
                          </Text>
                        )}
                      </Box>
                    </Box>
                    <Row justify="flex-end">
                      <Button
                        label={t('cloud115.remove')}
                        variant="ghost"
                        size="sm"
                        onPress={() => {
                          setDirty(true);
                          setLibraries((current) => current.filter((_, at) => at !== i));
                        }}
                      />
                    </Row>
                  </Box>
                ))}
                <Row>
                  <Button
                    label={t('cloud115.addMapping')}
                    icon="plus"
                    variant="ghost"
                    size="sm"
                    onPress={() => {
                      setDirty(true);
                      setLibraries((current) => [...current, { ...EMPTY_LIBRARY }]);
                    }}
                    disabled={busy}
                  />
                </Row>
              </Box>
            </Panel>

            <Panel title={t('cloud115.strmOptions')} hint={t('cloud115.strmOptionsHint')}>
              <Box gap={18}>
                <SettingRow label={t('cloud115.skipIso')} hint={t('cloud115.skipIsoHint')}>
                  <Switch
                    checked={options.skipIso}
                    onCheckedChange={(skipIso) => patchOptions({ skipIso })}
                    label={t('cloud115.skipIso')}
                  />
                </SettingRow>
                <SettingRow label={t('cloud115.probe')} hint={t('cloud115.probeHint')}>
                  <Switch
                    checked={options.probeEnabled}
                    onCheckedChange={(probeEnabled) => patchOptions({ probeEnabled })}
                    label={t('cloud115.probe')}
                  />
                </SettingRow>
                <SettingRow
                  label={t('cloud115.keepSuffix')}
                  hint={t('cloud115.keepSuffixHint')}
                >
                  <Switch
                    checked={options.keepSuffix}
                    onCheckedChange={(keepSuffix) => patchOptions({ keepSuffix })}
                    label={t('cloud115.keepSuffix')}
                  />
                </SettingRow>
                <SettingRow
                  label={t('cloud115.overwriteStrm')}
                  hint={t('cloud115.overwriteStrmHint')}
                >
                  <Switch
                    checked={options.overwriteStrm}
                    onCheckedChange={(overwriteStrm) => patchOptions({ overwriteStrm })}
                    label={t('cloud115.overwriteStrm')}
                  />
                </SettingRow>
                <SettingRow
                  label={t('cloud115.minVideoSize')}
                  hint={t('cloud115.minVideoSizeHint')}
                >
                  <NumberField
                    label={t('cloud115.minVideoSize')}
                    value={options.minVideoSizeMb}
                    min={0}
                    max={1_048_576}
                    step={100}
                    w={140}
                    onValueChange={(minVideoSizeMb) => patchOptions({ minVideoSizeMb })}
                  />
                </SettingRow>
              </Box>
            </Panel>

            <Panel title={t('cloud115.assetsTitle')} hint={t('cloud115.assetsHint')}>
              <Box gap={18}>
                <SettingRow
                  label={t('cloud115.syncImages')}
                  hint={t('cloud115.syncImagesHint')}
                >
                  <Switch
                    checked={options.syncImages}
                    onCheckedChange={(syncImages) => patchOptions({ syncImages })}
                    label={t('cloud115.syncImages')}
                  />
                </SettingRow>
                <SettingRow label={t('cloud115.syncNfo')} hint={t('cloud115.syncNfoHint')}>
                  <Switch
                    checked={options.syncNfo}
                    onCheckedChange={(syncNfo) => patchOptions({ syncNfo })}
                    label={t('cloud115.syncNfo')}
                  />
                </SettingRow>
                <SettingRow
                  label={t('cloud115.syncSubtitles')}
                  hint={t('cloud115.syncSubtitlesHint')}
                >
                  <Switch
                    checked={options.syncSubtitles}
                    onCheckedChange={(syncSubtitles) => patchOptions({ syncSubtitles })}
                    label={t('cloud115.syncSubtitles')}
                  />
                </SettingRow>
              </Box>
            </Panel>

            <Panel title={t('cloud115.replaceTitle')} hint={t('cloud115.replaceHint')}>
              <Box gap={12}>
                <Row between wrap gap={12} center>
                  <Text variant="meta" color="textDim" lines={1}>
                    {baseUrl || origin || '—'}
                  </Text>
                  <Row gap={8}>
                    <Button
                      label={t('cloud115.replacePreview')}
                      icon="eye"
                      variant="ghost"
                      size="sm"
                      loading={replaceBusy}
                      onPress={() => void replaceAddresses(true)}
                    />
                    <Button
                      label={t('cloud115.replaceRun')}
                      icon="refresh"
                      variant="glass"
                      size="sm"
                      disabled={replaceBusy || !replaceCount}
                      onPress={() => void replaceAddresses(false)}
                    />
                  </Row>
                </Row>
                {replaceMsg ? (
                  <Text variant="meta" color="textMuted">
                    {replaceMsg}
                  </Text>
                ) : null}
              </Box>
            </Panel>
          </>
        ) : tab === 'sync' ? (
          <>
            <Panel
              title={t('cloud115.watch')}
              hint={t('cloud115.watchHint')}
              actions={
                <Switch
                  checked={options.watchEnabled}
                  onCheckedChange={(watchEnabled) => patchOptions({ watchEnabled })}
                  label={t('cloud115.watch')}
                />
              }
            >
              <Box gap={18}>
                <SettingRow
                  label={t('cloud115.watchInterval')}
                  hint={t('cloud115.watchIntervalHint')}
                >
                  <NumberField
                    label={t('cloud115.watchInterval')}
                    value={options.watchIntervalSecs}
                    min={30}
                    max={3600}
                    step={10}
                    w={140}
                    onValueChange={(watchIntervalSecs) => patchOptions({ watchIntervalSecs })}
                  />
                </SettingRow>
                <Text variant="meta" color={status.gateOk === false ? 'danger' : 'textDim'}>
                  {status.lastPollAt
                    ? t('cloud115.lastPoll', {
                        time: new Date(status.lastPollAt * 1000).toLocaleTimeString(),
                      })
                    : t('cloud115.neverPolled')}
                  {status.gateOk === false ? ` · ${t('cloud115.gateOff')}` : ''}
                </Text>
                {status.watchEnabled && status.indexedDirs === 0 ? (
                  <Text variant="meta" color="accentText">
                    {t('cloud115.indexNotReady')}
                  </Text>
                ) : null}
              </Box>
            </Panel>

            <Panel
              title={status.running ? t('cloud115.syncProgress') : t('cloud115.lastRun')}
              hint={t('cloud115.syncBoardHint')}
              actions={
                <Button
                  label={t('cloud115.syncNow')}
                  icon="refresh"
                  variant="glass"
                  size="sm"
                  onPress={() => void syncNow()}
                  disabled={busy || status.running}
                />
              }
            >
              {dirty ? (
                <Text variant="meta" color="accentText" mb={10}>
                  {t('cloud115.syncDirtyHint')}
                </Text>
              ) : null}
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
                        rate: (report.startedAt
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
                          <Text variant="meta" color={entry.errors ? 'accentText' : 'textDim'}>
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
                <Text variant="meta" color="accentText" mt={8}>
                  {t('cloud115.truncated')}
                </Text>
              ) : null}
            </Panel>
          </>
        ) : (
          <>
            <Panel title={t('cloud115.runErrors')} hint={t('cloud115.runErrorsHint')}>
              <Box gap={8}>
                {status.lastPlayError ? (
                  <Text variant="meta" color="danger">
                    {t('cloud115.lastPlayError')} · {status.lastPlayError}
                  </Text>
                ) : null}
                {report.errors.length ? (
                  <pre style={ERRORS_PANE}>{report.errors.join('\n')}</pre>
                ) : (
                  <Text variant="meta" color="textDim">
                    {report.finishedAt ? t('cloud115.noErrors') : t('cloud115.never')}
                  </Text>
                )}
              </Box>
            </Panel>

            <Panel title={t('cloud115.playLog')} hint={t('cloud115.playLogHint')}>
              <Box gap={8}>
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
                  <Row between gap={12} mt={6} center>
                    <Box minW={0} shrink={1}>
                      <Text variant="meta" color="textDim">
                        {t('cloud115.lastLink')}
                      </Text>
                      <Text variant="body" color="textDim" font="mono" lines={1}>
                        {status.lastPlayUrl}
                      </Text>
                      {copyError ? (
                        <Text variant="meta" color="danger" mt={4}>
                          {copyError}
                        </Text>
                      ) : null}
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
                            setCopyError(null);
                            setTimeout(() => setCopiedLink(false), 1500);
                          })
                          .catch(() => setCopyError(t('cloud115.copyFailed')));
                      }}
                    />
                  </Row>
                ) : null}
              </Box>
            </Panel>

            <Panel title={t('cloud115.transferHistory')} hint={t('cloud115.transferHistoryHint')}>
              <TransferHistory entries={status.transfers} />
            </Panel>
          </>
        )}

        {showSaveBar ? (
          <SaveBar
            dirty={dirty}
            busy={busy}
            saved={saved}
            savedAt={savedAt}
            error={error}
            onSave={() => void save()}
            onDiscard={() => void discardChanges()}
          />
        ) : null}
      </Box>

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

      {targetOpen ? (
        <CloudFolderPicker
          onClose={() => setTargetOpen(false)}
          onPick={(next) => {
            setTarget(next);
            setTargetOpen(false);
          }}
        />
      ) : null}
    </>
  );
}

function TabPicker({ tab, onPick }: Readonly<{ tab: Tab; onPick: (next: Tab) => void }>) {
  const t = useT();
  if (useBreakpoint() === 'base') {
    return (
      <Select.Root
        label={t('cloud115.tabs')}
        value={tab}
        onValueChange={(next) => onPick(TABS.find((one) => one === next) ?? 'account')}
      >
        <Select.Trigger block />
        {TABS.map((one) => (
          <Select.Item key={one} value={one} label={t(TAB_LABEL[one])} />
        ))}
      </Select.Root>
    );
  }
  return (
    <SegmentGroup.Root value={tab} onValueChange={onPick} label={t('cloud115.tabs')}>
      {TABS.map((one) => (
        <SegmentGroup.Item key={one} value={one}>
          <SegmentGroup.Label>{t(TAB_LABEL[one])}</SegmentGroup.Label>
        </SegmentGroup.Item>
      ))}
    </SegmentGroup.Root>
  );
}

function LoginPrompt({ onGoAccount }: Readonly<{ onGoAccount: () => void }>) {
  const t = useT();
  return (
    <Callout.Root tone="accent" icon="lock">
      <Callout.Title>{t('cloud115.loginRequired')}</Callout.Title>
      <Callout.Detail>{t('cloud115.loginRequiredHint')}</Callout.Detail>
      <Callout.Actions>
        <Button label={t('cloud115.goAccount')} variant="glass" size="sm" onPress={onGoAccount} />
      </Callout.Actions>
    </Callout.Root>
  );
}

/** A titled card: the heading, its one sentence, optional actions, content. */
function Panel({
  title,
  hint,
  actions,
  children,
}: Readonly<{ title: string; hint?: string; actions?: ReactNode; children: ReactNode }>) {
  return (
    <Surface elevated border="border" pad="none" px={22} py={20}>
      <Row between wrap gap={12} align="flex-start" mb={16}>
        <Box shrink={1} minW={0} gap={4}>
          <Text variant="cardTitle">{title}</Text>
          {hint ? (
            <Text variant="meta" color="textDim">
              {hint}
            </Text>
          ) : null}
        </Box>
        {actions ?? null}
      </Row>
      {children}
    </Surface>
  );
}

/** Label and hint on the left, the control at the far end: the settings row the
 *  design uses, so a value is read before it is changed. */
function SettingRow({
  label,
  hint,
  children,
}: Readonly<{ label: string; hint?: string; children: ReactNode }>) {
  return (
    <Row wrap gapX={24} gapY={10} align="center" between>
      <Box shrink={1} minW={220} gap={3}>
        <Text variant="label">{label}</Text>
        {hint ? (
          <Text variant="meta" color="textDim">
            {hint}
          </Text>
        ) : null}
      </Box>
      <Row gap={8} center shrink={0}>
        {children}
      </Row>
    </Row>
  );
}

function SaveBar({
  dirty,
  busy,
  saved,
  savedAt,
  error,
  onSave,
  onDiscard,
}: Readonly<{
  dirty: boolean;
  busy: boolean;
  saved: boolean;
  savedAt: Date | null;
  error: string | null;
  onSave: () => void;
  onDiscard: () => void;
}>) {
  const t = useT();
  const s = useStyles(sDefinition);
  return (
    <Surface elevated border="border" pad="none" px={18} py={14} style={s.sticky}>
      <Row between wrap gap={12} center>
        <Row wrap gap={10} center>
          {dirty ? (
            <Badge tone="warning">{t('cloud115.unsaved')}</Badge>
          ) : saved ? (
            <Badge tone="success">{t('cloud115.saved')}</Badge>
          ) : (
            <Text variant="meta" color="textDim">
              {t('cloud115.clean')}
            </Text>
          )}
          {dirty ? (
            <Text variant="meta" color="textDim">
              {t('cloud115.savedHint')}
            </Text>
          ) : saved && savedAt ? (
            <Text variant="meta" color="textDim">
              {savedAt.toLocaleTimeString()}
            </Text>
          ) : null}
        </Row>
        <Row gap={8} center>
          <Button
            label={t('cloud115.discard')}
            icon="arrow-back-up"
            variant="ghost"
            size="sm"
            onPress={onDiscard}
            disabled={busy || !dirty}
          />
          <Button
            label={busy ? t('cloud115.saving') : t('cloud115.save')}
            icon="device-floppy"
            variant="primary"
            size="sm"
            onPress={onSave}
            disabled={busy || !dirty}
            loading={busy && dirty}
          />
        </Row>
      </Row>
      {error ? (
        <Text variant="meta" color="danger" mt={8}>
          {error}
        </Text>
      ) : null}
    </Surface>
  );
}

/** The share receipts the module keeps, newest first. The stored `at` is in
 *  MILLISECONDS (store::now_ms), unlike the scan history's seconds. */
function TransferHistory({ entries }: Readonly<{ entries: TransferEntryView[] }>) {
  const t = useT();
  if (entries.length === 0) {
    return (
      <Text variant="meta" color="textDim">
        {t('cloud115.transferNoHistory')}
      </Text>
    );
  }
  return (
    <Box gap={10}>
      {[...entries].reverse().map((entry) => (
        <Row key={`${entry.at}-${entry.code}`} between wrap gap={12} align="flex-start">
          <Box shrink={1} minW={0} gap={3}>
            <Text variant="label" lines={1}>
              {entry.title && entry.title !== UNTITLED_SHARE
                ? entry.title
                : t('cloud115.transferUntitled')}
            </Text>
            <Text variant="meta" color="textDim">
              {new Date(entry.at).toLocaleString()} · {t('cloud115.transferCode')} {entry.code} ·{' '}
              {t('cloud115.transferFileCount', { count: entry.files })}
            </Text>
            {/* A success message is the server's own Chinese sentence; the
                badge and the count already say it in every language. */}
            {!entry.ok && entry.message ? (
              <Text variant="meta" color="danger" lines={2}>
                {entry.message}
              </Text>
            ) : null}
          </Box>
          <Badge tone={entry.ok ? 'success' : 'danger'}>
            {entry.ok ? t('cloud115.transferOk') : t('cloud115.transferFail')}
          </Badge>
        </Row>
      ))}
    </Box>
  );
}

/** Pick a 115 folder for a share transfer, answering the folder's cid and the
 *  path it ends on. Starts at the root, like the mapping picker. */
function CloudFolderPicker({
  onClose,
  onPick,
}: Readonly<{ onClose: () => void; onPick: (target: Target) => void }>) {
  const t = useT();
  const api = useCloud115Api();
  const [trail, setTrail] = useState<{ cid: string; name: string }[]>([{ cid: '0', name: '' }]);
  const [view, setView] = useState<BrowseView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const current = trail[trail.length - 1] ?? { cid: '0', name: '' };
  const path = trail
    .slice(1)
    .map((step) => step.name)
    .join('/');

  const load = (cid: string) => {
    setBusy(true);
    setError(null);
    api
      .browse(cid)
      .then(setView)
      .catch((reason) => {
        setView(null);
        setError(errorText(reason));
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

  const folders = view
    ? view.entries
        .filter((entry) => entry.isDir)
        .sort((a, b) => a.name.localeCompare(b.name, 'zh-Hans'))
    : [];

  return (
    <Dialog.Root open title={t('cloud115.chooseFolder')} onClose={onClose} width="md" fullHeight>
      <Dialog.Header>
        <Text variant="title">{t('cloud115.chooseFolder')}</Text>
        <Row wrap gap={10} center>
          <Button
            label={t('cloud115.up')}
            icon="arrow-left"
            variant="ghost"
            size="sm"
            disabled={busy || trail.length <= 1}
            onPress={up}
          />
          <Text variant="meta" color="textDim">
            {path ? `/${path}` : t('cloud115.root')}
          </Text>
        </Row>
      </Dialog.Header>
      <Dialog.Panel>
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
        {!busy && view
          ? folders.map((entry) => (
              <Row between gap={12} key={entry.cid ?? entry.name}>
                <Row gap={10} center shrink={1} minW={0}>
                  <Icon name="folder" size={16} thickness={1.8} color="accentText" />
                  <Text variant="meta" lines={1}>
                    {entry.name}
                  </Text>
                </Row>
                <Button
                  label={t('cloud115.open')}
                  variant="ghost"
                  size="sm"
                  onPress={() => enter(entry)}
                />
              </Row>
            ))
          : null}
        {!busy && view && folders.length === 0 ? (
          <Text variant="meta" color="textDim">
            {t('cloud115.emptyFolder')}
          </Text>
        ) : null}
      </Dialog.Panel>
      <Dialog.Footer>
        <Text variant="meta" color="textDim" mb={8}>
          {t('cloud115.pickHint')}
        </Text>
        <Dialog.Actions
          onCancel={onClose}
          cancelLabel={t('cloud115.cancel')}
          onConfirm={() => onPick({ cid: current.cid, name: path })}
          confirmLabel={t('cloud115.pickThis')}
        />
      </Dialog.Footer>
    </Dialog.Root>
  );
}

function Stat({ label, value }: Readonly<{ label: string; value: number | string }>) {
  return (
    <Row gap={8} center>
      <Text variant="meta" color="textDim">
        {label}
      </Text>
      <Text variant="cardTitle">{typeof value === 'number' ? value.toLocaleString() : value}</Text>
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

/** The module's own page: the card holds the five settings tabs, this adds the
 *  drive. */
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
          {running ? <StatusChip running={running} report={report} /> : null}
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
        setError(errorText(reason));
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
      setError(`${t('cloud115.copyFailed')} ${url}`);
    }
  };

  const sorted = view
    ? [...view.entries].sort(
        (a, b) => Number(b.isDir) - Number(a.isDir) || a.name.localeCompare(b.name, 'zh-Hans'),
      )
    : [];

  return (
    <Surface elevated border="border" pad="none" px={22} py={20} mt={24}>
      <Row between wrap gap={12} mb={14} center>
        <Text variant="cardTitle">{t('cloud115.files')}</Text>
        <Row wrap gap={10} center>
          <Button
            label={t('cloud115.up')}
            icon="arrow-left"
            variant="ghost"
            size="sm"
            disabled={busy || trail.length <= 1}
            onPress={up}
          />
          <Text variant="meta" color="textDim">
            /{' '}
            {trail
              .slice(1)
              .map((step) => step.name)
              .join(' / ') || t('cloud115.root')}
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
  );
}

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
  const heading = mode === 'local' ? t('cloud115.chooseLocalFolder') : t('cloud115.chooseFolder');

  const load = (cid: string) => {
    setBusy(true);
    setError(null);
    (mode === 'local' ? api.browseLocal(cid) : api.browse(cid))
      .then(setView)
      .catch((reason) => {
        setView(null);
        setError(errorText(reason));
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
    <Dialog.Root open title={heading} onClose={onClose} width="md" fullHeight>
      <Dialog.Header>
        <Text variant="title">{heading}</Text>
        <Row wrap gap={10} center>
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
      </Dialog.Header>
      <Dialog.Panel>
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
        {!busy && view
          ? folders.map((entry) => (
              <Row between gap={12} key={entry.cid ?? entry.name}>
                <Row gap={10} center shrink={1} minW={0}>
                  <Icon name="folder" size={16} thickness={1.8} color="accentText" />
                  <Text variant="meta" lines={1}>
                    {entry.name}
                  </Text>
                </Row>
                <Button
                  label={t('cloud115.open')}
                  variant="ghost"
                  size="sm"
                  onPress={() => enter(entry)}
                />
              </Row>
            ))
          : null}
        {!busy && view && folders.length === 0 ? (
          <Text variant="meta" color="textDim">
            {t('cloud115.emptyFolder')}
          </Text>
        ) : null}
      </Dialog.Panel>
      <Dialog.Footer>
        <Text variant="meta" color="textDim" mb={8}>
          {t('cloud115.pickHint')}
        </Text>
        <Dialog.Actions
          onCancel={onClose}
          cancelLabel={t('cloud115.cancel')}
          onConfirm={() => onPick(path)}
          confirmLabel={t('cloud115.pickThis')}
        />
      </Dialog.Footer>
    </Dialog.Root>
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
