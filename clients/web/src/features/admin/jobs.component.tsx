import { useKromaClient } from '#web/shared/lib/web-runtime';
// Admin "Tâches" console: every background job with its schedule, next/last run
// and live progress, plus run-now / cancel / enable / edit-schedule actions and
// an expandable run-history + log panel. Mirrors the server's job registry
// (`services::jobs`) over `/api/admin/jobs`.

import { KromaEvents } from '@kromatv/client/events';
import { useT } from '@kromatv/ui';
import { Box, Button, EmptyState, Row, Section, Surface, Text } from '@kromatv/ui/kit';
import { useEffect, useState } from 'react';
import { JobCard } from '#web/features/admin/job-card.component';
import { RealtimeBadge } from '#web/features/admin/realtime-badge.component';
import { PageHeader, usePoll } from '#web/features/admin/shell';
import { apiBase } from '#web/shared/lib/api';
import { useAuth } from '#web/shared/lib/auth';
import { TableSkeleton } from '#web/shared/ui';

// Progress pushed over the WS bus, keyed by job key.
type LiveProgress = Record<string, { done: number; total: number }>;

interface CloudStatus {
  running?: boolean;
  report?: {
    current?: string;
    dirs?: number;
    files?: number;
    written?: number;
    pruned?: number;
    finishedAt?: number | null;
  };
  lastPollAt?: number;
  lastEvents?: number;
}

export function JobsPage() {
  const eventClient = useKromaClient();

  const t = useT();
  const { client } = useAuth();
  const { data, reload } = usePoll(['admin', 'jobs'], () => client.jobs.list(), 6000);
  const [live, setLive] = useState<LiveProgress>({});

  // A page-scoped event stream for smooth progress + immediate reloads on
  // start/finish (the shell's stream only bumps the refetch `tick`).
  useEffect(() => {
    const ev = new KromaEvents(apiBase(), {
      token: () => eventClient.sessionToken,
      refresh: () => eventClient.refreshSession(),

      onEvent: (e) => {
        if (e.type === 'job.progress') {
          setLive((s) => ({ ...s, [e.key]: { done: e.done, total: e.total } }));
        } else if (e.type === 'job.started' || e.type === 'job.finished') {
          void reload();
        }
      },
    });
    ev.connect();
    return () => ev.close();
  }, [reload, eventClient.sessionToken, eventClient.refreshSession]);

  // Pipeline stages live in their own "Pipeline" console, not the general task
  // list, so filter them out here.
  const jobs = (data?.jobs ?? []).filter((j) => j.category !== 'pipeline');
  const categories = [...new Set(jobs.map((j) => j.category))];

  return (
    <>
      <PageHeader.Root>
        <PageHeader.Title>{t('admin.jobsTitle')}</PageHeader.Title>
        <PageHeader.Subtitle>{t('admin.jobsSub')}</PageHeader.Subtitle>
        <PageHeader.Actions>
          <RealtimeBadge />
        </PageHeader.Actions>
      </PageHeader.Root>
      <ModuleTaskProgress />
      {data === null ? <TableSkeleton rows={6} /> : null}
      {categories.map((cat) => (
        <Section.Root key={cat} mt={28}>
          <Section.Header>
            <Section.Title>{t(`jobs.cat.${cat}`)}</Section.Title>
          </Section.Header>
          <Box gap={14}>
            {jobs
              .filter((j) => j.category === cat)
              .map((j) => (
                <JobCard key={j.key} job={j} live={live[j.key]} reload={reload} />
              ))}
          </Box>
        </Section.Root>
      ))}
      {data && jobs.length === 0 ? (
        <EmptyState.Root icon="clock-bolt">
          <EmptyState.Title>{t('jobs.empty')}</EmptyState.Title>
        </EmptyState.Root>
      ) : null}
    </>
  );
}

/** The 115 module's own tasks on the same console: the core job rows say when
 *  they last fired, while this says what the mirror is doing right now. */
function ModuleTaskProgress() {
  const t = useT();
  const kroma = useKromaClient();
  const { data } = usePoll(
    ['admin', 'module', 'cloud115', 'status'],
    async () => {
      const res = await fetch(`${apiBase()}/api/module/tv.kroma.cloud115/status`, {
        headers: { Authorization: `Bearer ${kroma.sessionToken}` },
      });
      if (!res.ok) {
        throw new Error(`HTTP ${res.status}`);
      }
      return (await res.json()) as CloudStatus;
    },
    4000,
  );
  if (!data) {
    return null;
  }
  const report = data.report ?? {};
  const poll = data.lastPollAt
    ? new Date(data.lastPollAt * 1000).toLocaleTimeString()
    : '—';
  const sync = data.running
    ? t('jobs.module115.syncRunning', {
        dir: report.current || '—',
        dirs: report.dirs ?? 0,
        files: report.files ?? 0,
      })
    : t('jobs.module115.syncIdle', {
        time: report.finishedAt
          ? new Date(report.finishedAt * 1000).toLocaleTimeString()
          : '—',
        written: report.written ?? 0,
        pruned: report.pruned ?? 0,
      });
  return (
    <Section.Root mt={28}>
      <Section.Header>
        <Section.Title>{t('jobs.module115.title')}</Section.Title>
      </Section.Header>
      <Surface elevated border="border" pad="none" px={20} py={16}>
        <Row between wrap gap={12}>
          <Text variant="cardTitle">{t('jobs.module115.watch')}</Text>
          <Text variant="meta" color="textDim">
            {t('jobs.module115.watchLine', { time: poll, events: data.lastEvents ?? 0 })}
          </Text>
        </Row>
        <Row between wrap gap={12} mt={10}>
          <Text variant="cardTitle">{t('jobs.module115.sync')}</Text>
          <Text variant="meta" color={data.running ? 'accentText' : 'textDim'}>
            {sync}
          </Text>
        </Row>
        <Row justify="flex-end" mt={12}>
          <Button variant="ghost" size="sm" label={t('jobs.module115.open')} asChild>
            <a href="/admin/cloud115" />
          </Button>
        </Row>
      </Surface>
    </Section.Root>
  );
}
