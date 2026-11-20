// This module's wire types. The shape is the one its Rust handlers answer with;
// the core does not model 115, so it lives here.

import { z } from 'zod';

export const CloudLibrary = z.object({
  remote: z.string(),
  local: z.string(),
});
export type CloudLibrary = z.infer<typeof CloudLibrary>;

export const CloudOptions = z.object({
  intervalMs: z.number(),
  linkTtlSecs: z.number(),
  pageSize: z.number(),
  maxCallsPerRun: z.number(),
  maxEntriesPerFolder: z.number(),
  probeEnabled: z.boolean(),
  organizeEnabled: z.boolean(),
  writePauseMs: z.number(),
  watchEnabled: z.boolean(),
  watchIntervalSecs: z.number(),
  skipIso: z.boolean(),
  keepSuffix: z.boolean().default(false),
  overwriteStrm: z.boolean().default(false),
  minVideoSizeMb: z.number().default(0),
  syncImages: z.boolean().default(true),
  syncNfo: z.boolean().default(true),
  syncSubtitles: z.boolean().default(true),
});
export type CloudOptions = z.infer<typeof CloudOptions>;

export const FolderView = z.object({
  name: z.string(),
  kind: z.string(),
  folders: z.array(z.string()),
});
export type FolderView = z.infer<typeof FolderView>;

export const CloudSettingsView = z.object({
  hasCookie: z.boolean(),
  baseUrl: z.string(),
  libraries: z.array(CloudLibrary),
  options: CloudOptions,
  libraryFolders: z.array(FolderView),
  resolvedLocal: z.array(z.string()),
});
export type CloudSettingsView = z.infer<typeof CloudSettingsView>;

export const SyncReport = z.object({
  startedAt: z.number().nullish(),
  finishedAt: z.number().nullish(),
  current: z.string(),
  elapsedMs: z.number(),
  dirs: z.number(),
  files: z.number(),
  written: z.number(),
  kept: z.number(),
  pruned: z.number(),
  probed: z.number(),
  pendingProbe: z.number(),
  assets: z.number(),
  calls: z.number(),
  callsTotal: z.number(),
  truncated: z.boolean(),
  errors: z.array(z.string()),
});
export type SyncReport = z.infer<typeof SyncReport>;

export const CloudHistoryEntry = z.object({
  at: z.number(),
  dirs: z.number(),
  files: z.number(),
  written: z.number(),
  pruned: z.number(),
  errors: z.number(),
  truncated: z.boolean(),
});
export type CloudHistoryEntry = z.infer<typeof CloudHistoryEntry>;

export const TransferEntryView = z.object({
  at: z.number(),
  code: z.string(),
  title: z.string(),
  files: z.number(),
  ok: z.boolean(),
  message: z.string(),
});
export type TransferEntryView = z.infer<typeof TransferEntryView>;

export const ShareEntryView = z.object({
  id: z.string(),
  name: z.string(),
  isDir: z.boolean(),
  size: z.number(),
});
export type ShareEntryView = z.infer<typeof ShareEntryView>;

export const ShareView = z.object({
  code: z.string(),
  password: z.string(),
  title: z.string(),
  totalSize: z.number(),
  entries: z.array(ShareEntryView),
});
export type ShareView = z.infer<typeof ShareView>;

export const ReceiveReport = z.object({
  files: z.number(),
  message: z.string(),
});
export type ReceiveReport = z.infer<typeof ReceiveReport>;

export const ReplaceSample = z.object({
  path: z.string(),
  old: z.string(),
  new: z.string(),
});
export const ReplaceReport = z.object({
  count: z.number(),
  samples: z.array(ReplaceSample),
});
export type ReplaceReport = z.infer<typeof ReplaceReport>;

export const CloudStatusView = z.object({
  running: z.boolean(),
  report: SyncReport,
  linkCache: z.number(),
  soonestLinkSecs: z.number().nullish(),
  pausedSecs: z.number().nullish(),
  cookieSet: z.boolean(),
  baseUrl: z.string(),
  watchEnabled: z.boolean(),
  watchIntervalSecs: z.number(),
  lastPollAt: z.number(),
  lastEvents: z.number(),
  gateOk: z.boolean().nullish(),
  indexedDirs: z.number(),
  lastPlayError: z.string(),
  lastPlayAt: z.number().default(0),
  lastPlayMode: z.string().default(''),
  lastPlayUrl: z.string().default(''),
  playCount: z.number().default(0),
  signCount: z.number().default(0),
  history: z.array(CloudHistoryEntry).default([]),
  transfers: z.array(TransferEntryView).default([]),
});
export type CloudStatusView = z.infer<typeof CloudStatusView>;

export const CloudAccount = z.object({
  total: z.number().default(0),
  used: z.number().default(0),
  remain: z.number().default(0),
  totalText: z.string().default(''),
  usedText: z.string().default(''),
  remainText: z.string().default(''),
});
export type CloudAccount = z.infer<typeof CloudAccount>;

export const LoginStarted = z.object({
  flow: z.string(),
  qr: z.string(),
  svg: z.string(),
  status: z.string(),
  app: z.string(),
});
export type LoginStarted = z.infer<typeof LoginStarted>;

export const LoginState = z.object({
  status: z.string(),
  message: z.string(),
  hasCookie: z.boolean(),
});
export type LoginState = z.infer<typeof LoginState>;

export const UploadStarted = z.object({
  hit: z.boolean(),
  upload: z.string().nullish(),
  parts: z.number(),
  partSize: z.number(),
  pickcode: z.string().nullish(),
});
export type UploadStarted = z.infer<typeof UploadStarted>;

export const UploadPartView = z.object({ uploaded: z.number(), parts: z.number() });
export type UploadPartView = z.infer<typeof UploadPartView>;

export const UploadFinished = z.object({ pickcode: z.string() });
export type UploadFinished = z.infer<typeof UploadFinished>;

export const RapidView = z.object({
  hit: z.boolean(),
  needUpload: z.boolean(),
  pickcode: z.string().nullish(),
  status: z.number(),
});
export type RapidView = z.infer<typeof RapidView>;

export const CookieImported = z.object({ ok: z.boolean(), message: z.string() });
export type CookieImported = z.infer<typeof CookieImported>;

export const SavedView = z.object({ saved: z.boolean(), errors: z.array(z.string()) });
export const StartedView = z.object({ started: z.boolean() });

export const BrowseEntry = z.object({
  name: z.string(),
  isDir: z.boolean(),
  cid: z.string().nullish(),
  fid: z.string().nullish(),
  pickCode: z.string(),
  size: z.number(),
});
export type BrowseEntry = z.infer<typeof BrowseEntry>;

export const BrowseView = z.object({
  cid: z.string(),
  total: z.number(),
  entries: z.array(BrowseEntry),
});
export type BrowseView = z.infer<typeof BrowseView>;
