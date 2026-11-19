import { createFileRoute } from '@tanstack/react-router';
import type { DiscoverSearch } from '#web/features/requests/discover-search';
import { validateDiscoverSearch } from '#web/features/requests/discover-search';
import { SearchPage } from '#web/features/requests/search.component';

export const Route = createFileRoute('/_app/search')({
  validateSearch: validateDiscoverSearch,
  component: () => {
    const search = Route.useSearch();
    const navigate = Route.useNavigate();
    // Every filter lives in the URL, so navigation is the single source of
    // truth and back/forward restores a filtered view.
    const onChange = (patch: Partial<DiscoverSearch>) =>
      navigate({ search: (p) => ({ ...p, ...patch }), replace: true });
    return <SearchPage search={search} onChange={onChange} />;
  },
});
