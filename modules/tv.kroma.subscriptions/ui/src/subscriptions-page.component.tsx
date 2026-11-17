import { Denied, ModuleFailed, ModuleLoading, useCap, useT } from '@kromatv/module-sdk';
import { Badge, Button, Field, PageHeader, Row, Section, Surface, Text } from '@kromatv/ui/kit';
import { useEffect, useState } from 'react';
import {
  type SubscriptionItem,
  type SubscriptionTask,
  useSubscriptionsApi,
} from './api.hook';

const blank = {
  name: '',
  source: '115',
  monitorPath: '',
  targetPath: '',
  failurePath: '',
  monitorMode: 'manual',
  enabled: true,
};

interface ListView {
  sourceUrl: string;
  tasks: SubscriptionTask[];
  items: SubscriptionItem[];
}

export default function SubscriptionsPage() {
  const t = useT();
  const api = useSubscriptionsApi();
  const allowed = useCap('settings.manage');
  const [data, setData] = useState<ListView | null>(null);
  const [sourceUrl, setSourceUrl] = useState('');
  const [form, setForm] = useState(blank);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

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

  if (!allowed) {
    return <Denied />;
  }
  if (!data) {
    return error ? <ModuleFailed /> : <ModuleLoading panels={2} />;
  }

  const set = (key: keyof typeof form, value: string | boolean) =>
    setForm((current) => ({ ...current, [key]: value }));

  const save = async () => {
    setBusy(true);
    setError('');
    try {
      await api.create(form);
      setForm(blank);
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

  return (
    <>
      <PageHeader.Root>
        <PageHeader.Title>{t('nav.subscriptions')}</PageHeader.Title>
        <PageHeader.Subtitle>{t('subscriptions.subtitle')}</PageHeader.Subtitle>
      </PageHeader.Root>

      <Section.Root mt={26}>
        <Section.Header>
          <Section.Title>{t('subscriptions.sourceUrl')}</Section.Title>
        </Section.Header>
        <Surface elevated border="border" pad="none" px={22} py={20}>
          <Row wrap gap={14} align="flex-end">
            <Field.Root
              label={t('subscriptions.sourceUrl')}
              value={sourceUrl}
              onValueChange={setSourceUrl}
            >
              <Field.Input placeholder="http://172.17.0.1:8790" />
              <Field.Hint>{t('subscriptions.sourceUrlHint')}</Field.Hint>
            </Field.Root>
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
      </Section.Root>

      <Section.Root mt={28}>
        <Section.Header>
          <Section.Title>{t('subscriptions.new')}</Section.Title>
        </Section.Header>
        <Surface elevated border="border" pad="none" px={22} py={20}>
          <Row wrap gap={14}>
            <Field.Root label={t('subscriptions.name')} value={form.name} onValueChange={(v) => set('name', v)}>
              <Field.Input />
            </Field.Root>
            <Field.Root label={t('subscriptions.source')} value={form.source} onValueChange={(v) => set('source', v)}>
              <Field.Input placeholder="115" />
            </Field.Root>
            <Field.Root label={t('subscriptions.monitor')} value={form.monitorPath} onValueChange={(v) => set('monitorPath', v)}>
              <Field.Input placeholder="/订阅" />
            </Field.Root>
            <Field.Root label={t('subscriptions.target')} value={form.targetPath} onValueChange={(v) => set('targetPath', v)}>
              <Field.Input placeholder="/媒体/电影" />
            </Field.Root>
            <Field.Root label={t('subscriptions.failure')} value={form.failurePath} onValueChange={(v) => set('failurePath', v)}>
              <Field.Input placeholder="/失败" />
            </Field.Root>
          </Row>
          <Row gap={12} mt={18}>
            <Button
              label={t('subscriptions.save')}
              variant="primary"
              size="sm"
              onPress={() => void save()}
              disabled={busy}
            />
          </Row>
        </Surface>
      </Section.Root>

      <Section.Root mt={28}>
        <Section.Header>
          <Section.Title>{t('subscriptions.tasks')}</Section.Title>
        </Section.Header>
        <Surface elevated border="border" pad="none" px={22} py={8}>
          {data.tasks.length ? (
            data.tasks.map((task) => (
              <Row key={task.id} between gap={16} py={16}>
                <div>
                  <Text variant="cardTitle">{task.name}</Text>
                  <Text variant="meta" color="textDim">
                    {task.resource || `${task.monitorPath} → ${task.targetPath}`}
                  </Text>
                </div>
                <Row gap={10} center>
                  <Badge tone={task.status === 'running-dry-run' ? 'info' : 'neutral'}>
                    {task.status}
                  </Badge>
                  <Button
                    label={t('subscriptions.scan')}
                    size="sm"
                    variant="ghost"
                    onPress={() => api.scan(task.id).then(load)}
                  />
                </Row>
              </Row>
            ))
          ) : (
            <Text variant="meta" color="textDim" py={18}>
              {t('subscriptions.empty')}
            </Text>
          )}
        </Surface>
      </Section.Root>
    </>
  );
}
