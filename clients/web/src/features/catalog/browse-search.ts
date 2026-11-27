// Old bookmarks still open their original library or genre instead of silently
// widening a scoped view to the entire catalogue. Sort alone is retired.
export function browseDestination(search: Record<string, unknown>) {
  if (typeof search.library === 'string' && search.library) {
    return { to: '/libraries/$id' as const, params: { id: search.library } };
  }
  if (typeof search.genre === 'string' && search.genre) {
    return { to: '/genres/$id' as const, params: { id: search.genre } };
  }
  return undefined;
}
