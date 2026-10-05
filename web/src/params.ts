import { defineParams } from '@sveltejs/kit/params';
import { match as localIdMatches } from './params/local_id.ts';
import { match as naddrMatches } from './params/naddr.ts';

export const params = defineParams({
  local_id: (value: string) => (localIdMatches(value) ? value : undefined),
  naddr: (value: string) => (naddrMatches(value) ? value : undefined)
});
