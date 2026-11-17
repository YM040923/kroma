import { Denied, ModuleFailed, ModuleLoading, useCap, useT } from '@kromatv/module-sdk';
import {
  Badge,
  Button,
  Dialog,
  Field,
  PageHeader,
  Row,
  Surface,
  Switch,
  Text,
} from '@kromatv/ui/kit';
import { type CSSProperties, useEffect, useMemo, useState } from 'react';
import {
  type SubscriptionItem,
  type SubscriptionTask,
  type SubscriptionTaskInput,
  useSubscriptionsApi,
} from './api.hook';

type Tab = 'movie' | 'show' | 'source';
type StatusFilter = 'all' | 'active' | 'completed' | 'failed';
type UpdatedFilter = 'all' | 'today' | 'week';

interface ListView {
  sourceUrl: string;
  tasks: SubscriptionTask[];
  items: SubscriptionItem[];
}

const blank = (kind: string): SubscriptionTaskInput => ({
  name: '',
  source: 'madreel',
  monitorPath: '',
  targetPath: '',
  failurePath: '',
  monitorMode: 'manual',
  enabled: true,
  poster: '',
  year: 0,
  season: 1,
  episodeStart: 1,
  episodeTotal: 0,
  link: '',
  tmdbId: 0,
  kind,
});

const GRID: CSSProperties = {
  display: 'grid',
  gridTemplateColumns: 'repeat(auto-fill, minmax(150px, 1fr))',
  gap: 22,
  marginTop: 20,
};

const CARD: CSSProperties = {
  display: 'block',
  width: '100%',
  padding: 0,
  border: 0,
  background: 'transparent',
  color: 'inherit',
  textAlign: 'left',
  cursor: 'pointer',
};

const POSTER: CSSProperties = {
  position: 'relative',
  width: '100%',
  aspectRatio: '2 / 3',
  overflow: 'hidden',
  borderRadius: 'var(--radius-md)',
  backgroundImage: 'linear-gradient(158deg, var(--kroma-surface-3), var(--kroma-surface-1))',
};

const IMG: CSSProperties = { width: '100%', height: '100%', objectFit: 'cover', display: 'block' };

const BADGE: CSSProperties = {
  position: 'absolute',
  top: 8,
  left: 8,
  padding: '2px 8px',
  borderRadius: 999,
  background: 'rgba(10, 10, 12, 0.72)',
  color: '#fff',
  font: 'var(--type-meta)',
};

const PROGRESS: CSSProperties = {
  position: 'absolute',
  left: 8,
  bottom: 8,
  padding: '2px 8px',
  borderRadius: 999,
  background: 'rgba(10, 10, 12, 0.72)',
  color: '#fff',
  font: 'var(--type-meta)',
};

const FILTER_ROW: CSSProperties = {
  display: 'flex',
  flexWrap: 'wrap',
  alignItems: 'center',
  gap: 8,
  marginTop: 14,
};

const TABS: CSSProperties = { display: 'flex', flexWrap: 'wrap', gap: 8, marginTop: 18 };

function statusKey(status: string): string {
  if (status === 'done' || status === 'succeeded') return 'subscriptions.statusDone';
  if (status === 'failed') return 'subscriptions.statusFailed';
  if (status === 'unlocking' || status === 'running') return 'subscriptions.statusRunning';
  if (status === 'draft') return 'subscriptions.statusDraft';
  if (status === 'paused' || status === 'disabled') return 'subscriptions.statusPaused';
  return 'subscriptions.statusPending';
}

function isActive(status: string): boolean {
  return !['done', 'succeeded', 'failed', 'paused', 'disabled'].includes(status);
}

export default function SubscriptionsPage() {
  const t = useT();
  const api = useSubscriptionsApi();
  const allowed = useCap('settings.manage');
  const [data, setData] = useState<ListView | null>(null);
  const [sourceUrl, setSourceUrl] = useState('');
  const [tab, setTab] = useState<Tab>('movie');
  const [statusFilter, setStatusFilter] = useState<StatusFilter>('all');
  const [network, setNetwork] = useState<'all' | '115'>('all');
  const [updated, setUpdated] = useState<UpdatedFilter>('all');
  const [query, setQuery] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [editing, setEditing] = useState<{ mode: 'new' | 'edit'; task?: SubscriptionTask } | null>(
    null,
  );
  const [form, setForm] = useState<SubscriptionTaskInput>(blank('movie'));

  const load = () =>
    api
      .list()
      .then((view) => {
        setData(view);
        setSourceUrl(view.sourceUrl);
      })
      .catch((reason) => setError(String(reason)));

  useEffect(() => {
    void load();
  }, [api]);

  const tasks = useMemo(() => {
    if (!data) return [];
    const now = Date.now();
    const day = 86_400_000;
    return data.tasks.filter((task) => {
      if (tab === 'movie' && task.kind === 'show') return false;
      if (tab === 'show' && task.kind !== 'show') return false;
      if (statusFilter === 'active' && !isActive(task.status)) return false;
      if (statusFilter === 'completed' && !['done', 'succeeded'].includes(task.status)) return false;
      if (statusFilter === 'failed' && task.status !== 'failed') return false;
      if (network === '115' && !/115|madreel/i.test(`${task.source} ${task.resource}`)) return false;
      if (updated === 'today' && task.lastRunAt < now - day) return false;
      if (updated === 'week' && task.lastRunAt < now - 7 * day) return false;
      if (query.trim() && !task.name.toLowerCase().includes(query.trim().toLowerCase())) return false;
      return true;
    });
  }, [data, tab, statusFilter, network, updated, query]);

  if (!allowed) {
    return <Denied />;
  }
  if (!data) {
    return error ? <ModuleFailed /> : <ModuleLoading panels={2} />;
  }

  const openNew = () => {
    setForm(blank(tab === 'show' ? 'show' : 'movie'));
    setEditing({ mode: 'new' });
  };

  const openEdit = (task: SubscriptionTask) => {
    setForm({
      name: task.name,
      source: task.source || 'madreel',
      monitorPath: task.monitorPath,
      targetPath: task.targetPath,
      failurePath: task.failurePath,
      monitorMode: task.monitorMode || 'manual',
      enabled: task.enabled,
      poster: task.poster,
      year: task.year,
      season: task.season || 1,
      episodeStart: task.episodeStart || 1,
      episodeTotal: task.episodeTotal,
      link: task.link,
      tmdbId: task.tmdbId,
      kind: task.kind || 'movie',
    });
    setEditing({ mode: 'edit', task });
  };

  const set = <K extends keyof SubscriptionTaskInput>(key: K, value: SubscriptionTaskInput[K]) =>
    setForm((current) => ({ ...current, [key]: value }));

  const save = async () => {
    if (!form.name.trim()) {
      setError('请填写标题');
      return;
    }
    setBusy(true);
    setError('');
    try {
      if (editing?.mode === 'edit' && editing.task) {
        await api.update(editing.task.id, form);
      } else {
        await api.create(form);
      }
      setEditing(null);
      await load();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  const saveSource = async () => {
    setBusy(true);
    setError('');
    try {
      const view = await api.source(sourceUrl.trim());
      setData(view);
      setSourceUrl(view.sourceUrl);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  const updatedLabel = (task: SubscriptionTask) =>
    task.lastRunAt ? new Date(task.lastRunAt).toLocaleDateString() : t('subscriptions.never');

  return (
    <>
      <PageHeader.Root>
        <PageHeader.Title>{t('nav.subscriptions')}</PageHeader.Title>
        <PageHeader.Subtitle>{t('subscriptions.subtitle')}</PageHeader.Subtitle>
      </PageHeader.Root>

      <div style={TABS}>
        {(
          [
            ['movie', 'subscriptions.movies'],
            ['show', 'subscriptions.shows'],
            ['source', 'subscriptions.settings'],
          ] as const
        ).map(([value, label]) => (
          <Button
            key={value}
            size="sm"
            variant={tab === value ? 'primary' : 'glass'}
            label={t(label)}
            onPress={() => {
              setTab(value);
              setError('');
            }}
          />
        ))}
      </div>

      {tab === 'source' ? (
        <Surface elevated border="border" pad="none" px={22} py={20} mt={20}>
          <Field.Root label={t('subscriptions.sourceUrl')} value={sourceUrl} onValueChange={setSourceUrl}>
            <Field.Input placeholder="http://172.17.0.1:8790" />
            <Field.Hint>{t('subscriptions.sourceUrlHint')}</Field.Hint>
          </Field.Root>
          <Row gap={12} mt={16}>
            <Button
              label={t('subscriptions.saveSource')}
              variant="primary"
              size="sm"
              disabled={busy}
              onPress={() => void saveSource()}
            />
          </Row>
          {error ? (
            <Text variant="meta" color="danger" mt={10}>
              {error}
            </Text>
          ) : null}
        </Surface>
      ) : (
        <>
          <div style={FILTER_ROW}>
            <Text variant="meta" color="textDim">{t('subscriptions.filterStatus')}</Text>
            {(
              [
                ['all', 'subscriptions.all'],
                ['active', 'subscriptions.active'],
                ['completed', 'subscriptions.completed'],
                ['failed', 'subscriptions.failed'],
              ] as const
            ).map(([value, label]) => (
              <Button
                key={value}
                size="sm"
                variant={statusFilter === value ? 'primary' : 'ghost'}
                label={t(label)}
                onPress={() => setStatusFilter(value)}
              />
            ))}
          </div>
          <div style={FILTER_ROW}>
            <Text variant="meta" color="textDim">{t('subscriptions.filterNetwork')}</Text>
            {(
              [
                ['all', t('subscriptions.all')],
                ['115', '115'],
              ] as const
            ).map(([value, label]) => (
              <Button
                key={value}
                size="sm"
                variant={network === value ? 'primary' : 'ghost'}
                label={label}
                onPress={() => setNetwork(value)}
              />
            ))}
          </div>
          <div style={FILTER_ROW}>
            <Text variant="meta" color="textDim">{t('subscriptions.filterUpdated')}</Text>
            {(
              [
                ['all', 'subscriptions.all'],
                ['today', 'subscriptions.today'],
                ['week', 'subscriptions.week'],
              ] as const
            ).map(([value, label]) => (
              <Button
                key={value}
                size="sm"
                variant={updated === value ? 'primary' : 'ghost'}
                label={t(label)}
                onPress={() => setUpdated(value)}
              />
            ))}
          </div>

          <Row wrap gap={12} align="flex-end" mt={18}>
            <Field.Root label={t('subscriptions.searchTitle')} value={query} onValueChange={setQuery}>
              <Field.Input placeholder={t('subscriptions.searchTitle')} />
            </Field.Root>
            <Button
              label={t('subscriptions.addTask')}
              icon="plus"
              variant="primary"
              size="sm"
              onPress={openNew}
            />
          </Row>

          {error ? (
            <Text variant="meta" color="danger" mt={10}>
              {error}
            </Text>
          ) : null}

          {tasks.length ? (
            <div style={GRID}>
              {tasks.map((task) => (
                <button key={task.id} type="button" style={CARD} onClick={() => openEdit(task)}>
                  <div style={POSTER}>
                    {task.poster ? (
                      <img
                        src={task.poster}
                        alt=""
                        style={IMG}
                        onError={(event) => {
                          event.currentTarget.style.display = 'none';
                        }}
                      />
                    ) : null}
                    {task.season > 0 ? (
                      <span style={BADGE}>{t('subscriptions.seasonN', { season: task.season })}</span>
                    ) : null}
                    {task.episodeTotal > 0 ? (
                      <span style={PROGRESS}>
                        {t('subscriptions.episodes', {
                          done: task.episodeStart,
                          total: task.episodeTotal,
                        })}
                      </span>
                    ) : null}
                  </div>
                  <Text variant="label" lines={1} mt={8}>
                    {task.name}
                  </Text>
                  <Row gap={8} align="center" mt={2}>
                    <Badge tone={task.status === 'failed' ? 'warning' : 'neutral'}>
                      {t(statusKey(task.status))}
                    </Badge>
                    <Text variant="meta" color="textDim">
                      {task.year || '—'} · {updatedLabel(task)}
                    </Text>
                  </Row>
                </button>
              ))}
            </div>
          ) : (
            <Text variant="meta" color="textDim" mt={24}>
              {data.tasks.length ? t('subscriptions.noResults') : t('subscriptions.empty')}
            </Text>
          )}
        </>
      )}

      <Dialog.Root
        open={editing != null}
        onClose={() => !busy && setEditing(null)}
        title={editing?.mode === 'edit' ? t('subscriptions.edit') : t('subscriptions.addTask')}
        width="lg"
      >
        <Dialog.Header>
          <Text variant="overline" color="textDim">
            {editing?.mode === 'edit' ? t('subscriptions.edit') : t('subscriptions.addTask')}
          </Text>
          <Text variant="title" mt={4} lines={1}>
            {form.name || t('subscriptions.addTask')}
          </Text>
        </Dialog.Header>
        <Dialog.Panel>
          <Row wrap gap={16}>
            <Field.Root label={t('subscriptions.title')} value={form.name} onValueChange={(v) => set('name', v)}>
              <Field.Input />
            </Field.Root>
            <NumberField label={t('subscriptions.tmdb')} value={form.tmdbId} onChange={(v) => set('tmdbId', v)} />
            <NumberField label={t('subscriptions.season')} value={form.season} onChange={(v) => set('season', v)} />
            <NumberField label={t('subscriptions.start')} value={form.episodeStart} onChange={(v) => set('episodeStart', v)} />
            <NumberField label={t('subscriptions.total')} value={form.episodeTotal} onChange={(v) => set('episodeTotal', v)} />
          </Row>
          <Field.Root label={t('subscriptions.link')} value={form.link} onValueChange={(v) => set('link', v)} mt={14}>
            <Field.Input placeholder="https://re0.me/resource/..." />
          </Field.Root>
          <Row wrap gap={16} mt={14}>
            <Field.Root label={t('subscriptions.target')} value={form.targetPath} onValueChange={(v) => set('targetPath', v)}>
              <Field.Input placeholder="/媒体/电影" />
            </Field.Root>
            <Field.Root label={t('subscriptions.failure')} value={form.failurePath} onValueChange={(v) => set('failurePath', v)}>
              <Field.Input placeholder="/失败" />
            </Field.Root>
          </Row>
          <Row between gap={12} mt={18}>
            <Text variant="meta" color="textDim">
              {t('subscriptions.enabled')}
            </Text>
            <Switch
              label={t('subscriptions.enabled')}
              checked={form.enabled}
              onCheckedChange={(v) => set('enabled', v)}
            />
          </Row>
        </Dialog.Panel>
        <Dialog.Footer>
          <Row gap={12}>
            <Button label={t('subscriptions.save')} variant="primary" size="sm" disabled={busy} onPress={() => void save()} />
            <Button label={t('subscriptions.cancel')} variant="ghost" size="sm" disabled={busy} onPress={() => setEditing(null)} />
          </Row>
        </Dialog.Footer>
      </Dialog.Root>
    </>
  );
}

function NumberField({
  label,
  value,
  onChange,
}: Readonly<{ label: string; value: number; onChange: (value: number) => void }>) {
  return (
    <Field.Root
      label={label}
      value={String(value)}
      onValueChange={(text) => onChange(Number.parseInt(text.replace(/[^0-9]/g, ''), 10) || 0)}
    >
      <Field.Input />
    </Field.Root>
  );
}
