import { metaLine, posterColors, synopsisLead } from '@kromatv/core';
import { useT } from '@kromatv/ui';
import { Badge, Box, Button, Row, Skeleton, Text } from '@kromatv/ui/kit';
import { HeroArt, HeroIntro } from '#web/features/catalog/hero-art.component';
import layout from '#web/features/catalog/hero-layout.module.css';
import type { MovieCardView } from '#web/shared/lib/api';
import type { HeroEntry } from '#web/shared/lib/queries';
import { RouteLink } from '#web/shared/ui/route-link.component';

type HeroBadge = '4K' | 'HDR' | 'H.265';

function heroBadges(v: MovieCardView['video']): HeroBadge[] {
  if (!v) return [];
  const out: HeroBadge[] = [];
  if ((v.width ?? 0) >= 3840) out.push('4K');
  if (v.hdr) out.push('HDR');
  if (v.codec === 'hevc') out.push('H.265');
  return out;
}

/** The fallback pick uses the same canvas as the carousel, without a pager. */
export function Hero({ entry }: Readonly<{ entry: HeroEntry }>) {
  const t = useT();
  const media = entry.type === 'movie' ? entry.movie : entry.show;
  const gradient = `linear-gradient(158deg, ${posterColors(media.id).join(', ')})`;
  const meta = media.metadata;
  const synopsis = synopsisLead(meta?.overview, 240);
  const badges = heroBadges(media.video);
  const line = entry.type === 'movie'
    ? metaLine(entry.movie)
    : [entry.show.year, t('content.seasonCount', { count: entry.show.seasonCount })]
        .filter(Boolean).join(' · ');

  return (
    <section className={layout.bleed} aria-label={t('content.featured')}>
      <div className={`${layout.mast} ${layout.home}`}>
        <HeroArt src={media.backdrop} background={gradient} />
        <div className={layout.homeCopy}>
          <Text variant="overline">{t('content.featured')}</Text>
          <h1 className={layout.title}>{media.title}</h1>
          <Row wrap gap={8} mb={10}>
            {meta?.rating ? <Text variant="meta" color="accentText">{`${meta.rating.toFixed(1)}★`}</Text> : null}
            <Text variant="meta" color="textMuted">{line}</Text>
            {badges.map((b) => <Badge key={b} tone={b}>{b}</Badge>)}
          </Row>
          {synopsis ? <HeroIntro>{synopsis}</HeroIntro> : null}
          <Row wrap gap={8}>
            {entry.type === 'movie' ? (
              <Button size="sm" icon="player-play-filled" label={t('content.play')} asChild>
                <RouteLink to="/watch/$id" params={{ id: media.id }} />
              </Button>
            ) : null}
            <Button size="sm" variant={entry.type === 'movie' ? 'glass' : undefined} icon="info-circle" label={t('content.moreInfo')} asChild>
              {entry.type === 'movie' ? (
                <RouteLink to="/movies/$id" params={{ id: media.id }} />
              ) : (
                <RouteLink to="/shows/$id" params={{ id: media.id }} />
              )}
            </Button>
          </Row>
        </div>
      </div>
    </section>
  );
}

export function HeroSkeleton() {
  return (
    <div className={layout.bleed} aria-hidden>
      <div className={`${layout.mast} ${layout.home}`}>
        <Box fill>
          <Skeleton h="100%" w="100%" radius={0} />
        </Box>
      </div>
    </div>
  );
}
