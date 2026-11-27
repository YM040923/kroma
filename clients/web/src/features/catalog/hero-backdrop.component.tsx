import { HeroArt } from '#web/features/catalog/hero-art.component';

export function HeroBackdrop({
  backdrop,
  gradient,
}: Readonly<{ backdrop: string | null; gradient: string }>) {
  return <HeroArt src={backdrop} background={gradient} veil="detail" />;
}
