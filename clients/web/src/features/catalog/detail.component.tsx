import type { CrewMember, ItemId, MediaItem } from '@kromatv/client/media';
import { canDirectPlay, posterColors } from '@kromatv/core';
import { useT, useThemeAudio } from '@kromatv/ui';
import {
  BackButton,
  Badge,
  Box,
  Button,
  ExpandableText,
  Ground,
  type HostElement,
  IconButton,
  styles,
  Text,
  useStyles,
} from '@kromatv/ui/kit';
import { type ReactNode, useSyncExternalStore } from 'react';
import {
  HeroFields,
  ListButton,
  ReportButton,
  WatchedButton,
} from '#web/features/catalog/detail-hero-parts.component';
import { HeroBackdrop } from '#web/features/catalog/hero-backdrop.component';
import layout from '#web/features/catalog/hero-layout.module.css';
import type { QualityTone } from '#web/shared/lib/quality-badges';
import { Image } from '#web/shared/ui';
import { CastButton } from '#web/shared/ui/cast-button.component';

export type { SimilarItem } from '#web/features/catalog/detail-rails';
export { CastRail, initials, SimilarRail } from '#web/features/catalog/detail-rails';
export {
  audioFlagLabel,
  audioString,
  langName,
  subString,
} from '#web/features/catalog/media-labels';
export type { QualityTone } from '#web/shared/lib/quality-badges';
export { qualityBadges } from '#web/shared/lib/quality-badges';

const sDefinition = styles({
  noCaps: { textTransform: 'none' },
  tagline: { fontWeight: '600' },
  overview: { fontSize: 'clamp(14px, 1.35cqw, 17px)', lineHeight: '1.7em', maxWidth: '48em' },
});

const subscribeNever = () => () => {};
const CORNER_LEFT = { base: 16, md: 32 } as const;
const CORNER_TOP = { base: 16, md: 20 } as const;

export interface DetailHeroProps {
  art: { id: string; backdrop: string | null; poster: string };
  overline: string;
  title: string;
  rating?: number | null;
  meta: string;
  badges: QualityTone[];
  audioFlag?: string | null;
  directors?: CrewMember[];
  tagline?: string | null;
  overview?: string | null;
  audio?: string;
  subtitles?: string;
  playLabel?: string;
  primaryAction?: ReactNode;
  back: HostElement;
  onPlay?: () => void;
  castItemId?: ItemId;
  watched?: boolean;
  onToggleWatched?: () => void;
  inList?: boolean;
  onToggleList?: () => void;
  playable?: MediaItem | null;
  themeUrl?: string | null;
  adminAction?: ReactNode;
  trailerAction?: ReactNode;
  onReport?: () => void;
}

/** A finite scene, followed by a normal-flow fiche shared by movies and series. */
export function DetailHero({
  art, overline, title, rating, meta, badges, audioFlag, directors, tagline, overview,
  audio, subtitles, playLabel, primaryAction, back, onPlay, castItemId, watched,
  onToggleWatched, inList, onToggleList, playable, themeUrl, adminAction, trailerAction, onReport,
}: Readonly<DetailHeroProps>) {
  const s = useStyles(sDefinition);
  const t = useT();
  const [c1, c2] = posterColors(art.id);
  const heroGradient = `linear-gradient(135deg, ${c1}, ${c2})`;
  const theme = useThemeAudio(themeUrl);

  // Codec support belongs to the viewer's browser, not the SSR environment.
  const unsupported = useSyncExternalStore(
    subscribeNever,
    () => {
      if (!playable) return null;
      const v = canDirectPlay(playable);
      return v.canDirectPlay ? null : t(v.messageKey, v.messageVars);
    },
    () => null,
  );

  return (
    <div className={layout.detail}>
      <header className={`${layout.mast} ${layout.dmast}`}>
        <HeroBackdrop backdrop={art.backdrop} gradient={heroGradient} />
        <Box absolute z={3} left={CORNER_LEFT} top={CORNER_TOP}>
          <Ground tone="dark">
            <BackButton diameter={42} label={t('common.back')} asChild>{back}</BackButton>
          </Ground>
        </Box>
        <ThemeToggle theme={theme} />
        <div className={layout.detailTitle}>
          <Text variant="overline" style={s.noCaps}>{overline}</Text>
          <h1 className={layout.title}>{title}</h1>
        </div>
      </header>

      <div className={layout.detailBody}>
        <div className={layout.poster}>
          <Image src={art.poster} background={heroGradient} fit="cover" fill />
        </div>
        <div className={layout.info}>
          <div className={layout.lead}>
            <Box row wrap align="center" gap={8}>
              {rating ? <Text variant="label" color="accentText">{`${rating.toFixed(1)}★`}</Text> : null}
              <Text variant="meta" color="textMuted">{meta}</Text>
              {badges.map((b) => <Badge key={b} tone={b}>{b}</Badge>)}
              {audioFlag ? <Badge tone="warning">{audioFlag}</Badge> : null}
            </Box>
            <Box row wrap align="center" gap={8} mt={12} mb={16}>
              {primaryAction ?? (onPlay ? (
                <Button icon="player-play-filled" label={playLabel ?? t('content.play')} onPress={onPlay} />
              ) : null)}
              {trailerAction}
              {castItemId ? <CastButton itemId={castItemId} /> : null}
              <WatchedButton watched={watched} onToggle={onToggleWatched} />
              <ListButton inList={inList} onToggle={onToggleList} />
              <ReportButton onReport={onReport} />
              {adminAction}
            </Box>
          </div>
          <div className={layout.readout}>
            {tagline ? <Text variant="meta" mb={6} style={s.tagline}>{tagline}</Text> : null}
            {overview ? (
              <ExpandableText variant="body" color="textMuted" lines={4} moreLabel={t('person.readMore')} style={s.overview}>
                {overview}
              </ExpandableText>
            ) : null}
            <HeroFields directors={directors} audio={audio} subtitles={subtitles} />
            {unsupported ? <Text variant="meta" color="textMuted" mt={10}>{unsupported}</Text> : null}
          </div>
        </div>
      </div>
    </div>
  );
}

function ThemeToggle({ theme }: Readonly<{ theme: ReturnType<typeof useThemeAudio> }>) {
  const t = useT();
  if (!theme.active) return null;
  return (
    <Box absolute z={3} right={CORNER_LEFT} top={CORNER_TOP}>
      <Ground tone="dark">
        <IconButton
          variant="scrim"
          diameter={42}
          glyph={19}
          icon={theme.muted ? 'volume-off' : 'volume'}
          label={theme.muted ? t('content.unmuteTheme') : t('content.muteTheme')}
          onPress={theme.toggle}
        />
      </Ground>
    </Box>
  );
}
