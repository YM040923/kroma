import { defineModule } from '@kromatv/module-sdk';
import { lazy } from 'react';
import { SubscribeAction } from './subscribe-action.component';

export default defineModule({
  pages: [{ path: 'subscriptions', component: lazy(() => import('./subscriptions-page.component')), nav: { label: 'nav.subscriptions', icon: 'repeat', section: 'system', requires: 'settings.manage' } }],
  slots: [{ slot: 'title.detail.actions', component: SubscribeAction }],
});
