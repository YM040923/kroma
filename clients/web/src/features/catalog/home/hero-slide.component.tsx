import type { SubjectId } from '@kromatv/client/media';
import { qualityBadge, qualityBadgeForVideo, synopsisLead } from '@kromatv/core';
import { useT } from '@kromatv/ui';
import {
  Badge,
  Button,
  IconButton,
  qualityTone,
  Row,
  styles,
  Text,
  useStyles,
} from '@kromatv/ui/kit';
import {
  entryFacts,
  entryId,
  entryMetadata,
  entryTitle,
  type HomeEntry,
} from '#web/features/catalog/home/entries';
import { HeroIntro } from '#web/features/catalog/hero-art.component';
import layout from '#web/features/catalog/hero-layout.module.css';
import { entryLink } from '#web/features/catalog/home/entry-link';
import { useMyList } from '#web/shared/lib/mylist';
import { RouteLink } from '#web/shared/ui/route-link.component';

const SYNOPSIS_BUDGET = 240;

const sDefinition = styles({
  number: { fontVariantNumeric: 'tabular-nums' },
});

export interface HeroSlideProps {
  entry: HomeEntry;
  note: string;
  current: number;
  total: number;
}

function SlideFacts({ entry }: Readonly<{ entry: HomeEntry }>) {
  const t = useT();
  const rating = entryMetadata(entry)?.rating;
  const badge =
    entry.type === 'show' ? qualityBadgeForVideo(entry.show.video) : qualityBadge(entry.item);

  return (
    <Row wrap gap={8} mb={10}>
      {rating ? <Text variant="meta" color="accentText">{`${rating.toFixed(1)}★`}</Text> : null}
      <Text variant="meta" color="textMuted">
        {entryFacts(t, entry).join(' · ')}
      </Text>
      {badge ? <Badge tone={qualityTone(badge)}>{badge}</Badge> : null}
    </Row>
  );
}

function SlideSave({ id }: Readonly<{ id: SubjectId }>) {
  const t = useT();
  const { inList, toggle } = useMyList();
  const saved = inList(id);

  return (
    <IconButton
      diameter={37}
      variant="glass"
      icon={saved ? 'bookmark-filled' : 'bookmark'}
      label={t(saved ? 'content.removeFromList' : 'content.addToList')}
      pressed={saved}
      active={saved}
      onPress={() => toggle(id)}
    />
  );
}

function SlideActions({ entry }: Readonly<{ entry: HomeEntry }>) {
  const t = useT();

  return (
    <Row wrap gap={8}>
      {entry.type === 'movie' ? (
        <Button size="sm" icon="player-play-filled" label={t('content.play')} asChild>
          <RouteLink to="/watch/$id" params={{ id: entry.item.id }} />
        </Button>
      ) : null}
      <Button
        size="sm"
        variant={entry.type === 'movie' ? 'glass' : undefined}
        icon="info-circle"
        label={t('content.moreInfo')}
        asChild
      >
        {entryLink(entry)}
      </Button>
      <SlideSave id={entryId(entry)} />
    </Row>
  );
}

/** What one hero title says about itself, and the ways into it. */
export function HeroSlide({ entry, note, current, total }: Readonly<HeroSlideProps>) {
  const s = useStyles(sDefinition);
  const t = useT();
  const synopsis = synopsisLead(entryMetadata(entry)?.overview, SYNOPSIS_BUDGET);

  return (
    <div className={layout.homeCopy} role="group" aria-label={t('content.heroPosition', { current, total })} aria-roledescription="slide">
      <Row wrap gap={10}>
        <Text variant="overline">{note}</Text>
        {total > 1 ? (
          <Text variant="overline" aria-hidden style={s.number}>
            {`${String(current).padStart(2, '0')} / ${String(total).padStart(2, '0')}`}
          </Text>
        ) : null}
      </Row>
      <h1 className={layout.title}>{entryTitle(entry)}</h1>
      <SlideFacts entry={entry} />
      {synopsis ? <HeroIntro>{synopsis}</HeroIntro> : null}
      <SlideActions entry={entry} />
    </div>
  );
}
