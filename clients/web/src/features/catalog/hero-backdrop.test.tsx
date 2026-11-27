// @vitest-environment jsdom

import { act, cleanup, render } from '@testing-library/react';
import { renderToString } from 'react-dom/server';
import { afterEach, expect, it, vi } from 'vitest';
import { HeroBackdrop } from './hero-backdrop.component';
import layout from './hero-layout.module.css';

const base = 'https://kroma.test';
const path = '/api/images/hero.webp';
const gradient = 'linear-gradient(135deg, black, gray)';
const hero = (url: string | null) => <HeroBackdrop backdrop={url} gradient={gradient} />;
const drawn = () => Array.from(document.querySelectorAll('img'), (img) => img.getAttribute('src'));
function frame(width: number, dpr: number) {
  window.__KROMA_API__ = base;
  vi.stubGlobal('devicePixelRatio', dpr);
  return vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue(new DOMRect(0, 0, width, 480));
}

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  delete window.__KROMA_API__;
});

it('renders the detail edge fade during SSR without a speculative image or preload', () => {
  const html = renderToString(hero(path));
  expect(html).toContain(layout.detailVeil);
  expect(html).not.toContain(path);
});

it.each([
  [390, 1, '?w=480'], [390, 2, '?w=780'], [640, 2, '?w=1280'],
  [1024.1, 1.25, ''], [540, 3, ''], [1350, 2, ''],
])('draws one image for %s CSS pixels at DPR %s', (width, dpr, suffix) => {
  frame(width, dpr);
  render(hero(path));
  expect(drawn()).toEqual([`${base}${path}${suffix}`]);
});

it.each([`${base}${path}?sig=a%2Fb&w=780`, `https://external.test${path}`, `${base}${path}#detail`])('preserves %s verbatim', (url) => {
  frame(400, 1);
  render(hero(url));
  expect(drawn()).toEqual([url]);
});

it('reselects for a container resize, pinch zoom and monitor change', () => {
  const bounds = frame(600, 1);
  const resolution = new EventTarget();
  vi.stubGlobal('matchMedia', () => resolution);
  const viewport = Object.assign(new EventTarget(), { scale: 1 });
  vi.stubGlobal('visualViewport', viewport);
  let resized = () => {};
  vi.stubGlobal('ResizeObserver', class {
    constructor(callback: () => void) { resized = callback; }
    observe() {}
    disconnect() {}
  });
  render(hero(`${base}${path}`));
  expect(drawn()).toContain(`${base}${path}?w=780`);
  bounds.mockReturnValue(new DOMRect(0, 0, 800, 480));
  act(resized);
  expect(drawn()).toContain(`${base}${path}?w=960`);
  viewport.scale = 1.5;
  act(() => { viewport.dispatchEvent(new Event('resize')); });
  expect(drawn()).toContain(`${base}${path}?w=1280`);
  vi.stubGlobal('devicePixelRatio', 1.25);
  act(() => { resolution.dispatchEvent(new Event('change')); });
  expect(drawn()).toContain(`${base}${path}`);
});

it('renders without a broken image when there is no backdrop', () => {
  frame(1350, 2);
  render(hero(null));
  expect(drawn()).toEqual([]);
});
