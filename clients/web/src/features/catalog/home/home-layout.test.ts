import { expect, it } from 'vitest';
import type { Translate } from '@kromatv/i18n';
import { Library, CardSection } from '@kromatv/client/media';
import { homeSlots, layoutInventory, moveModule, orderedModules } from './home-layout';

const t = ((key: string) => key) as Translate;
const show = { type: 'show', show: { id: 's1', title: 'Title', library: 'l', year: null, video: null, addedAt: 't', seasonCount: 1, episodeCount: 1 } };
const rail = (id: string, layout = 'rail') => CardSection.parse({ id, title: `Name of ${id}`, layout, items: [show] });

it('keeps the actual server order by default, with the hero and continue at the front', () => {
  const rows = homeSlots([rail('hero','hero'), rail('genre:16'), rail('spotlight','spotlight')]);
  expect(orderedModules(rows, []).map(x=>x.id)).toEqual(['hero','continue','genre:16','spotlight']);
  expect(orderedModules(rows,['continue','spotlight','absent','hero']).map(x=>x.id)).toEqual(['continue','spotlight','hero','genre:16']);
});
it('keeps missing static rows configurable without inserting them on the real home', () => {
  const sections = [rail('curated:anime')];
  const inventory = layoutInventory(sections,t,false);
  expect(inventory.find(x=>x.id==='trending')?.available).toBe(false);
  expect(inventory.find(x=>x.id==='curated:anime')?.title).toBe('Name of curated:anime');
  expect(homeSlots(sections).some(x=>x.id==='trending')).toBe(false);
});
it('merges actual libraries by stable id; empty ones are editable, missing ones are not revived by saved order', () => {
  const first = Library.parse({id:'one', name:'Cinema', kind:'movies', path:'', itemCount:2});
  const empty = Library.parse({...first, id:'two', itemCount:0});
  const added = Library.parse({...first, id:'three'});
  const inventory = layoutInventory([], t, false, [first, empty]);
  const [one, two] = inventory.filter(x => x.title === 'Cinema');
  expect(one?.available).toBe(true);
  expect(two?.available).toBe(false);
  expect(two?.id).not.toBe(one?.id);
  const slots = orderedModules(homeSlots([], [{...first, name:'Renamed'}, empty, added]), [two!.id, one!.id, 'hero']);
  expect(slots.map(x => x.library?.name ?? x.id)).toEqual(['Renamed', 'hero', 'continue', 'Cinema']);
  expect(orderedModules(homeSlots([], [added]), [one!.id]).some(x => x.id === one!.id)).toBe(false);
});

it('moves exactly one position and keeps the first and last items within bounds',()=>{
  expect(moveModule(['a','b','c'],'b',-1)).toEqual(['b','a','c']);
  expect(moveModule(['a','b','c'],'c',1)).toEqual(['a','b','c']);
});
