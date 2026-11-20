import { createFileRoute } from '@tanstack/react-router';
import { LibraryDetailScreen } from '#web/features/admin/library-detail.component';

export const Route = createFileRoute('/admin/libraries/$id')({
  component: () => {
    const { id } = Route.useParams();
    return <LibraryDetailScreen id={id} />;
  },
});