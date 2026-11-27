import { entryId, entryMetadata, type HomeEntry } from './entries';

export interface LeadVisit {
  hero?: string[];
  history?: string[];
  spotlight?: string;
}

export function leadId(entry: HomeEntry) {
  const tmdb = entryMetadata(entry)?.tmdbId;
  return `${entry.type}:${tmdb != null && tmdb > 0 ? `tmdb:${tmdb}` : `local:${entryId(entry)}`}`;
}

function catalogue(rows: readonly HomeEntry[]) {
  const entries = new Map<string, HomeEntry>();
  for (const item of rows) {
    const id = leadId(item);
    if (!entries.has(id)) entries.set(id, item);
  }
  return entries;
}

export function resolveLeads(seeds: readonly HomeEntry[], spotSeed: HomeEntry | undefined, pool: readonly HomeEntry[], visit: LeadVisit, previous: string | null) {
  const catalog = catalogue([...seeds, ...pool]);
  const base = [...catalogue(seeds).values()].slice(0, 5);
  const selected = visit.hero?.flatMap((id) => catalog.has(id) ? [catalog.get(id)!] : []) ?? [];
  const hero = !seeds.length ? [] : !selected.length ? base : [...catalogue([...selected, ...base, ...pool]).values()].filter((e) => leadId(e) !== visit.spotlight).slice(0, 5);
  const showing = new Set(hero.map(leadId));
  const spots = spotSeed ? [...catalogue([spotSeed, ...pool]).values()].filter((entry) => !showing.has(leadId(entry))) : [];
  const pinned = visit.spotlight && spots.find((entry) => leadId(entry) === visit.spotlight);
  const last = spots.findIndex((entry) => leadId(entry) === previous);
  const spotlight = pinned || (spots.length ? spots[(last + 1) % spots.length] : undefined);
  const eligible = [...catalog.keys()].filter((id) => !showing.has(id) && (!spotlight || leadId(spotlight) !== id));
  const history = new Map((visit.history ?? []).map((id, order) => [id, order]));
  eligible.sort((a, b) => (history.get(a) ?? -1) - (history.get(b) ?? -1));
  const next = eligible.length && seeds.length ? [...eligible, ...showing].slice(0, 5) : null;
  return { hero, spotlight, next };
}

export function shuffledVisit(visit: LeadVisit, current: readonly HomeEntry[], next: string[]): LeadVisit {
  const seen = [...(visit.history ?? []), ...current.map(leadId), ...next];
  return { ...visit, hero: next, history: [...new Set(seen.reverse())].reverse().slice(-100) };
}
