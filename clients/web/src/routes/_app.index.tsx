import { useT } from '@kromatv/ui';
import { Button, EmptyState, styles, useStyles } from '@kromatv/ui/kit';
import { useQuery, useSuspenseQuery } from '@tanstack/react-query';
import { createFileRoute } from '@tanstack/react-router';
import { Suspense } from 'react';
import { ContinueRow } from '#web/features/catalog/continue-row.component';
import { Hero, HeroSkeleton } from '#web/features/catalog/featured-hero.component';
import { HeroCarousel } from '#web/features/catalog/home/hero-carousel.component';
import { HomeSectionView, SectionsSkeleton } from '#web/features/catalog/home/home-sections.component';
import { CONTINUE_SLOT, HERO_SLOT, homeSlots, orderedModules } from '#web/features/catalog/home/home-layout';
import { LibraryHomeRow } from '#web/features/catalog/home/library-home-row.component';
import { leadId } from '#web/features/catalog/home/home-leads';
import { useHomeLeads } from '#web/features/catalog/home/use-home-leads.hook';
import { RouteLink } from '#web/shared/ui/route-link.component';
import { isAuthed } from '#web/shared/lib/api';
import { useAuth } from '#web/shared/lib/auth';
import { useHomeLiveRefresh } from '#web/shared/lib/use-live-refresh.hook';
import { useWebRuntime } from '#web/shared/lib/web-runtime';
import { PageFrame } from '#web/shared/ui';

export const Route = createFileRoute('/_app/')({
  loader: ({ context: { runtime: webRuntime, queryClient } }) => {
    // The catalogue is auth-gated: skip until signed in (the gate covers the UI;
    // the root invalidates queries on login so these prefetch then).
    if (!isAuthed()) {
      return;
    }
    const { userQueries } = webRuntime.queries;
    void queryClient.prefetchQuery(userQueries.home());
    void queryClient.prefetchQuery(userQueries.continueWatching());
  },
  component: HomePage,
});

const homeStyles = styles({ frame: { overflowX: 'hidden' } });

function HomePage() {
  useHomeLiveRefresh();
  const s = useStyles(homeStyles);
  const { user, ready } = useAuth();
  const runtime = useWebRuntime();
  return (
    <PageFrame style={s.frame}>
      {ready && user ? <AccountHome key={JSON.stringify([runtime.client.baseUrl, user.id])} viewer={user.id} /> : <><HeroSkeleton /><SectionsSkeleton /></>}
    </PageFrame>
  );
}

function AccountHome({ viewer }: Readonly<{ viewer: string }>) {
  const t = useT();
  const runtime = useWebRuntime();
  const { userQueries } = runtime.queries;
  const layout = useQuery(userQueries.homeLayout(viewer));
  const home = useQuery(userQueries.home());
  const libraries = useQuery(runtime.queries.catalogQueries.libraries(viewer));
  const slots = orderedModules(homeSlots(home.data ?? [], libraries.isSuccess ? libraries.data : []), layout.data?.order ?? []).filter(({ id }) => !layout.data?.hidden.includes(id));
  const hero = slots.find((slot) => slot.id === HERO_SLOT)?.section;
  const spot = slots.find((slot) => slot.section?.layout === 'spotlight')?.section;
  const discover = useQuery({ ...userQueries.homeDiscovery(viewer), enabled: !!layout.data && !!(hero || spot) });
  const leads = useHomeLeads(runtime, viewer, hero, spot, discover.data ?? [], !discover.isPending);
  const onlyContinue = slots.length === 1 && slots[0]?.id === CONTINUE_SLOT;
  const resume = useQuery({ ...userQueries.continueWatching(), enabled: !!home.data && !!layout.data && onlyContinue });
  if (!layout.data) {
    return layout.isError ? (
      <EmptyState.Root icon="adjustments-horizontal">
        <EmptyState.Title>{t('account.homeLoadFailed')}</EmptyState.Title>
        <EmptyState.Actions><Button label={t('error.retry')} onPress={() => void layout.refetch()} /></EmptyState.Actions>
      </EmptyState.Root>
    ) : <><HeroSkeleton /><SectionsSkeleton /></>;
  }
  if (home.isError && !home.data) throw home.error;
  if (home.data && (!slots.length || (onlyContinue && resume.data?.length === 0))) {
    if (libraries.isPending) return <SectionsSkeleton />;
    if (libraries.isError) return <EmptyState.Root icon="library">
      <EmptyState.Title>{t('error.serverTitle')}</EmptyState.Title>
      <EmptyState.Actions><Button label={t('error.retry')} onPress={() => void libraries.refetch()} /></EmptyState.Actions>
    </EmptyState.Root>;
    return <EmptyState.Root icon="adjustments-horizontal">
      <EmptyState.Title>{t('account.homeHidden')}</EmptyState.Title>
      <EmptyState.Actions><Button label={t('account.homeModules')} asChild><RouteLink to="/account" search={{ section: 'home' }} /></Button></EmptyState.Actions>
    </EmptyState.Root>;
  }
  return <>
    {slots.map(({ id, section, library }) => {
      if (library) return <LibraryHomeRow key={id} library={library} viewer={viewer} />;
      if (id === CONTINUE_SLOT) return <ContinueRow key={id} />;
      if (id === HERO_SLOT) return (
        <Suspense key={id} fallback={<HeroSkeleton />}>
          {!home.data ? <HeroSkeleton /> : leads.hero ? (
            <HeroCarousel key={JSON.stringify(leads.hero.items.map(leadId))} section={leads.hero} onShuffle={leads.shuffle} />
          ) : <FeaturedHero />}
        </Suspense>
      );
      if (!section) return null;
      if (section.layout === 'spotlight') return leads.spotlight && !discover.isPending ? <HomeSectionView key={id} section={leads.spotlight} /> : null;
      return <HomeSectionView key={id} section={section} />;
    })}
    {!home.data && <SectionsSkeleton />}
  </>;
}

function FeaturedHero() {
  const { catalogQueries } = useWebRuntime().queries;
  const { data: featured } = useSuspenseQuery(catalogQueries.featured());
  return featured ? <Hero entry={featured} /> : <CatalogueHero />;
}

function CatalogueHero() {
  const { catalogQueries } = useWebRuntime().queries;
  const t = useT();
  const { data: movies } = useSuspenseQuery(catalogQueries.moviesView());
  const { data: shows } = useSuspenseQuery(catalogQueries.showsView());
  const first = movies[0];
  if (first) {
    return <Hero entry={{ type: 'movie', movie: first }} />;
  }
  if (shows.length > 0) {
    return null;
  }
  return (
    <EmptyState.Root icon="movie">
      <EmptyState.Title>{t('content.homeEmpty')}</EmptyState.Title>
      <EmptyState.Hint>{t('content.homeEmptyHint')}</EmptyState.Hint>
    </EmptyState.Root>
  );
}
