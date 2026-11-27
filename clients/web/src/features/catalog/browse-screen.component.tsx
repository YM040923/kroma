import { sortTitles, type Sortable } from '@kromatv/core';
import type { MessageKey } from '@kromatv/i18n';
import { useT } from '@kromatv/ui';
import { EmptyState, type IconName } from '@kromatv/ui/kit';
import { useEffect, useMemo, useState, type ReactElement } from 'react';
import { BrowseHero } from '#web/features/catalog/browse-hero.component';
import { PageFrame } from '#web/shared/ui';

export function BrowseScreen<T extends Sortable & { backdrop: string | null }>({
  heading, items, countKey, emptyIcon, emptyTitle, renderGrid,
}: Readonly<{
  heading: string;
  items: readonly T[];
  countKey: MessageKey;
  emptyIcon: IconName;
  emptyTitle: string;
  renderGrid: (view: T[]) => ReactElement;
}>) {
  const t = useT();
  const view = useMemo(() => sortTitles(items, 'added'), [items]);
  const pool = useMemo(() => items.filter((item) => item.backdrop)
    .sort((a, b) => (b.metadata?.rating ?? -1) - (a.metadata?.rating ?? -1)).slice(0, 6), [items]);
  const [slide, setSlide] = useState(0);
  useEffect(() => {
    if (pool.length < 2) return;
    const timer = setInterval(() => setSlide((n) => n + 1), 8000);
    return () => clearInterval(timer);
  }, [pool]);
  const featured = pool[slide % Math.max(pool.length, 1)];
  return (
    <PageFrame>
      <BrowseHero heading={heading} eyebrow={t('browse.library')} countText={t(countKey, { count: view.length })}
        backdrop={featured?.backdrop} creditTitle={featured?.title} />
      {view.length ? renderGrid(view) : (
        <EmptyState.Root icon={emptyIcon}><EmptyState.Title>{emptyTitle}</EmptyState.Title></EmptyState.Root>
      )}
    </PageFrame>
  );
}
