import { describe, expect, it } from 'vitest';
import { checkEndpoint, type Endpoint } from '../../endpoints.fixture';
import { AcquisitionOutcome, type Cloud115Candidate } from './schemas';

const MOVIE = { kind: 'movie' as const, tmdbId: 603, title: 'The Matrix', year: 1999 };
const EPISODES = { seasons: null, episodes: [{ season: 1, episode: 2 }] };
const CANDIDATE: Cloud115Candidate = {
  index: 3,
  fileName: 'The.Matrix.1999.1080p.mkv',
  size: '8589934592',
  price: 5,
  resource: 'share-1',
  link: 'https://115.example/s/abc',
  shareCode: 'abc',
};

describe('the acquisition endpoints', () => {
  it.each<Endpoint>([
    {
      name: 'start',
      call: (c) => c.acquisition.start(MOVIE),
      method: 'POST',
      path: '/acquisition/start',
      body: MOVIE,
    },
    {
      name: 'start narrowed to episodes',
      call: (c) => c.acquisition.start({ ...MOVIE, kind: 'show', ...EPISODES }),
      method: 'POST',
      path: '/acquisition/start',
      body: { ...MOVIE, kind: 'show', ...EPISODES },
    },
    {
      name: 'confirm a 115 candidate',
      call: (c) => c.acquisition.confirm({ ...MOVIE, candidate: CANDIDATE }),
      method: 'POST',
      path: '/acquisition/confirm',
      body: { ...MOVIE, candidate: CANDIDATE },
    },
    {
      name: 'status',
      call: (c) => c.acquisition.status('a1'),
      method: 'GET',
      path: '/acquisition/status?attemptId=a1',
    },
  ])('$name', checkEndpoint);

  it('accepts an approved show request response from the acquisition API', () => {
    expect(
      AcquisitionOutcome.parse({
        state: 'started',
        provider: 'bt',
        requestId: 'request-1',
      }),
    ).toEqual({
      state: 'started',
      provider: 'bt',
      requestId: 'request-1',
    });
  });
});
