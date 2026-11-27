import { styles, Text, useStyles } from '@kromatv/ui/kit';
import { useEffect, useRef, useState } from 'react';
import { apiBase } from '#web/shared/lib/api';
import { Image } from '#web/shared/ui/image.component';
import layout from './hero-layout.module.css';

const veils = { home: layout.homeVeil, detail: layout.detailVeil, spotlight: layout.spotVeil };
const textStyles = styles({ intro: { fontSize: 'clamp(13px, 1.27cqw, 17px)', lineHeight: '1.6em' } });

/** Home recommendations tease the synopsis; the detail page owns the full text. */
export function HeroIntro({ children }: Readonly<{ children: string }>) {
  const s = useStyles(textStyles);
  return (
    <Text variant="body" lines={2} mb={15} style={s.intro}>
      {children}
    </Text>
  );
}

// Server renditions; unlike tiles, a hero wider than the ladder needs the master.
const widths = [160, 240, 320, 480, 780, 960, 1280];

function heroSource(src: string | null, pixels: number) {
  if (!src || src.includes('?') || src.includes('#')) return src;
  const prefix = `${apiBase()}/api/images/`;
  if (!src.startsWith('/api/images/') && !src.startsWith(prefix)) return src;
  if (!pixels) return null;
  const width = widths.find((w) => w >= pixels);
  return width ? `${src}?w=${width}` : src;
}

/** Render the gradient during SSR/hydration rather than fetching a guessed size. */
export function HeroArt({ src, background, position = '50% 28%', veil = 'home', priority = true }: Readonly<{
  src: string | null;
  background?: string;
  position?: string;
  veil?: 'home' | 'detail' | 'spotlight';
  priority?: boolean;
}>) {
  const ref = useRef<HTMLDivElement>(null);
  const [pixels, setPixels] = useState(0);
  useEffect(() => {
    const frame = ref.current;
    if (!frame) return;
    const viewport = window.visualViewport;
    const measure = () => setPixels(frame.getBoundingClientRect().width * (window.devicePixelRatio || 1) * (viewport?.scale || 1));
    let resolution: MediaQueryList | undefined;
    const watchResolution = () => {
      resolution?.removeEventListener('change', watchResolution);
      resolution = window.matchMedia?.(`(resolution: ${window.devicePixelRatio || 1}dppx)`);
      resolution?.addEventListener('change', watchResolution);
      measure();
    };
    watchResolution();
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measure);
    observer?.observe(frame);
    window.addEventListener('resize', measure);
    viewport?.addEventListener('resize', measure);
    return () => {
      observer?.disconnect();
      resolution?.removeEventListener('change', watchResolution);
      window.removeEventListener('resize', measure);
      viewport?.removeEventListener('resize', measure);
    };
  }, []);
  return (
    <div ref={ref} className={layout.art} aria-hidden="true">
      <Image src={pixels ? heroSource(src, pixels) : null} background={background} position={position} fill priority={priority} />
      <div className={`${layout.veil} ${veils[veil]}`} />
    </div>
  );
}
