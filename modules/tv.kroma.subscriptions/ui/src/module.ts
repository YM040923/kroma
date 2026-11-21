import { defineModule } from '@kromatv/module-sdk';
import { lazy } from 'react';

// The acquisition centre is the console's one list of subscriptions; this
// module keeps the page it links to, and a nav entry would put a second
// "Subscriptions" in the sidebar beside it.
export default defineModule({
  pages: [
    {
      path: 'subscriptions',
      component: lazy(() => import('./subscriptions-page.component')),
    },
  ],
});
