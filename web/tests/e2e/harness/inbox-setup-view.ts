import { mount, unmount } from 'svelte';
import Panel from './inbox-setup-panel.svelte';
import { makeSetupFixture, fixturePolicy, inbox } from './inbox-setup.ts';
import {
  makePublisherFixture,
  closePublicPool,
  getPublicPool
} from './inbox-preference-publisher.ts';
import {
  createInboxSetupView,
  closeInboxSetupView,
  inboxSetupViewSnapshot
} from '../../../src/lib/messaging/inbox-setup-view.ts';
export { inboxSetupViewSnapshot };
export { makeSetupFixture };
export async function renderFixture(
  mode: 'incomplete' | 'compatible' | 'review' | 'publish' = 'review'
) {
  const f =
    mode === 'publish'
      ? await makePublisherFixture()
      : await makeSetupFixture(
          mode === 'compatible' ? [['relay', inbox]] : undefined
        );
  const controller = createInboxSetupView({
    identity: f.identity,
    policy: 'policy' in f ? f.policy : fixturePolicy(),
    lookup: () => {
      if ('own' in f)
        return Promise.resolve(
          f.resolve(undefined, mode !== 'incomplete').resolver
        );
      return Promise.resolve(f.resolve());
    },
    now: () => 101
  });
  if (!controller) throw new Error('genuine setup controller unavailable');
  const target = document.createElement('div');
  document.body.append(target);
  const mounted = mount(Panel, { target, props: { controller } });
  return {
    controller,
    beforeSign(callback: () => Promise<void>) {
      if ('beforeSign' in f) f.beforeSign(callback);
    },
    async close() {
      closeInboxSetupView(controller);
      await unmount(mounted);
      f.close();
      if ('policy' in f) {
        const pool = getPublicPool(f.policy);
        if (pool) closePublicPool(pool);
      }
      target.remove();
    }
  };
}
