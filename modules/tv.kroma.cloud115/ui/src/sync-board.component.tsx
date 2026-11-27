import { useT } from '@kromatv/module-sdk';
import { Badge, Box, Progress, Row, Text } from '@kromatv/ui/kit';
import type { CloudHistoryEntry, CloudStatusView, MappingProgress, SyncReport } from './schemas';

function MappingRows({ mappings }: { mappings: MappingProgress[] }) {
  const t = useT();
  return (
    <Box gap={12}>
      <Text variant="cardTitle">
        {t('cloud115.rootsCompleted', {
          done: mappings.filter((m) => m.state === 'completed').length,
          total: mappings.length,
        })}
      </Text>
      {mappings.map((m, index) => {
        const dirs =
          m.totalDirs === null
            ? t('cloud115.dirsDiscovered', { count: m.discoveredDirs })
            : t('cloud115.dirsProcessed', { done: m.doneDirs, total: m.totalDirs });
        const errors = t('cloud115.mappingErrors', { count: m.errorCount });
        return (
          <Box key={JSON.stringify([index, m.remote, m.localDisplay])} gap={4}>
            <Row wrap gap={8} align="center">
              <Badge
                tone={
                  m.state === 'completed'
                    ? 'success'
                    : m.state === 'queued'
                      ? 'neutral'
                      : m.state === 'processing' || m.state === 'discovering'
                        ? 'info'
                        : 'warning'
                }
              >
                {t(`cloud115.state.${m.state}`)}
              </Badge>
              <Text variant="body">
                {t('cloud115.remote')}: {m.remote}
              </Text>
            </Row>
            <Text variant="meta" color="textDim">
              {t('cloud115.local')}: {m.localDisplay || '—'}
            </Text>
            {m.state !== 'queued' && (
              <Row wrap gap={12}>
                <Text variant="meta">{dirs}</Text>
                {m.state !== 'discovering' && (
                  <Text variant="meta" color="textMuted">
                    {t('cloud115.statFiles')} {m.files} · {t('cloud115.statWritten')} {m.written}
                  </Text>
                )}
              </Row>
            )}
            {m.totalDirs !== null && m.totalDirs > 0 && m.state !== 'queued' && (
              <Progress value={m.doneDirs / m.totalDirs} label={`${m.remote}: ${dirs}`} />
            )}
            {m.errorCount > 0 &&
              (m.errors.length ? (
                <details>
                  <summary style={{ cursor: 'pointer' }}>{errors}</summary>
                  <Box gap={6} mt={4}>
                    {m.errors.length < m.errorCount && (
                      <Text variant="meta" color="textDim">
                        {t('cloud115.errorsRecorded', {
                          count: m.errors.length,
                          total: m.errorCount,
                        })}
                      </Text>
                    )}
                    {m.errors.map((error, i) => (
                      <Text key={i} variant="meta">
                        {error.path} · {t(`cloud115.stage.${error.stage}`)} · {error.message}
                      </Text>
                    ))}
                  </Box>
                </details>
              ) : (
                <Text variant="meta">
                  {errors} · {t('cloud115.noErrorDetails')}
                </Text>
              ))}
          </Box>
        );
      })}
    </Box>
  );
}

function Totals({ entry }: { entry: CloudHistoryEntry | SyncReport }) {
  const t = useT();
  return (
    <Text variant="meta">
      {t('cloud115.historyLine', {
        dirs: entry.dirs,
        files: entry.files,
        written: entry.written,
        pruned: entry.pruned,
        errors: typeof entry.errors === 'number' ? entry.errors : entry.errors.length,
      })}
    </Text>
  );
}

export function SyncBoard({
  running,
  report,
  history,
}: Pick<CloudStatusView, 'running' | 'report' | 'history'>) {
  const t = useT();
  // A restart clears the in-memory report; the history snapshot belongs to the run,
  // unlike the editable settings or the freshly initialized report counters.
  const latest = history.at(-1);
  const run = !running && !report.mappings?.length && latest ? latest : report;
  const hasRun = running || run !== report || report.startedAt || report.finishedAt;
  return (
    <div style={{ minWidth: 0, overflowWrap: 'anywhere' }}>
      {run.mappings?.length ? (
        <MappingRows mappings={run.mappings} />
      ) : hasRun ? (
        <Box gap={4}>
          <Totals entry={run} />
          <Text variant="meta" color="textDim">
            {t('cloud115.noMappingDetails')}
          </Text>
        </Box>
      ) : (
        <Text variant="meta" color="textDim">
          {t('cloud115.never')}
        </Text>
      )}
      {run.truncated && (
        <Text variant="meta" color="accentText" mt={8}>
          {t('cloud115.truncated')}
        </Text>
      )}
      {history.length > 0 && (
        <>
          <Text variant="cardTitle" mt={18}>
            {t('cloud115.history')}
          </Text>
          <Box gap={8} mt={8}>
            {history
              .slice(-10)
              .reverse()
              .map((entry, i) => {
                const time = new Date(entry.at * 1000).toLocaleString();
                return entry.mappings?.length ? (
                  <details key={`${entry.at}:${history.length - i}`}>
                    <summary style={{ cursor: 'pointer' }}>
                      {time} · <Totals entry={entry} />
                    </summary>
                    <MappingRows mappings={entry.mappings} />
                  </details>
                ) : (
                  <Box gap={2} key={`${entry.at}:${history.length - i}`}>
                    <Text variant="meta" color="textDim">
                      {time}
                    </Text>
                    <Totals entry={entry} />
                    <Text variant="meta" color="textDim">
                      {t('cloud115.noMappingDetails')}
                    </Text>
                  </Box>
                );
              })}
          </Box>
        </>
      )}
    </div>
  );
}
