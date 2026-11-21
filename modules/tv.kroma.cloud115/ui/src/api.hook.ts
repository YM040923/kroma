// This module's own admin API, served by the module under the mount the host
// derives from its id; `moduleApiHook` binds it, so the id is never repeated here.

import { moduleApiHook } from '@kromatv/module-sdk';
import { z } from 'zod';
import {
  BrowseView,
  CloudAccount,
  CloudConfigExport,
  ClearedView,
  AliasView,
  RemovedView,
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
  ShareView,
  ReceiveReport,
  ReplaceReport,
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
  account: (refresh = false) =>
    api.get(`/account${refresh ? '?refresh=1' : ''}`, CloudAccount),
  accountAlias: (alias: string) => api.post('/account/alias', { alias }, AliasView),
  accountRemove: () => api.post('/account/remove', {}, RemovedView),
  watchNow: () => api.post('/watch/now', {}),
  checkpointClear: () => api.post('/checkpoint/clear', {}, ClearedView),
  configExport: () => api.get('/config/export', CloudConfigExport),
  configImport: (body: CloudConfigExport) => api.post('/config/import', body, SavedView),
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
  transferPreview: (text: string) =>
    api.post('/transfer/preview', { text }, z.array(ShareView)),
  transferReceive: (body: {
    code: string;
    password: string;
    title: string;
    cid: string;
    entries: { id: string; isDir: boolean }[];
  }) => api.post('/transfer/receive', body, ReceiveReport),
  replacePreview: () => api.post('/replace/preview', {}, ReplaceReport),
  replaceRun: () => api.post('/replace/run', {}, ReplaceReport),
}));
