import { defineModule } from '@kromatv/module-sdk';
import { lazy } from 'react';

export default defineModule({
  pages: [{ path: 'subscriptions', component: lazy(() => import('./subscriptions-page.component')), nav: { label: 'nav.subscriptions', icon: 'repeat', section: 'system', requires: 'settings.manage' } }],
});
