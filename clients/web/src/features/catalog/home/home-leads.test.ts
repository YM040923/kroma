import { CardSection } from '@kromatv/client/media';
import { expect, it } from 'vitest';
import { leadId, resolveLeads, shuffledVisit } from './home-leads';

const film = (n: number) => CardSection.parse({id:'hero',title:'hero',items:[{type:'show', show:{id:`s${n}`,title:`Title ${n}`,library:'L',addedAt:'t',video:null,year:null,seasonCount:1,episodeCount:1}}]}).items[0]!;
const pool = Array.from({length:19}, (_,i)=>film(i));
const seeds = pool.slice(0,5);

it('cycles through other titles before repeating one and keeps the spotlight out of the carousel', () => {
  const first = resolveLeads(seeds,pool[5],pool,{},null);
  const next = shuffledVisit({spotlight:leadId(first.spotlight!)}, first.hero, first.next!);
  const second = resolveLeads(seeds,pool[5],pool,next,null);
  expect(second.hero.map(leadId)).toEqual(pool.slice(6,11).map(leadId));
  expect(second.next).toEqual(pool.slice(11,16).map(leadId));
  expect(second.spotlight).toBe(pool[5]);
});

it('replaces only as many as exist without duplicates and offers no fake shuffle', () => {
  const first = resolveLeads(seeds,undefined,pool.slice(0,6),{},null);
  expect(first.next).toEqual([pool[5],...seeds.slice(0,4)].map(e=>leadId(e!)));
  expect(new Set(first.next).size).toBe(5);
  expect(resolveLeads(seeds,undefined,seeds,{},null).next).toBeNull();
  expect(resolveLeads([],undefined,pool,{},null).next).toBeNull();
});

it('advances on a new document, not when the same account revisits the home or queries refresh', () => {
  const current = resolveLeads(seeds,pool[5],pool,{},null).spotlight!;
  const reloaded = resolveLeads(seeds,pool[5],pool,{},leadId(current)).spotlight!;
  expect(reloaded).toBe(pool[6]);
  expect(resolveLeads(seeds,pool[5],[...pool].reverse(),{spotlight:leadId(reloaded)},leadId(current)).spotlight).toBe(reloaded);
  expect(resolveLeads(seeds,pool[5],pool,{},leadId(reloaded)).spotlight).toBe(pool[7]);
  expect(resolveLeads(seeds,undefined,pool,{},leadId(current)).spotlight).toBeUndefined();
});
