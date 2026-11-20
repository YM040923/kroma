import { LibraryId, type MediaItem } from '@kromatv/client/media';
import { useT, useLocale } from '@kromatv/ui';
import { Box, Chip, EmptyState, PageHeader, Row, Text } from '@kromatv/ui/kit';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { createFileRoute } from '@tanstack/react-router';
import { type ViewStyle } from 'react-native';
import { useEffect, useMemo, useState } from 'react';
import { useAuth } from '#web/shared/lib/auth';
import { useWebRuntime } from '#web/shared/lib/web-runtime';
import { Image, PageFrame, SkeletonRow } from '#web/shared/ui';

export const Route = createFileRoute('/_app/photos')({
  validateSearch: (raw: Record<string, unknown>) => ({
    library: typeof raw.library === 'string' ? raw.library : undefined,
    month: typeof raw.month === 'string' && /^\d{4}-\d{2}$/.test(raw.month) ? raw.month : undefined,
  }),
  component: PhotosPage,
});

const masonry: React.CSSProperties = {
  columns: '4 220px',
  columnGap: 14,
};

const photoImage: ViewStyle = {
  width: '100%',
  aspectRatio: 4 / 3,
};

const tile: React.CSSProperties = {
  display: 'block',
  width: '100%',
  margin: '0 0 14px',
  breakInside: 'avoid',
  border: 0,
  borderRadius: 'var(--radius-lg)',
  overflow: 'hidden',
  padding: 0,
  background: 'var(--kroma-surface-1)',
  cursor: 'zoom-in',
  textAlign: 'left',
};

function monthOf(capturedAt: string): string {
  return capturedAt.slice(0, 7);
}

function labelMonth(month: string, locale: string): string {
  const date = new Date(`${month}-01T12:00:00`);
  return date.toLocaleDateString(locale, { year: 'numeric', month: 'long' });
}

function PhotoLightbox({ item, onClose }: Readonly<{ item: MediaItem | null; onClose: () => void }>) {
  const t = useT();
  const { client } = useAuth();
  if (!item) return null;
  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label={item.title}
      onClick={onClose}
      style={{
        position: 'fixed',
        inset: 0,
        zIndex: 100,
        display: 'grid',
        placeItems: 'center',
        padding: 24,
        background: 'rgba(0,0,0,.88)',
        cursor: 'zoom-out',
      }}
    >
      <button
        type="button"
        aria-label={t('common.close')}
        onClick={onClose}
        style={{ position: 'absolute', top: 18, right: 18, border: 0, background: 'transparent', color: '#fff', fontSize: 28 }}
      >
        ×
      </button>
      <img
        src={client.media.artwork.photoUrl(item.id)}
        alt={item.title}
        onClick={(event) => event.stopPropagation()}
        style={{ maxWidth: '100%', maxHeight: 'calc(100vh - 90px)', objectFit: 'contain', borderRadius: 8 }}
      />
      <Text variant="label" color="white" style={{ position: 'absolute', bottom: 20 }}>
        {item.title}
      </Text>
    </div>
  );
}

function PhotosPage() {
  const t = useT();
  const { client } = useAuth();
  const runtime = useWebRuntime();
  const search = Route.useSearch();
  const navigate = Route.useNavigate();
  const library = search.library ? LibraryId.parse(search.library) : undefined;
  const { data: libraries = [] } = useQuery(runtime.queries.catalogQueries.libraries());
  const allLibraryPhotos = useQuery({
    ...runtime.queries.catalogQueries.photos(library),
    placeholderData: keepPreviousData,
  });
  const libraryPhotos = useQuery({
    ...runtime.queries.catalogQueries.photos(library, search.month),
    placeholderData: keepPreviousData,
  });
  const { data: allPhotos = [] } = allLibraryPhotos;
  const { data: photos = [], isPending } = libraryPhotos;
  const [selected, setSelected] = useState<MediaItem | null>(null);
  const locale = useLocale();
  useEffect(() => {
    if (!selected) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setSelected(null);
    };
    window.addEventListener('keydown', closeOnEscape);
    return () => window.removeEventListener('keydown', closeOnEscape);
  }, [selected]);
  const months = useMemo(
    () => [...new Set(allPhotos.map((item) => monthOf(item.addedAt)))].sort().reverse(),
    [allPhotos],
  );

  if (isPending) {
    return (
      <PageFrame>
        <PageHeader.Root>
          <PageHeader.Title>{t('nav.photos')}</PageHeader.Title>
        </PageHeader.Root>
        <Box mt={24}><SkeletonRow count={8} /></Box>
      </PageFrame>
    );
  }

  return (
    <>
      <PageFrame>
        <PageHeader.Root>
          <PageHeader.Title suffix={photos?.length ? String(photos.length) : undefined}>
            {t('nav.photos')}
          </PageHeader.Title>
          <PageHeader.Subtitle>{t('browse.photosHint')}</PageHeader.Subtitle>
        </PageHeader.Root>
        <Row wrap gap={8} mt={20} mb={18}>
          <Chip
            active={!search.library}
            variant="subtle"
            icon="library"
            label={t('browse.allLibraries')}
            onPress={() => navigate({ search: (prev) => ({ ...prev, library: undefined, month: undefined })})}
          />
          {libraries.filter((lib) => lib.kind === 'photos').map((lib) => (
            <Chip
              key={lib.id}
              active={lib.id === search.library}
              variant="subtle"
              label={lib.name}
              onPress={() => navigate({ search: (prev) => ({ ...prev, library: lib.id, month: undefined })})}
            />
          ))}
        </Row>
        {months.length > 0 ? (
          <Row wrap gap={8} mb={20}>
            <Chip
              active={!search.month}
              variant="subtle"
              label={t('browse.allMonths')}
              onPress={() => navigate({ search: (prev) => ({ ...prev, month: undefined })})}
            />
            {months.map((month) => (
              <Chip
                key={month}
                active={month === search.month}
                variant="subtle"
                label={labelMonth(month, locale)}
                onPress={() => navigate({ search: (prev) => ({ ...prev, month })})}
              />
            ))}
          </Row>
        ) : null}
        {photos?.length ? (
          <div style={masonry}>
            {photos.map((item) => (
              <button key={item.id} type="button" style={tile} onClick={() => setSelected(item)}>
                <Image src={client.media.artwork.photoUrl(item.id, 520)} alt={item.title} style={photoImage} />
                <Box px={12} py={10}>
                  <Text variant="meta" numberOfLines={1}>{item.title}</Text>
                  <Text variant="meta" color="textDim" mt={2}>
                    {labelMonth(monthOf(item.addedAt), locale)}
                  </Text>
                </Box>
              </button>
            ))}
          </div>
        ) : (
          <EmptyState.Root icon="photo">
            <EmptyState.Title>{t('browse.photosEmpty')}</EmptyState.Title>
            <EmptyState.Hint>{t('browse.photosHint')}</EmptyState.Hint>
          </EmptyState.Root>
        )}
      </PageFrame>
      <PhotoLightbox item={selected} onClose={() => setSelected(null)} />
    </>
  );
}
