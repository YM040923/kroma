import { defineModule } from '@kromatv/module-sdk';
import { lazy } from 'react';

// The 115 Cloud module (frontend half). Its one surface is the page in the
// System group: the account, the folder mappings, the sync controls and the
// drive's own folder tree, all in one place. The module's card stays the
// generic one KROMA renders for every module, with nothing repeated on it.
const cloud115Module = defineModule({
  pages: [
    {
      path: 'cloud115',
      component: lazy(() => import('./cloud115-page.component')),
      nav: { label: 'nav.cloud115', icon: 'cloud', section: 'system', requires: 'settings.manage' },
    },
  ],
});

export default cloud115Module;
