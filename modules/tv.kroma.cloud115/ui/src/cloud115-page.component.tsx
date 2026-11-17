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
  CloudLibrary,
  CloudOptions,
  CloudStatusView,
  SyncReport,
} from './schemas';

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
const TASK_ROW: CSSProperties = {
  display: 'grid',
  gridTemplateColumns: 'minmax(0, 1fr) minmax(0, 1fr) auto',
  gap: 18,
  alignItems: 'end',
  padding: '18px 0',
  borderTop: '1px solid var(--kroma-border)',
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
  const [baseUrl, setBaseUrl] = useState('');
  const [cookie, setCookie] = useState('');
  const [hasCookie, setHasCookie] = useState(false);
  const [libraries, setLibraries] = useState<CloudLibrary[]>([]);
  const [options, setOptions] = useState<CloudOptions | null>(null);
  const [resolved, setResolved] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const loaded = useRef(false);
  const [picker, setPicker] = useState<{ at: number; mode: 'remote' | 'local' } | null>(null);
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
    return () => clearInterval(id);
  }, [api, t]);

  if (!canManage) {
    return <Denied />;
  }
  if (!options || !status) {
    return error ? <ModuleFailed /> : <ModuleLoading panels={3} />;
  }

  const updateLibrary = (at: number, patch: Partial<CloudLibrary>) => {
    setLibraries((current) => current.map((item, i) => (i === at ? { ...item, ...patch } : item)));
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
      setSaved(true);
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

  const clearCookie = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.save({ clearCookie: true, baseUrl, libraries, options });
      setHasCookie(false);
      setSaved(true);
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
          <div style={{ ...GRID_ROW, marginTop: 24, marginBottom: 0 }}>
            <div style={CELL}>
              <Field.Root label={t('cloud115.cookie')} value={cookie} onValueChange={setCookie}>
                <Field.Input
                  type="password"
                  placeholder={hasCookie ? t('cloud115.cookieKeep') : 'UID=…; CID=…; SEID=…'}
                />
                <Field.Hint>{t('cloud115.cookieHint')}</Field.Hint>
              </Field.Root>
            </div>
            <div style={CELL}>
              <Field.Root label={t('cloud115.baseUrl')} value={baseUrl} onValueChange={setBaseUrl}>
                <Field.Input placeholder="http://192.168.1.10:4040" />
                <Field.Hint>{t('cloud115.baseUrlHint')}</Field.Hint>
              </Field.Root>
            </div>
            <Row gap={8}>
              {hasCookie ? (
                <Button label={t('cloud115.clearCookie')} variant="ghost" size="sm" onPress={() => void clearCookie()} />
              ) : null}
            </Row>
          </div>
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
        <Surface elevated border="border" pad="none" px={22} py={8}>
          {libraries.map((library, i) => (
            <div style={TASK_ROW} key={i}>
              <div>
                <Field.Root label={t('cloud115.remote')} value={library.remote} onValueChange={(value) => updateLibrary(i, { remote: value })}>
                  <Field.Input placeholder="/电影" />
                </Field.Root>
                <Row mt={6}>
                  <Button label={t('cloud115.browseDrive')} icon="folder" variant="ghost" size="sm" onPress={() => setPicker({ at: i, mode: 'remote' })} disabled={busy} />
                </Row>
              </div>
              <div>
                <Field.Root label={t('cloud115.local')} value={library.local} onValueChange={(value) => updateLibrary(i, { local: value })}>
                  <Field.Input placeholder="/media/115/电影" />
                  <Field.Hint>{t('cloud115.localHint')}</Field.Hint>
                </Field.Root>
                <Row mt={6}>
                  <Button label={t('cloud115.browseLocal')} icon="folder" variant="ghost" size="sm" onPress={() => setPicker({ at: i, mode: 'local' })} disabled={busy} />
                </Row>
                {!library.remote.trim() || !library.local.trim() ? null : i < resolved.length ? (
                  resolved[i] ? <Text variant="meta" color="textDim" mt={4}>{t('cloud115.resolved', { path: resolved[i] as string })}</Text> : <Text variant="meta" color="accentText" mt={4}>{t('cloud115.unresolved')}</Text>
                ) : <Text variant="meta" color="textDim" mt={4}>{t('cloud115.willCreate')}</Text>}
              </div>
              <Button label={t('cloud115.remove')} variant="ghost" size="sm" onPress={() => setLibraries((current) => current.filter((_, at) => at !== i))} />
            </div>
          ))}
          <Row mt={4} mb={12}>
            <Button label={t('cloud115.addMapping')} icon="plus" variant="ghost" size="sm" onPress={() => setLibraries((current) => [...current, { ...EMPTY_LIBRARY }])} disabled={busy} />
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
              onCheckedChange={(watchEnabled) => setOptions({ ...options, watchEnabled })}
              label={t('cloud115.watch')}
            />
          </Row>
          <div style={CELL}>
            <NumberField
              label={t('cloud115.watchInterval')}
              value={options.watchIntervalSecs}
              onChange={(watchIntervalSecs) => setOptions({ ...options, watchIntervalSecs })}
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
                onChange={(intervalMs) => setOptions({ ...options, intervalMs })}
              />
            </div>
            <div style={CELL}>
              <NumberField
                label={t('cloud115.budget')}
                value={options.maxCallsPerRun}
                onChange={(maxCallsPerRun) => setOptions({ ...options, maxCallsPerRun })}
              />
            </div>
            <div style={CELL}>
              <NumberField
                label={t('cloud115.folderCap')}
                value={options.maxEntriesPerFolder}
                onChange={(maxEntriesPerFolder) => setOptions({ ...options, maxEntriesPerFolder })}
              />
            </div>
            <div style={CELL}>
              <NumberField
                label={t('cloud115.linkTtl')}
                value={options.linkTtlSecs}
                onChange={(linkTtlSecs) => setOptions({ ...options, linkTtlSecs })}
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
              onCheckedChange={(skipIso) => setOptions({ ...options, skipIso })}
              label={t('cloud115.skipIso')}
            />
          </Row>
        </Surface>
      </Section.Root>

      <Row wrap gap={12} mt={26}>
        <Button
          label={busy ? t('cloud115.saving') : t('cloud115.save')}
          icon="device-floppy"
          variant="primary"
          size="sm"
          onPress={() => void save()}
          disabled={busy}
        />
        <Button
          label={t('cloud115.syncNow')}
          icon="refresh"
          variant="glass"
          size="sm"
          onPress={() => void syncNow()}
          disabled={busy || status.running}
        />
        {saved ? (
          <Text variant="meta" color="success">
            {t('cloud115.saved')}
          </Text>
        ) : null}
        {error ? (
          <Text variant="meta" color="danger">
            {error}
          </Text>
        ) : null}
      </Row>

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

function Stat({ label, value }: Readonly<{ label: string; value: number }>) {
  return (
    <Row gap={8} center>
      <Text variant="meta" color="textDim">
        {label}
      </Text>
      <Text variant="cardTitle">{value.toLocaleString()}</Text>
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
          <StatusChip running={running} report={report} />
        </PageHeader.Actions>
      </PageHeader.Root>

      <Cloud115Settings />
      <Cloud115Files />
    </>
  );
}

/** A read-through of the drive, one folder at a time, so an operator can check
 *  what the mirror sees without opening 115 in another tab. Each folder costs
 *  one API call; the pacer applies. */
function Cloud115Files() {
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

const DIALOG: CSSProperties = { width: 'min(580px, 94vw)', maxHeight: '76vh' };

const DIALOG_BODY: CSSProperties = { overflow: 'auto', maxHeight: '42vh', paddingRight: 4 };

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
