import { Badge, Box, Text } from '@kromatv/ui/kit';
import { useT } from '@kromatv/ui';

// The card that describes a season the library does not hold. It is a reading
// surface, not a control: the request itself is filed from the missing-request
// block below, where its scope is confirmed first. The poster falls back to the
// show's own art (the provider has no season portrait in this payload) and then
// to a numbered plate, so the card is never a grey hole.

import type { TitleSeason } from '#web/shared/lib/title-view';
import { Image } from '#web/shared/ui';
import { RequestStatusChip } from '#web/shared/ui/request-status-chip.component';

export interface SeasonRequestCardProps {
  season: TitleSeason;
  /** The show's poster, already resolved; a season portrait when one exists. */
  poster: string | null;
  overview: string | null;
  year: number | null;
}

/** One season's facts, for a season that is not in the library. */
export function SeasonRequestCard({
  season,
  poster,
  overview,
  year,
}: Readonly<SeasonRequestCardProps>) {
  const t = useT();
  const partial = !season.available && season.episodesAvailable > 0;
  const epLabel = partial
    ? t('discover.episodesPartial', {
        have: String(season.episodesAvailable),
        total: String(season.episodeCount),
      })
    : t('discover.episodesN', { n: String(season.episodeCount) });
  const name = season.name ?? t('discover.seasonN', { n: String(season.number) });
  const airYear = season.airDate ? Number(season.airDate.slice(0, 4)) : null;
  const facts = [airYear || year, epLabel].filter(Boolean).join(' · ');

  return (
    <Box row align="flex-start" gap={16} p={16} radius="xl" bg="tint/2.5" border="border">
      <Box
        w={88}
        aspect={2 / 3}
        shrink={0}
        center
        overflow="hidden"
        radius="md"
        bg="tint/5"
      >
        {poster ? (
          <Image src={poster} fit="cover" fill />
        ) : (
          <Text variant="h2" color="textDim">
            {season.number}
          </Text>
        )}
      </Box>
      <Box minW={0} flex gap={6}>
        <Box row align="center" wrap gap={8}>
          <Text variant="title" lines={2}>
            {name}
          </Text>
          {season.requested ? (
            <RequestStatusChip status="pending" size="row" />
          ) : (
            <Badge tone="neutral">{t('content.seasonNotInLibrary')}</Badge>
          )}
        </Box>
        <Text variant="meta" color="textDim">
          {facts}
        </Text>
        {overview ? (
          <Text variant="meta" color="textMuted" lines={4}>
            {overview}
          </Text>
        ) : null}
      </Box>
    </Box>
  );
}
