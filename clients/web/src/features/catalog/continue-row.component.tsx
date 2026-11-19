import { useWebRuntime } from '#web/shared/lib/web-runtime';
// "Reprendre la lecture" rail. Unlike the catalogue rails (SSR-loaded, public),
// this is per-user so it loads client-side once a session is hydrated, then
// renders resumable items with a progress bar straight to the player.

import type { ContinueItem } from '@kromatv/client/media';
import { episodeTag, posterColors } from '@kromatv/core';
import { useT } from '@kromatv/ui';
import {
  ArtScrim,
  Box,
  color,
  Focusable,
  motion,
  Progress,
  styles,
  Text,
  useStyles,
  VirtualRail,
} from '@kromatv/ui/kit';
import { useSuspenseQuery } from '@tanstack/react-query';
import { Suspense, useState } from 'react';
import { SectionHeading } from '#web/features/catalog/home/section-heading.component';
import { useAuth } from '#web/shared/lib/auth';

import { Image, RailSkeleton } from '#web/shared/ui';
import { RouteLink } from '#web/shared/ui/route-link.component';

// The same landscape card the home rails draw, so resuming sits beside the
// curated rows instead of opening a second, portrait poster language. A still
// (episode) or backdrop (film) fills the 16:9 frame; the poster only steps in
// when the server holds no wide art at all.
const CARD_W = 320;
const GAP = 18;
const PAD = 12;
const CARD_H = Math.round((CARD_W * 9) / 16);

const sDefinition = styles({
  rail: { height: CARD_H + PAD * 2 },
  content: { py: PAD },
  hit: { w: '100%', radius: 'xl' },
  lift: {
    transitionProperty: 'transform',
    transitionDuration: `${motion.duration.base}ms`,
  },
  lifted: { transform: [{ translateY: motion.focusLift }] },
  art: {
    aspect: 16 / 9,
    radius: 'xl',
    overflow: 'hidden',
    outlineStyle: 'solid',
    outlineWidth: 3,
    outlineColor: 'transparent',
    transitionProperty: 'outline-color',
    transitionDuration: `${motion.duration.base}ms`,
  },
  artLit: { outlineColor: color('accent') },
  caption: { position: 'absolute', left: 14, right: 14, bottom: 18 },
  progress: { position: 'absolute', left: 0, right: 0, bottom: 0 },
});

export function ContinueRow() {
  const { user, ready } = useAuth();
  // Per-user data: don't fetch behind the login gate. A skeleton fills the rail
  // while it loads, then collapses to nothing if there's nothing to resume.
  if (!ready || !user) {
    return null;
  }
  return (
    <Suspense fallback={<RailSkeleton count={6} />}>
      <ContinueRail />
    </Suspense>
  );
}

function continueProgress(row: ContinueItem): number {
  const duration = row.durationMs ?? row.item.durationMs ?? 0;
  if (duration <= 0) {
    return 0;
  }
  return Math.min(100, Math.round((row.positionMs / duration) * 100));
}

/** The caption and the accessible name: a film is its own title, a resumed
 *  episode is "show · SxxExx · episode" so the card never reads as the whole
 *  series poster it is not. */
function continueFace(
  item: ContinueItem['item'],
  filmLabel: string,
): { overline: string; title: string; label: string } {
  if (item.kind !== 'episode') {
    const title = item.title.trim();
    return { overline: filmLabel, title, label: title };
  }
  const show = item.showTitle?.trim() || null;
  const tag = episodeTag(item);
  const title = (item.episodeTitle ?? item.title).trim();
  return {
    overline: [show, tag].filter(Boolean).join(' · '),
    title,
    label: [show, tag, title].filter(Boolean).join(' · '),
  };
}

function ContinueCard({ row }: Readonly<{ row: ContinueItem }>) {
  const s = useStyles(sDefinition);
  const t = useT();
  const { client } = useAuth();
  const [hovered, setHovered] = useState(false);

  const item = row.item;
  const { overline, title, label } = continueFace(item, t('content.film'));
  const progress = continueProgress(row);
  const art =
    client.media.artwork.backdropFor(item, CARD_W * 2) ??
    client.media.artwork.posterFor(item, CARD_W * 2);

  return (
    <Box
      w="100%"
      maxW={CARD_W}
      self="center"
      onPointerEnter={() => setHovered(true)}
      onPointerLeave={() => setHovered(false)}
    >
      <Box style={hovered ? [s.lift, s.lifted] : s.lift}>
        <Focusable label={label} style={s.hit} asChild>
          <RouteLink to="/watch/$id" params={{ id: item.id }}>
            <Box style={[s.art, hovered ? s.artLit : null]}>
              <Image
                src={art}
                alt={title}
                background={`linear-gradient(158deg, ${posterColors(item.id).join(', ')})`}
                fit="cover"
                position="center 24%"
                fill
              />
              <ArtScrim variant="deep" />
              <Box style={s.caption}>
                {overline ? (
                  <Text variant="overline" color="accent" lines={1} mb={4}>
                    {overline}
                  </Text>
                ) : null}
                <Text variant="label" color="white" lines={1}>
                  {title}
                </Text>
              </Box>
              <Box style={s.progress}>
                <Progress
                  value={progress / 100}
                  thickness={4}
                  trackColor="white/20"
                  rounded={false}
                  label={t('content.progressWatched', { percent: progress })}
                />
              </Box>
            </Box>
          </RouteLink>
        </Focusable>
      </Box>
    </Box>
  );
}

function ContinueRail() {
  const webRuntime = useWebRuntime();

  const t = useT();
  const s = useStyles(sDefinition);
  const { data: items } = useSuspenseQuery(webRuntime.queries.userQueries.continueWatching());

  if (items.length === 0) {
    return null;
  }

  return (
    <section>
      <SectionHeading title={t('content.continueWatching')} />
      <VirtualRail
        data={items}
        itemWidth={CARD_W + GAP}
        gap={GAP}
        style={s.rail}
        contentStyle={s.content}
        renderItem={(row) => <ContinueCard row={row} />}
      />
    </section>
  );
}
