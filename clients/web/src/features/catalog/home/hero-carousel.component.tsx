import { posterColors } from '@kromatv/core';
import { useEffect, useEffectEvent, useState } from 'react';
import {
  entryBackdrop,
  entryId,
  type HomeSection,
} from '#web/features/catalog/home/entries';
import { HeroPager } from '#web/features/catalog/home/hero-pager.component';
import { HeroSlide } from '#web/features/catalog/home/hero-slide.component';
import { type Carousel, useCarousel } from '#web/features/catalog/home/use-carousel.hook';
import { HeroArt } from '#web/features/catalog/hero-art.component';
import layout from '#web/features/catalog/hero-layout.module.css';
import { useAuth } from '#web/shared/lib/auth';
import { useReducedMotion } from '#web/shared/lib/use-reduced-motion.hook';

function useListener<K extends keyof HTMLElementEventMap>(
  node: HTMLElement | null,
  type: K,
  handle: (event: HTMLElementEventMap[K]) => void,
) {
  const handleLatest = useEffectEvent(handle);
  useEffect(() => {
    if (!node) {
      return undefined;
    }
    const listener = (event: HTMLElementEventMap[K]) => handleLatest(event);
    node.addEventListener(type, listener);
    return () => node.removeEventListener(type, listener);
  }, [node, type]);
}

function useArrowKeys(node: HTMLElement | null, carousel: Carousel) {
  useListener(node, 'keydown', (event) => {
    if (event.key === 'ArrowRight') {
      carousel.step(1);
    } else if (event.key === 'ArrowLeft') {
      carousel.step(-1);
    }
  });
}

function useResting(node: HTMLElement | null) {
  const still = useReducedMotion();
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  useListener(node, 'pointerenter', () => setHovered(true));
  useListener(node, 'pointerleave', () => setHovered(false));
  useListener(node, 'focusin', () => setFocused(true));
  useListener(node, 'focusout', () => setFocused(false));
  return hovered || focused || still;
}

function useHeroCarousel(count: number) {
  const [node, setNode] = useState<HTMLElement | null>(null);
  const carousel = useCarousel(count, { resting: useResting(node) });
  useArrowKeys(node, carousel);
  return { setNode, carousel };
}

interface HeroCarouselProps {
  section: HomeSection;
  onShuffle?: () => void;
}

/** The few titles the home opens on, one at a time: it pages by itself, by
 * the arrow keys, and by the strip of the titles still to come. */
export function HeroCarousel({ section, onShuffle }: Readonly<HeroCarouselProps>) {
  const entries = section.items;
  const { setNode, carousel } = useHeroCarousel(entries.length);
  const entry = entries[carousel.index];
  const { client } = useAuth();

  if (!entry) {
    return null;
  }

  return (
    <section
      ref={setNode}
      className={layout.bleed}
      aria-roledescription="carousel"
      aria-label={section.title}
    >
      <div className={`${layout.mast} ${layout.home}`}>
        <HeroArt
          src={entryBackdrop(client, entry)}
          background={`linear-gradient(158deg, ${posterColors(entryId(entry)).join(', ')})`}
        />
        <HeroSlide
          key={entryId(entry)}
          entry={entry}
          note={section.notes[entryId(entry)] ?? section.title}
          current={carousel.index + 1}
          total={entries.length}
        />
        <HeroPager entries={entries} carousel={carousel} onShuffle={onShuffle} />
      </div>
    </section>
  );
}
