// Settings drafts persist across tabs; authorization is independent of configuration.
import {
  apiErrorText,
  Denied,
  ModuleFailed,
  ModuleLoading,
  useAdminHost,
  useCap,
  useT,
} from '@kromatv/module-sdk';
import {
  Badge,
  Box,
  Button,
  Callout,
  Checkbox,
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
import { SyncBoard } from './sync-board.component';
import { CloudConfigExport } from './schemas';
import { pickLocalFolder } from './local-folder';
import type {
  BrowseEntry,
  BrowseView,
  CloudAccount,
  CloudLibrary,
  CloudOptions,
  CloudStatusView,
  SyncReport,
} from './schemas';

import { AppIdLogin } from './app-id-login.component';

const FAST_STATES: Record<string, string> = {
  pending: 'cloud115.state.queued', checked: 'cloud115.fastChecked', baselining: 'cloud115.fastBaseline',
  'rate limited': 'cloud115.fastLimited', error: 'cloud115.state.failed',
};

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

const EMPTY_LIBRARY: CloudLibrary = { remote: '', local: '', localDisplay: '', fastSync: true };

function errorText(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason);
}

/** The five tabs share one loaded settings view and one dirty flag. */
export function Cloud115Settings() {
  const t = useT();
  const api = useCloud115Api();
  const { client } = useAdminHost();
  const canManage = useCap('settings.manage');

  const [status, setStatus] = useState<CloudStatusView | null>(null);
  const [account, setAccount] = useState<CloudAccount | null>(null);
  const [baseUrl, setBaseUrl] = useState('');
  const [appId, setAppId] = useState('');
  const [authorized, setAuthorized] = useState(false);
  const [libraries, setLibraries] = useState<CloudLibrary[]>([]);
  const [options, setOptions] = useState<CloudOptions | null>(null);
  const [pickingLocalRow, setPickingLocalRow] = useState<CloudLibrary | null>(null);
  const [localPickerError, setLocalPickerError] = useState<{
    row: CloudLibrary;
    message: string;
  } | null>(null);
  const localPickPending = useRef(false);
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
  const [remotePickerRow, setRemotePickerRow] = useState<CloudLibrary | null>(null);
  const [uploadRemote, setUploadRemote] = useState('');
  const [uploadPath, setUploadPath] = useState('');
  const [uploadMsg, setUploadMsg] = useState<string | null>(null);
  const [uploadBusy, setUploadBusy] = useState(false);
  const [tab, setTab] = useState<Tab>('account');
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [replaceBusy, setReplaceBusy] = useState(false);
  const [replaceCount, setReplaceCount] = useState<number | null>(null);
  const [replaceMsg, setReplaceMsg] = useState<string | null>(null);
  const [accountBusy, setAccountBusy] = useState(false);
  const [aliasOpen, setAliasOpen] = useState(false);
  const [aliasDraft, setAliasDraft] = useState('');
  const [removeOpen, setRemoveOpen] = useState(false);
  const [clearOpen, setClearOpen] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [importText, setImportText] = useState('');
  const [opsMsg, setOpsMsg] = useState<string | null>(null);
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
        setAppId(view.appId);
        setAuthorized(view.authorized);
        setLibraries(view.libraries.length ? view.libraries : [{ ...EMPTY_LIBRARY }]);
        setOptions(view.options);
      })
      .catch(() => setError(t('cloud115.loadFailed')));

    const pull = () => {
      api
        .status()
        .then((view) => {
          setStatus(view);
          setAuthorized(view.authorized);
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

  if (!canManage) {
    return <Denied />;
  }
  if (!options || !status) {
    return error ? <ModuleFailed /> : <ModuleLoading panels={3} />;
  }

  const updateLibrary = (row: CloudLibrary, patch: Partial<CloudLibrary>) => {
    setDirty(true);
    setLibraries((current) => current.map((item) => (item === row ? { ...item, ...patch } : item)));
  };

  const pickingLocal = pickingLocalRow !== null;
  const mappingBusy = busy || pickingLocal || replaceBusy;
  const pickerDisabled =
    mappingBusy || accountBusy || uploadBusy;
  const incompleteMapping = libraries.some(
    (row) => Boolean(row.remote.trim()) !== Boolean(row.local.trim()),
  );

  const chooseLocalFolder = async (row: CloudLibrary) => {
    if (pickerDisabled || localPickPending.current) {
      return;
    }
    localPickPending.current = true;
    setPickingLocalRow(row);
    setLocalPickerError(null);
    try {
      const choice = await pickLocalFolder(client.admin);
      if (choice === 'unavailable') {
        setLocalPickerError({ row, message: t('cloud115.localPickerUnavailable') });
      } else if (choice) {
        updateLibrary(row, choice);
      }
    } catch (reason) {
      const fallback = t('cloud115.localPickerFailed');
      setLocalPickerError({
        row,
        message: typeof apiErrorText === 'function' ? apiErrorText(reason, fallback) : fallback,
      });
    } finally {
      localPickPending.current = false;
      setPickingLocalRow(null);
    }
  };

  const patchOptions = (patch: Partial<CloudOptions>) => {
    setDirty(true);
    setOptions((current) => (current ? { ...current, ...patch } : current));
  };

  const restartAfterMappingChange = async () => {
    if (typeof client.admin.restartModule !== 'function') {
      throw new Error(t('cloud115.mappingRestartUnavailable'));
    }
    await client.admin.restartModule('tv.kroma.cloud115');
  };

  const save = async () => {
    if (localPickPending.current || (incompleteMapping && tab !== 'account')) {
      return;
    }
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
        ...(incompleteMapping ? {} : { libraries: kept }),
        options,
      });
      if (result.restartRequired) {
        await restartAfterMappingChange();
      }
      setDirty(incompleteMapping);
      setSaved(!incompleteMapping);
      setSavedAt(new Date());
      if (!incompleteMapping) {
        savedMappings.current = kept.length;
      }
      if (result.errors.length) {
        setSaved(false);
        setError(result.errors.join(' · '));
      } else if (!incompleteMapping && (await api.settings()).resolvedLocal.some((path) => !path)) {
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
      setAuthorized(view.authorized);
      setLibraries(view.libraries.length ? view.libraries : [{ ...EMPTY_LIBRARY }]);
      setOptions(view.options);
      setDirty(false);
      setSaved(false);
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

  const reloadSettings = async () => {
    const view = await api.settings();
    savedMappings.current = view.libraries.filter(
      (library) => library.remote.trim() || library.local.trim(),
    ).length;
    setBaseUrl(view.baseUrl);
    setAuthorized(view.authorized);
    setLibraries(view.libraries.length ? view.libraries : [{ ...EMPTY_LIBRARY }]);
    setOptions(view.options);
  };

  const refreshAccount = async () => {
    setAccountBusy(true);
    setError(null);
    try {
      const [view, board] = await Promise.all([api.account(true), api.status()]);
      setAccount(view);
      setStatus(board);
      setAuthorized(board.authorized);
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      setAccountBusy(false);
    }
  };

  const saveAlias = async () => {
    setAccountBusy(true);
    setError(null);
    try {
      const result = await api.accountAlias(aliasDraft.trim());
      setAccount((current) => (current ? { ...current, alias: result.alias } : current));
      setAliasOpen(false);
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      setAccountBusy(false);
    }
  };

  const removeAccount = async () => {
    setAccountBusy(true);
    setError(null);
    try {
      await api.accountRemove();
      setAccount(null);
      setAuthorized(false);
      setRemoveOpen(false);
      api
        .status()
        .then((view) => {
          setStatus(view);
          setAuthorized(view.authorized);
        })
        .catch(() => undefined);
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      setAccountBusy(false);
    }
  };

  const watchNow = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.watchNow();
      setOpsMsg(t('cloud115.watchStarted'));
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      setBusy(false);
    }
  };

  const clearCheckpoint = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.checkpointClear();
      setClearOpen(false);
      setOpsMsg(t('cloud115.checkpointCleared'));
      api.status().then(setStatus).catch(() => undefined);
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      setBusy(false);
    }
  };

  const exportConfig = async () => {
    setBusy(true);
    setError(null);
    try {
      const view = await api.configExport();
      const blob = new Blob([JSON.stringify(view, null, 2)], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const link = document.createElement('a');
      link.href = url;
      link.download = 'cloud115-config.json';
      link.click();
      URL.revokeObjectURL(url);
      setOpsMsg(t('cloud115.exportDone'));
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      setBusy(false);
    }
  };

  const importConfig = async () => {
    setBusy(true);
    setError(null);
    try {
      const parsed = JSON.parse(importText) as unknown;
      const config = CloudConfigExport.safeParse(parsed);
      if (!config.success || !Array.isArray((parsed as { libraries?: unknown }).libraries)) {
        throw new Error(t('cloud115.importInvalid'));
      }
      const imported = await api.configImport(config.data);
      if (imported.restartRequired) {
        await restartAfterMappingChange();
      }
      await reloadSettings();
      setDirty(false);
      setSaved(true);
      setSavedAt(new Date());
      setImportOpen(false);
      setImportText('');
      setOpsMsg(t('cloud115.importDone'));
    } catch (reason) {
      setError(reason instanceof SyntaxError ? t('cloud115.importInvalid') : errorText(reason));
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
  const lastErrorCount = report.mappings?.length
    ? report.mappings.reduce((sum, mapping) => sum + mapping.errorCount, 0)
    : status.running ? report.errors.length : status.history.at(-1)?.errors ?? report.errors.length;
  const usedText = account ? account.usedText || formatSize(account.used) || '0' : '—';
  const totalText = account ? account.totalText || formatSize(account.total) || '0' : '—';
  const capacity = account && account.total > 0 ? account.used / account.total : 0;
  const displayName = account?.alias || account?.userName || '115 Cloud';
  const showSaveBar = tab === 'account' || tab === 'strm' || (authorized && tab === 'sync');

  return (
    <>
      <Box gap={20} mt={24}>
        {status.pausedSecs ? (
          <Callout.Root tone="accent" icon="clock">
            <Callout.Title>{t('cloud115.paused', { seconds: status.pausedSecs })}</Callout.Title>
          </Callout.Root>
        ) : null}

        <TabPicker tab={tab} onPick={setTab} disabled={pickingLocal} />

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
                        {displayName.slice(0, 1)}
                      </Text>
                    )}
                  </Box>
                  <Box shrink={1} minW={0} gap={4}>
                    <Row wrap gap={8} align="center">
                      <Text variant="title" lines={1}>
                        {displayName}
                      </Text>
                      <Badge tone={authorized ? 'success' : 'warning'}>
                        {authorized ? t('cloud115.connected') : t('cloud115.disconnected')}
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

              <Row wrap gap={8} align="center" mt={14}>
                <Button
                  label={t('cloud115.refreshAccount')}
                  icon="refresh"
                  variant="ghost"
                  size="sm"
                  loading={accountBusy}
                  disabled={!authorized}
                  onPress={() => void refreshAccount()}
                />
                <Button
                  label={t('cloud115.renameAccount')}
                  variant="ghost"
                  size="sm"
                  disabled={!authorized}
                  onPress={() => {
                    setAliasDraft(account?.alias || account?.userName || '');
                    setAliasOpen(true);
                  }}
                />
                <Button
                  label={t('cloud115.removeAccount')}
                  variant="ghost"
                  size="sm"
                  disabled={!authorized}
                  onPress={() => setRemoveOpen(true)}
                />
              </Row>

              {!removeOpen && <AppIdLogin key={appId} initialId={appId} authorized={authorized} disabled={accountBusy}
                onConfirmed={(id) => {
                  setAuthorized(true);
                  setAppId(id);
                  setAccount(null);
                  void api.account(true).then(setAccount).catch(() => undefined);
                }} />}

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
                    {baseMismatch ? (
                      <>
                        <Text variant="meta" color="accentText">
                          {t('cloud115.baseMismatch')}
                        </Text>
                        <Button
                          label={t('cloud115.useCurrent')}
                          variant="ghost"
                          size="sm"
                          onPress={() => { setBaseUrl(origin); setDirty(true); }}
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
                <SettingRow
                  label={t('cloud115.dailyBudget')}
                  hint={t('cloud115.dailyBudgetHint')}
                >
                  <NumberField
                    label={t('cloud115.dailyBudget')}
                    value={options.dailyCallBudget}
                    min={0}
                    max={1000000}
                    step={100}
                    w={140}
                    onValueChange={(dailyCallBudget) => patchOptions({ dailyCallBudget })}
                  />
                </SettingRow>
                {status.dailyCallBudget > 0 ? (
                  <Text variant="meta" color="textDim">
                    {t('cloud115.dailyUsed', {
                      calls: status.callsToday,
                      total: status.dailyCallBudget,
                    })}
                  </Text>
                ) : null}
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
        ) : !authorized && tab !== 'strm' ? (
          <LoginPrompt onGoAccount={() => setTab('account')} />
        ) : tab === 'ingest' ? (
          <>
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
                          <Text variant="body" selectable>
                            {library.remote || t('cloud115.driveFolderUnset')}
                          </Text>
                        </Box>
                        <Row>
                          <Button
                            label={t('cloud115.browseDrive')}
                            icon="folder"
                            variant="glass"
                            size="sm"
                            onPress={() => setRemotePickerRow(library)}
                            disabled={mappingBusy || !authorized}
                          />
                        </Row>
                        {!authorized ? (
                          <Text variant="meta" color="textDim">
                            {t('cloud115.mappingLoginHint')}
                          </Text>
                        ) : null}
                      </Box>
                      <Box flex minW={240}>
                        <Text variant="meta" color="textDim" mb={6}>
                          {t('cloud115.local')}
                        </Text>
                        <Box minH={40} px={12} py={10} bg="surface1" radius="md" border="border">
                          <Text variant="body" selectable>
                            {library.localDisplay || t('cloud115.localFolderUnset')}
                          </Text>
                        </Box>
                        <Text variant="meta" color="textDim" mt={6}>
                          {t('cloud115.localHint')}
                        </Text>
                        <Row mt={8} accessibilityLiveRegion="polite">
                          <Button
                            label={t(
                              pickingLocalRow === library
                                ? 'cloud115.localPickerWaiting'
                                : 'cloud115.browseLocal',
                            )}
                            icon="folder"
                            variant="ghost"
                            size="sm"
                            onPress={() => void chooseLocalFolder(library)}
                            loading={pickingLocalRow === library}
                            disabled={pickerDisabled}
                          />
                        </Row>
                        {localPickerError?.row === library ? (
                          <Callout.Root tone="danger" size="sm" icon="alert-triangle">
                            <Callout.Detail>{localPickerError.message}</Callout.Detail>
                          </Callout.Root>
                        ) : library.local && !library.localDisplay ? (
                          <Text variant="meta" color="accentText" mt={6}>
                            {t('cloud115.unresolved')}
                          </Text>
                        ) : null}
                      </Box>
                    </Box>
                    <Row wrap gap={12} justify="space-between" align="center">
                      <Row gap={8} center>
                        <Checkbox label={t('cloud115.fastMapping')} checked={library.fastSync ?? true}
                          disabled={mappingBusy} onCheckedChange={(fastSync) => updateLibrary(library, { fastSync })} />
                        <Text variant="meta">{t('cloud115.fastMapping')}</Text>
                      </Row>
                      <Button
                        label={t('cloud115.remove')}
                        variant="ghost"
                        size="sm"
                        disabled={mappingBusy}
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
                    disabled={mappingBusy}
                  />
                </Row>
              </Box>
            </Panel>

            {authorized ? (
              <div inert={pickingLocal || busy}>
                <Box gap={20} opacity={pickingLocal || busy ? 0.5 : 1}>
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
                      <SettingRow label={t('cloud115.keepSuffix')} hint={t('cloud115.keepSuffixHint')}>
                        <Switch
                          checked={options.keepSuffix}
                          onCheckedChange={(keepSuffix) => patchOptions({ keepSuffix })}
                          label={t('cloud115.keepSuffix')}
                        />
                      </SettingRow>
                      <SettingRow label={t('cloud115.overwriteStrm')} hint={t('cloud115.overwriteStrmHint')}>
                        <Switch
                          checked={options.overwriteStrm}
                          onCheckedChange={(overwriteStrm) => patchOptions({ overwriteStrm })}
                          label={t('cloud115.overwriteStrm')}
                        />
                      </SettingRow>
                      <SettingRow label={t('cloud115.minVideoSize')} hint={t('cloud115.minVideoSizeHint')}>
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
                      <SettingRow label={t('cloud115.prefetchNext')} hint={t('cloud115.prefetchNextHint')}>
                        <NumberField
                          label={t('cloud115.prefetchNext')}
                          value={options.prefetchNext}
                          min={0}
                          max={3}
                          step={1}
                          w={140}
                          onValueChange={(prefetchNext) => patchOptions({ prefetchNext })}
                        />
                      </SettingRow>
                    </Box>
                  </Panel>

                  <Panel title={t('cloud115.assetsTitle')} hint={t('cloud115.assetsHint')}>
                    <Box gap={18}>
                      <SettingRow label={t('cloud115.syncImages')} hint={t('cloud115.syncImagesHint')}>
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
                      <SettingRow label={t('cloud115.syncSubtitles')} hint={t('cloud115.syncSubtitlesHint')}>
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
                </Box>
              </div>
            ) : null}
          </>
        ) : tab === 'sync' ? (
          <>
            <Panel
              title={t('cloud115.watch')}
              hint={t('cloud115.watchHint')}
              actions={
                <Row gap={10} align="center">
                  <Button
                    label={t('cloud115.watchNow')}
                    variant="ghost"
                    size="sm"
                    onPress={() => void watchNow()}
                    disabled={busy || !authorized}
                  />
                  <Switch
                    checked={options.watchEnabled}
                    onCheckedChange={(watchEnabled) => patchOptions({ watchEnabled })}
                    label={t('cloud115.watch')}
                  />
                </Row>
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
                <Badge tone={status.watchEnabled ? 'success' : 'neutral'}>
                  {t(status.watchEnabled ? 'cloud115.fastEnabled' : 'cloud115.autoSyncOff')}
                </Badge>
                <Text variant="meta">{status.fastSync.lastPollAt
                  ? t('cloud115.lastPoll', { time: new Date(status.fastSync.lastPollAt * 1000).toLocaleString() })
                  : t('cloud115.neverPolled')}</Text>
                <Stat label={t('cloud115.statWritten')} value={status.fastSync.lastAdded} />
                <Stat label={t('cloud115.indexedDirs')} value={status.fastSync.indexedDirs} />
                {status.fastSync.mappings.map((m, i) => (
                  <Box key={`${m.remote}:${i}`} gap={4}>
                    <Text variant="label">{m.remote} · {t(FAST_STATES[m.state] ?? 'cloud115.fastUnknown')}</Text>
                    <Text variant="meta">{m.lastCheckedAt ? t('cloud115.lastPoll', {
                      time: new Date(m.lastCheckedAt * 1000).toLocaleString(),
                    }) : t('cloud115.neverPolled')} · {t('cloud115.statWritten')} {m.lastAdded}{m.total != null && m.lastCheckedAt > 0 ? ` · ${t('cloud115.statFiles')} ${m.total}` : ''}</Text>
                  </Box>
                ))}
                {status.fastSync.lastError && <Callout.Root tone="danger"><Callout.Detail>{status.fastSync.lastError}</Callout.Detail></Callout.Root>}
              </Box>
            </Panel>

            <Panel title={t('cloud115.schedule')} hint={t('cloud115.scheduleHint')}>
              <Box gap={12}>
                <SettingRow label={t('cloud115.autoSync')} hint={t('cloud115.autoSyncHint')}>
                  <Select.Root
                    value={String(options.autoSyncHours)}
                    onValueChange={(value) => patchOptions({ autoSyncHours: Number(value) })}
                    label={t('cloud115.autoSync')}
                  >
                    <Select.Trigger size="sm" />
                    <Select.Item value="0" label={t('cloud115.autoSyncOff')} />
                    <Select.Item value="6" label={t('cloud115.autoSyncEvery', { hours: 6 })} />
                    <Select.Item value="12" label={t('cloud115.autoSyncEvery', { hours: 12 })} />
                    <Select.Item value="24" label={t('cloud115.autoSyncDaily')} />
                  </Select.Root>
                </SettingRow>
                <Text variant="meta" color="textDim">
                  {status.lastAutoSyncAt
                    ? t('cloud115.lastAutoSync', {
                        time: new Date(status.lastAutoSyncAt * 1000).toLocaleString(),
                      })
                    : t('cloud115.neverAutoSync')}
                </Text>
                {opsMsg ? (
                  <Text variant="meta" color="accentText">
                    {opsMsg}
                  </Text>
                ) : null}
              </Box>
            </Panel>

            <Panel title={t('cloud115.syncOps')} hint={t('cloud115.syncOpsHint')}>
              <Row wrap gap={8} align="center">
                <Button
                  label={t('cloud115.clearCheckpoint')}
                  variant="ghost"
                  size="sm"
                  onPress={() => setClearOpen(true)}
                  disabled={busy || !authorized}
                />
                <Button
                  label={t('cloud115.exportConfig')}
                  icon="download"
                  variant="ghost"
                  size="sm"
                  onPress={() => void exportConfig()}
                  disabled={busy}
                />
                <Button
                  label={t('cloud115.importConfig')}
                  icon="upload"
                  variant="ghost"
                  size="sm"
                  onPress={() => setImportOpen(true)}
                  disabled={busy}
                />
              </Row>
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
                  disabled={busy || status.running || !authorized}
                />
              }
            >
              {dirty ? (
                <Text variant="meta" color="accentText" mb={10}>
                  {t('cloud115.syncDirtyHint')}
                </Text>
              ) : null}
              <SyncBoard running={status.running} report={report} history={status.history} />
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
                    {lastErrorCount > 0
                      ? t('cloud115.mappingErrors', { count: lastErrorCount })
                      : status.running || report.finishedAt || status.history.length
                        ? t('cloud115.noErrors')
                        : t('cloud115.never')}
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

          </>
        )}

        {showSaveBar ? (
          <SaveBar
            dirty={dirty}
            busy={busy}
            canSave={dirty && !mappingBusy && (!incompleteMapping || tab === 'account')}
            canDiscard={dirty && !mappingBusy}
            hint={incompleteMapping
              ? t(tab === 'account' ? 'cloud115.accountMappingDraft' : 'cloud115.mappingIncomplete')
              : t('cloud115.savedHint')}
            saved={saved}
            savedAt={savedAt}
            error={error}
            onSave={() => void save()}
            onDiscard={() => void discardChanges()}
          />
        ) : null}
      </Box>

      {authorized && remotePickerRow ? (
        <FolderPicker
          onClose={() => setRemotePickerRow(null)}
          onPick={(path) => {
            updateLibrary(remotePickerRow, { remote: path });
            setRemotePickerRow(null);
          }}
        />
      ) : null}

      {aliasOpen ? (
        <Dialog.Root
          open
          title={t('cloud115.renameAccount')}
          onClose={() => setAliasOpen(false)}
          width="sm"
        >
          <Dialog.Header>
            <Text variant="title">{t('cloud115.renameAccount')}</Text>
          </Dialog.Header>
          <Dialog.Panel>
            <Field.Root
              label={t('cloud115.accountAlias')}
              value={aliasDraft}
              onValueChange={setAliasDraft}
            >
              <Field.Input placeholder={account?.userName || '115 Cloud'} />
              <Field.Hint>{t('cloud115.accountAliasHint')}</Field.Hint>
            </Field.Root>
          </Dialog.Panel>
          <Dialog.Footer>
            <Dialog.Actions
              onCancel={() => setAliasOpen(false)}
              cancelLabel={t('cloud115.cancel')}
              onConfirm={() => void saveAlias()}
              confirmLabel={t('cloud115.save')}
              busy={accountBusy}
            />
          </Dialog.Footer>
        </Dialog.Root>
      ) : null}

      {removeOpen ? (
        <Dialog.Root
          open
          title={t('cloud115.removeAccount')}
          onClose={() => setRemoveOpen(false)}
          width="sm"
        >
          <Dialog.Header>
            <Text variant="title">{t('cloud115.removeAccount')}</Text>
          </Dialog.Header>
          <Dialog.Panel>
            <Callout.Root tone="danger" icon="alert-triangle">
              <Callout.Title>{t('cloud115.removeAccountWarn')}</Callout.Title>
              <Callout.Detail>{t('cloud115.removeAccountHint')}</Callout.Detail>
            </Callout.Root>
          </Dialog.Panel>
          <Dialog.Footer>
            <Dialog.Actions
              onCancel={() => setRemoveOpen(false)}
              cancelLabel={t('cloud115.cancel')}
              onConfirm={() => void removeAccount()}
              confirmLabel={t('cloud115.removeAccount')}
              destructive
              busy={accountBusy}
            />
          </Dialog.Footer>
        </Dialog.Root>
      ) : null}

      {clearOpen ? (
        <Dialog.Root
          open
          title={t('cloud115.clearCheckpoint')}
          onClose={() => setClearOpen(false)}
          width="sm"
        >
          <Dialog.Header>
            <Text variant="title">{t('cloud115.clearCheckpoint')}</Text>
          </Dialog.Header>
          <Dialog.Panel>
            <Callout.Root tone="accent" icon="alert-triangle">
              <Callout.Title>{t('cloud115.clearCheckpointWarn')}</Callout.Title>
              <Callout.Detail>{t('cloud115.clearCheckpointHint')}</Callout.Detail>
            </Callout.Root>
          </Dialog.Panel>
          <Dialog.Footer>
            <Dialog.Actions
              onCancel={() => setClearOpen(false)}
              cancelLabel={t('cloud115.cancel')}
              onConfirm={() => void clearCheckpoint()}
              confirmLabel={t('cloud115.clearCheckpoint')}
              destructive
              busy={busy}
            />
          </Dialog.Footer>
        </Dialog.Root>
      ) : null}

      {importOpen ? (
        <Dialog.Root
          open
          title={t('cloud115.importConfig')}
          onClose={() => setImportOpen(false)}
          width="md"
        >
          <Dialog.Header>
            <Text variant="title">{t('cloud115.importConfig')}</Text>
          </Dialog.Header>
          <Dialog.Panel>
            <Field.Root
              label={t('cloud115.importConfigLabel')}
              value={importText}
              onValueChange={setImportText}
            >
              <Field.Textarea rows={12} placeholder='{ "baseUrl": "http://192.168.1.10:4040", … }' />
              <Field.Hint>{t('cloud115.importConfigHint')}</Field.Hint>
            </Field.Root>
          </Dialog.Panel>
          <Dialog.Footer>
            <Dialog.Actions
              onCancel={() => setImportOpen(false)}
              cancelLabel={t('cloud115.cancel')}
              onConfirm={() => void importConfig()}
              confirmLabel={t('cloud115.importConfig')}
              busy={busy}
              disabled={!importText.trim()}
            />
          </Dialog.Footer>
        </Dialog.Root>
      ) : null}
    </>
  );
}

function TabPicker({ tab, onPick, disabled }: Readonly<{
  tab: Tab;
  onPick: (next: Tab) => void;
  disabled: boolean;
}>) {
  const t = useT();
  if (useBreakpoint() === 'base') {
    return (
      <Select.Root
        label={t('cloud115.tabs')}
        disabled={disabled}
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
        <SegmentGroup.Item key={one} value={one} disabled={disabled}>
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
  canSave,
  canDiscard,
  hint,
  saved,
  savedAt,
  error,
  onSave,
  onDiscard,
}: Readonly<{
  dirty: boolean;
  busy: boolean;
  canSave: boolean;
  canDiscard: boolean;
  hint: string;
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
              {hint}
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
            disabled={!canDiscard}
          />
          <Button
            label={busy ? t('cloud115.saving') : t('cloud115.save')}
            icon="device-floppy"
            variant="primary"
            size="sm"
            onPress={onSave}
            disabled={!canSave}
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
  onClose,
  onPick,
}: Readonly<{ onClose: () => void; onPick: (path: string) => void }>) {
  const t = useT();
  const api = useCloud115Api();
  const start = '0';
  const [trail, setTrail] = useState<{ cid: string; name: string }[]>([{ cid: start, name: '' }]);
  const [view, setView] = useState<BrowseView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const path = `/${trail
    .slice(1)
    .map((step) => step.name)
    .join('/')}`;
  const heading = t('cloud115.chooseFolder');

  const load = (cid: string) => {
    setBusy(true);
    setError(null);
    api.browse(cid)
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
            {path === '/' ? t('cloud115.root') : path}
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
          disabled={busy || !view}
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
