import { defineModule } from '@kromatv/module-sdk';
import { lazy } from 'react';

// The core Acquire button is the title page's one acquisition entry, so this
// module keeps its page and no longer puts a second button beside it.
export default defineModule({
  pages: [
    {
      path: 'subscriptions',
      component: lazy(() => import('./subscriptions-page.component')),
      nav: {
        label: 'nav.subscriptions',
        icon: 'repeat',
        section: 'system',
        requires: 'settings.manage',
      },
    },
  ],
});
