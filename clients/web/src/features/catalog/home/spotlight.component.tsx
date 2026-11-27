import { posterColors, synopsisLead } from '@kromatv/core';
import { useT } from '@kromatv/ui';
import { Button, Row, Text } from '@kromatv/ui/kit';
import { HeroArt, HeroIntro } from '#web/features/catalog/hero-art.component';
import layout from '#web/features/catalog/hero-layout.module.css';
import {
  entryBackdrop,
  entryFacts,
  entryId,
  entryMetadata,
  entryTitle,
  type HomeEntry,
  type HomeSection,
} from '#web/features/catalog/home/entries';
import { entryLink } from '#web/features/catalog/home/entry-link';
import { useAuth } from '#web/shared/lib/auth';
import { RouteLink } from '#web/shared/ui/route-link.component';

function SpotlightActions({ entry }: Readonly<{ entry: HomeEntry }>) {
  const t = useT();
  return (
    <Row wrap gap={8}>
      {entry.type === 'movie' ? (
        <Button size="sm" icon="player-play-filled" label={t('content.play')} asChild>
          <RouteLink to="/watch/$id" params={{ id: entry.item.id }} />
        </Button>
      ) : null}
      <Button size="sm" variant={entry.type === 'movie' ? 'glass' : undefined} label={t('content.moreInfo')} asChild>
        {entryLink(entry)}
      </Button>
    </Row>
  );
}

/** One editorial pick. On phones its copy follows the faded edge of the art. */
export function Spotlight({ section }: Readonly<{ section: HomeSection }>) {
  const { client } = useAuth();
  const t = useT();
  const entry = section.items[0];
  if (!entry) return null;
  const meta = entryMetadata(entry);
  const synopsis = synopsisLead(meta?.overview, 220);
  const note = section.notes[entryId(entry)] ?? section.reason;

  return (
    <section className={layout.spotlight} aria-label={section.title}>
      <div className={`${layout.mast} ${layout.band}`}>
        <div className={layout.spotArt}>
          <HeroArt src={entryBackdrop(client, entry)} background={`linear-gradient(158deg, ${posterColors(entryId(entry)).join(', ')})`} veil="spotlight" priority={false} />
        </div>
        <div className={layout.spotCopy}>
          <Row wrap gap={8}>
            <Text variant="overline">{section.title}</Text>
            {note ? <Text variant="overline" color="textMuted">{note}</Text> : null}
          </Row>
          <h2 className={layout.title}>{entryTitle(entry)}</h2>
          <Row wrap gap={8} mb={10}>
            {meta?.rating ? <Text variant="meta" color="accentText">{`${meta.rating.toFixed(1)}★`}</Text> : null}
            <Text variant="meta" color="textMuted">{entryFacts(t, entry).join(' · ')}</Text>
          </Row>
          {synopsis ? <HeroIntro>{synopsis}</HeroIntro> : null}
          <SpotlightActions entry={entry} />
        </div>
      </div>
    </section>
  );
}
