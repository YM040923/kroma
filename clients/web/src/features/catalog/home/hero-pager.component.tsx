import { posterColors } from '@kromatv/core';
import { useT } from '@kromatv/ui';
import { Box, Button, Focusable, IconButton, styles, useBreakpoint, useStyles } from '@kromatv/ui/kit';
import layout from '#web/features/catalog/hero-layout.module.css';
import {
  entryBackdrop,
  entryId,
  entryTitle,
  type HomeEntry,
} from '#web/features/catalog/home/entries';
import { type Carousel, HOLD_MS } from '#web/features/catalog/home/use-carousel.hook';
import { useAuth } from '#web/shared/lib/auth';
import { Image } from '#web/shared/ui';

const sDefinition = styles({
  // The strip scrolls inside its remaining space. The compact indicator keeps
  // a real hit target even though its visible bar is only four pixels high.
  stop: { width: 'var(--stop-width)', shrink: 0, minH: 32, center: true, radius: 'badge' },
  timer: {
    absolute: true, left: 0, right: 0, bottom: 0, h: 2, bg: 'accent', transformOrigin: 'left',
    animationKeyframes: 'kroma-fill', animationDuration: `${HOLD_MS}ms`,
    animationTimingFunction: 'linear', animationFillMode: 'both',
  },
  pill: { radius: 'pill' },
});

export interface HeroPagerProps {
  entries: readonly HomeEntry[];
  carousel: Carousel;
  onShuffle?: () => void;
}

function PagerThumb({ entry }: Readonly<{ entry: HomeEntry }>) {
  const { client } = useAuth();
  return (
    <div className={layout.thumbArt}>
      <Image
        src={entryBackdrop(client, entry, 240)}
        background={`linear-gradient(158deg, ${posterColors(entryId(entry)).join(', ')})`}
        fit="cover"
        fill
      />
    </div>
  );
}

function PagerStop({ entry, index, carousel, pictures }: Readonly<{
  entry: HomeEntry;
  index: number;
  carousel: Carousel;
  pictures: boolean;
}>) {
  const s = useStyles(sDefinition);
  const t = useT();
  const up = index === carousel.index;
  return (
    <Focusable
      label={t('content.heroShow', { title: entryTitle(entry) })}
      pressed={up}
      onPress={() => carousel.go(index)}
      style={s.stop}
    >
      <div className={layout.thumb}>
        {pictures ? <PagerThumb entry={entry} /> : null}
        {up && carousel.running ? <Box pointerEvents="none" style={s.timer} /> : null}
      </div>
    </Focusable>
  );
}

/** Shuffle is a batch action, not a stop: a single title can still be replaced. */
export function HeroPager({ entries, carousel, onShuffle }: Readonly<HeroPagerProps>) {
  const s = useStyles(sDefinition);
  const t = useT();
  const pictures = useBreakpoint() !== 'base';
  const paging = entries.length > 1;
  if (!paging && !onShuffle) return null;

  return (
    <div className={layout.pager}>
      {paging ? (
        <div className={layout.stops}>
          {entries.map((entry, index) => (
            <PagerStop key={entryId(entry)} entry={entry} index={index} carousel={carousel} pictures={pictures} />
          ))}
        </div>
      ) : null}
      <div className={layout.controls}>
        {onShuffle ? (
          <Button variant="glass" size="sm" style={s.pill} icon="arrows-shuffle" label={t('content.heroShuffle')} onPress={onShuffle} />
        ) : null}
        {paging ? (
          <>
            <IconButton diameter={36} icon="chevron-left" label={t('content.heroPrevious')} onPress={() => carousel.step(-1)} />
            <IconButton diameter={36} icon="chevron-right" label={t('content.heroNext')} onPress={() => carousel.step(1)} />
          </>
        ) : null}
      </div>
    </div>
  );
}
