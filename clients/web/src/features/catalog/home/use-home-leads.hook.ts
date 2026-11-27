import { useEffect, useState } from 'react';
import type { WebRuntime } from '#web/shared/lib/api';
import type { HomeEntry, HomeSection } from './entries';
import { type LeadVisit, leadId, resolveLeads, shuffledVisit } from './home-leads';

const visits = new WeakMap<WebRuntime, Map<string, LeadVisit>>();

function previousSpot(key: string) {
  try { return window.sessionStorage.getItem(key); } catch { return null; }
}

function rememberSpot(key: string, id: string) {
  try { window.sessionStorage.setItem(key, id); } catch { /* Tab storage is optional. */ }
}

export function useHomeLeads(runtime: WebRuntime, viewer: string, hero: HomeSection | undefined, spot: HomeSection | undefined, pool: readonly HomeEntry[], ready: boolean) {
  const scope = JSON.stringify([runtime.client.baseUrl, viewer]);
  const storageKey = `kroma.home.spotlight:${scope}`;
  const [visit, setVisit] = useState<LeadVisit>(() => visits.get(runtime)?.get(scope) ?? {});
  const [previous] = useState(() => previousSpot(storageKey));
  const selection = resolveLeads(hero?.items ?? [], spot?.items[0], pool, visit, previous);
  const spotId = selection.spotlight && leadId(selection.spotlight);
  function keep(next: LeadVisit) {
    let tab = visits.get(runtime);
    if (!tab) { tab = new Map(); visits.set(runtime, tab); }
    tab.set(scope, next);
    setVisit(next);
  }
  useEffect(() => {
    if (!ready || !spotId || visit.spotlight === spotId) return;
    rememberSpot(storageKey, spotId);
    let tab = visits.get(runtime);
    if (!tab) { tab = new Map(); visits.set(runtime, tab); }
    const nextVisit = { ...visit, spotlight: spotId };
    tab.set(scope, nextVisit);
    setVisit(nextVisit);
  }, [ready, runtime, scope, spotId, storageKey, visit]);
  return {
    hero: hero && { ...hero, items: selection.hero },
    spotlight: spot && selection.spotlight && { ...spot, items: [selection.spotlight] },
    shuffle: ready && selection.next ? () => {
      if (selection.next) keep(shuffledVisit({ ...visit, spotlight: spotId ?? visit.spotlight }, selection.hero, selection.next));
    } : undefined,
  };
}
