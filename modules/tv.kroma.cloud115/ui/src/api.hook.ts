// This module's own admin API, served by the module under the mount the host
// derives from its id; `moduleApiHook` binds it, so the id is never repeated here.

import { moduleApiHook } from '@kromatv/module-sdk';
import {
  BrowseView,
  CloudAccount,
  CloudSettingsView,
  CloudStatusView,
  CookieImported,
  LoginStarted,
  LoginState,
  RapidView,
  SavedView,
  UploadFinished,
  UploadPartView,
  UploadStarted,
  StartedView,
  type CloudLibrary,
  type CloudOptions,
} from './schemas';

/** A blank/omitted `cookie` keeps the stored one; an empty field never wipes it. */
export interface Cloud115Save {
  cookie?: string;
  clearCookie?: boolean;
  baseUrl: string;
  libraries: CloudLibrary[];
  options: CloudOptions;
}

export const useCloud115Api = moduleApiHook((api) => ({
  settings: () => api.get('/settings', CloudSettingsView),
  save: (body: Cloud115Save) => api.put('/settings', body, SavedView),
  status: () => api.get('/status', CloudStatusView),
  account: () => api.get('/account', CloudAccount),
  sync: (baseUrl: string) => api.post('/sync', { baseUrl }, StartedView),
  browse: (cid: string) => api.get(`/browse?cid=${encodeURIComponent(cid)}`, BrowseView),
  browseLocal: (path: string) => api.get(`/browse-local?path=${encodeURIComponent(path)}`, BrowseView),
  loginStart: (app: string) => api.post('/login/start', { app }, LoginStarted),
  loginState: (flow: string) =>
    api.get(`/login/state?flow=${encodeURIComponent(flow)}`, LoginState),
  loginCancel: (flow: string) => api.post('/login/cancel', { flow }),
  loginCookie: (cookie: string) => api.post('/login/cookie', { cookie }, CookieImported),
  uploadRapid: (remote: string, path: string) =>
    api.post('/upload/rapid', { remote, path }, RapidView),
  uploadStart: (remote: string, path: string) =>
    api.post('/upload/start', { remote, path }, UploadStarted),
  uploadPart: (upload: string, index: number) =>
    api.post('/upload/part', { upload, index }, UploadPartView),
  uploadFinish: (upload: string) => api.post('/upload/finish', { upload }, UploadFinished),
}));
