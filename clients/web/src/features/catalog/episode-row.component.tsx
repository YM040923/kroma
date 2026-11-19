import {
  activeTheme,
  Badge,
  Box,
  backdropBlur,
  CheckboxFace,
  color,
  Focusable,
  IconButton,
  styles,
  sv,
  Text,
  useStyleScope,
  useStyles,
  useThemeVersion,
  WatchedBadge,
} from '@kromatv/ui/kit';
import { useWebRuntime } from '#web/shared/lib/web-runtime';

// One line of a season: an owned, playable episode, or a gap the viewer can
// request. The whole row is the control, so the trailing report / watched
// buttons are actions inside it rather than a second stop beside it.

import type { MediaItem } from '@kromatv/client/media';
import {
  datedDayLabel,
  formatRuntime,
  posterColors,
  relativeAirDate,
  sentenceCase,
} from '@kromatv/core';
import { useLocale, useT } from '@kromatv/ui';

import { IconPlayerPlayFilled } from '@tabler/icons-react';
import { ReportDialog } from '#web/features/catalog/report-dialog.component';

import { wash } from '#web/shared/lib/art-styles';
import type { SeasonGap } from '#web/shared/lib/title-view';
import { Image } from '#web/shared/ui';
import { RequestStatusChip } from '#web/shared/ui/request-status-chip.component';

const ROW_GAP = { base: 12, md: 20 } as const;
const STILL_W = { base: 128, md: 200 } as const;
const sDefinition = styles({
  bold: { fontWeight: '700' },
  dimmed: { opacity: 0.6 },
  frost: backdropBlur(2),
  lift: { ...backdropBlur(2), transform: [{ scale: 1.1 }] },
  scrim: {
    backgroundImage: `linear-gradient(170deg, ${color('black/5')}, ${color('black/45')})`,
  },
});

const episodeRow = sv({
  base: {
    row: true,
    align: 'center',
    gap: ROW_GAP,
    radius: 'xl',
    p: 14,
    bg: 'tint/3',
    border: 'border',
    _hover: { bg: 'tint/6' },
  },
  variants: {
    watched: { true: { border: 'accent/30' }, false: {} },
  },
  defaults: { watched: false },
});

export function EpisodeRow({
  episode,
  watched,
  progress,
  onPlay,
  onToggleWatched,
}: Readonly<{
  episode: MediaItem;
  progress: number | null;
  watched: boolean;
  onPlay: () => void;
  onToggleWatched: () => void;
}>) {
  const webRuntime = useWebRuntime();

  const scope = useStyleScope();
  useThemeVersion();

  const s = useStyles(sDefinition);

  const t = useT();
  const [g1, g2] = posterColors(episode.id);
  const runtime = formatRuntime(episode.durationMs);
  const synopsis = episode.metadata?.overview;
  const still = webRuntime.client.media.artwork.backdropFor(episode);
  const num =
    episode.season != null && episode.episode != null
      ? `S${String(episode.season).padStart(2, '0')}E${String(episode.episode).padStart(2, '0')} · `
      : '';
  const reportLabel = `${num}${episode.episodeTitle ?? episode.title}`;
  return (
    <Focusable
      sv={episodeRow}
      vars={{ watched }}
      label={episode.episodeTitle ?? episode.title}
      onPress={onPlay}
    >
      {({ hovered }) => (
        <>
          <Box
            w={STILL_W}
            aspect={16 / 9}
            shrink={0}
            center
            overflow="hidden"
            radius="md"
            style={wash(g1, g2, 135, scope)}
          >
            <Image src={still} fit="cover" fill style={watched ? s.dimmed : undefined} />
            <Box fill style={s.scrim} />
            {watched ? <WatchedBadge size={28} /> : null}
            <Box w={44} h={44} center radius="circle" bg="bg/50" style={hovered ? s.lift : s.frost}>
              <IconPlayerPlayFilled size={18} color={color('text', activeTheme(scope.theme))} />
            </Box>
            {progress != null && !watched ? (
              <Box absolute left={0} right={0} bottom={0} h={4} bg="tint/25">
                <Box h="100%" bg="accent" w={`${progress}%`} />
              </Box>
            ) : null}
          </Box>

          <Box minW={0} flex>
            <Box row align="center" gap={10} mb={6}>
              <Text variant="label" lines={1} color={watched ? 'textDim' : 'text'} style={s.bold}>
                {`${episode.episode}. ${episode.episodeTitle ?? episode.title}`}
              </Text>
              {runtime ? (
                <Text variant="meta" color="textDim" shrink={0}>
                  {runtime}
                </Text>
              ) : null}
            </Box>
            {synopsis ? (
              <Text variant="meta" color="textMuted" lines={2}>
                {synopsis}
              </Text>
            ) : null}
          </Box>

          <IconButton
            icon="flag"
            label={t('report.action')}
            onPress={() =>
              void ReportDialog.call({
                subjectKind: 'episode',
                subjectId: episode.id,
                subjectTitle: reportLabel,
              })
            }
          />
          <IconButton
            icon="check"
            active={watched}
            pressed={watched}
            label={watched ? t('content.markUnwatched') : t('content.markWatched')}
            onPress={onToggleWatched}
          />
        </>
      )}
    </Focusable>
  );
}

const missingRow = sv({
  base: {
    row: true,
    align: 'center',
    gap: ROW_GAP,
    radius: 'xl',
    p: 14,
    bg: 'tint/2',
    border: 'border',
    _hover: { bg: 'tint/5' },
  },
  variants: {
    selected: { true: { border: 'accent/45', bg: 'accentWash/8' }, false: {} },
  },
  defaults: { selected: false },
});

/** A gap in the season the viewer can ask for. The whole row is the toggle, so
 * picking three episodes is three presses and one request from the bar below;
 * a row already asked for shows its status instead. What the provider lists
 * for the episode (title, still, air date, synopsis, runtime) rides along once
 * enrichment has fetched the season; a row with no still borrows the show's
 * poster, then a plain numbered plate, rather than rendering a grey hole. */
export function MissingEpisodeRow({
  episode,
  listed,
  pending,
  selected,
  selectable = true,
  poster = null,
  onToggle,
}: Readonly<{
  season: number;
  episode: number;
  listed: SeasonGap | null;
  pending: boolean;
  selected: boolean;
  selectable?: boolean;
  poster?: string | null;
  onToggle: () => void;
}>) {
  const t = useT();
  const label = listed?.title
    ? `${episode}. ${listed.title}`
    : t('content.episodeN', { n: episode });
  if (pending) {
    return (
      <Box row align="center" gap={ROW_GAP} radius="xl" p={14} bg="tint/2" border="border">
        <MissingStill episode={episode} still={listed?.stillUrl ?? null} poster={poster} />
        <MissingText label={label} listed={listed} color="textMuted" />
        <RequestStatusChip status="pending" size="card" />
      </Box>
    );
  }
  if (!selectable) {
    return (
      <Box row align="center" gap={ROW_GAP} radius="xl" p={14} bg="tint/2" border="border">
        <MissingStill episode={episode} still={listed?.stillUrl ?? null} poster={poster} />
        <MissingText label={label} listed={listed} color="textMuted" />
      </Box>
    );
  }
  return (
    <Focusable
      role="checkbox"
      checked={selected}
      sv={missingRow}
      vars={{ selected }}
      label={label}
      onPress={onToggle}
    >
      <MissingStill episode={episode} still={listed?.stillUrl ?? null} poster={poster} />
      <MissingText label={label} listed={listed} color={selected ? 'text' : 'textMuted'} />
      <CheckboxFace checked={selected} />
    </Focusable>
  );
}

function runtimeMinOf(listed: SeasonGap | null): number {
  if (!listed || !('runtimeMin' in listed)) {
    return 0;
  }
  return listed.runtimeMin ?? 0;
}

function MissingText({
  label,
  listed,
  color: tone,
}: Readonly<{ label: string; listed: SeasonGap | null; color: 'text' | 'textMuted' }>) {
  const s = useStyles(sDefinition);

  const t = useT();
  const locale = useLocale();
  const airDate = listed?.airDate ?? null;
  const facts: string[] = [];
  if (airDate) {
    facts.push(
      sentenceCase(relativeAirDate(airDate, locale), locale),
      datedDayLabel(airDate, locale),
    );
  } else if (listed) {
    facts.push(t('requests.noDate'));
  }
  const runtime = formatRuntime(runtimeMinOf(listed) * 60_000);
  if (runtime) {
    facts.push(runtime);
  }
  return (
    <Box minW={0} flex>
      <Box row align="center" gap={10} mb={6}>
        <Text variant="label" lines={1} color={tone} style={s.bold}>
          {label}
        </Text>
        <Badge tone="neutral">{t('content.seasonNotInLibrary')}</Badge>
        {facts.length > 0 ? (
          <Text variant="meta" color="textDim" shrink={0}>
            {facts.join(' · ')}
          </Text>
        ) : null}
      </Box>
      {listed?.overview ? (
        <Text variant="meta" color="textMuted" lines={2}>
          {listed.overview}
        </Text>
      ) : null}
    </Box>
  );
}

function MissingStill({
  episode,
  still,
  poster,
}: Readonly<{ episode: number; still: string | null; poster: string | null }>) {
  const webRuntime = useWebRuntime();

  const s = useStyles(sDefinition);

  const art = webRuntime.client.media.artwork.resolve(still) ?? poster;
  return (
    <Box w={STILL_W} aspect={16 / 9} shrink={0} center overflow="hidden" radius="md" bg="tint/4">
      {art ? (
        <>
          <Image src={art} fit="cover" fill style={still ? s.dimmed : undefined} />
          <Box fill style={s.scrim} />
        </>
      ) : null}
      {still ? null : (
        <Text variant="label" color="textDim" style={s.bold}>
          {String(episode)}
        </Text>
      )}
    </Box>
  );
}
