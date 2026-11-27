// This module's wire types. The shape is the one its Rust handlers answer with;
// the core does not model 115, so it lives here.

import { z } from 'zod';

export const CloudLibrary = z.object({
  remote: z.string(),
  local: z.string(),
  fastSync: z.boolean().default(true),
  localDisplay: z.string().default(''),
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
  prefetchNext: z.number().default(2),
  dailyCallBudget: z.number().default(0),
  autoSyncHours: z.number().default(6),
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
  authorized: z.boolean(),
  appId: z.string(),
  baseUrl: z.string(),
  libraries: z.array(CloudLibrary),
  options: CloudOptions,
  libraryFolders: z.array(FolderView),
  resolvedLocal: z.array(z.string()),
});
export type CloudSettingsView = z.infer<typeof CloudSettingsView>;

export const MappingProgress = z.object({
  remote: z.string(),
  localDisplay: z.string(),
  state: z.enum(['queued', 'discovering', 'processing', 'completed', 'partial', 'failed', 'paused', 'interrupted']),
  discoveredDirs: z.number(),
  totalDirs: z.number().nullable(),
  doneDirs: z.number(),
  files: z.number(),
  written: z.number(),
  errorCount: z.number(),
  errors: z.array(z.object({
    path: z.string(),
    stage: z.enum(['list', 'mkdir', 'strm', 'asset', 'metadata', 'budget', 'mapping', 'other']),
    message: z.string(),
  })),
});
export type MappingProgress = z.infer<typeof MappingProgress>;

export const SyncReport = z.object({
  mappings: z.array(MappingProgress).optional(),
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
  mappings: z.array(MappingProgress).optional(),
  at: z.number(),
  dirs: z.number(),
  files: z.number(),
  written: z.number(),
  pruned: z.number(),
  errors: z.number(),
  truncated: z.boolean(),
});
export type CloudHistoryEntry = z.infer<typeof CloudHistoryEntry>;

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
  authorized: z.boolean(),
  baseUrl: z.string(),
  watchEnabled: z.boolean(),
  watchIntervalSecs: z.number(),
  fastSync: z.object({
    lastPollAt: z.number(), lastAdded: z.number(), indexedDirs: z.number(), lastError: z.string(),
    mappings: z.array(z.object({
      remote: z.string(), state: z.string(), lastCheckedAt: z.number(), lastAdded: z.number(),
      nextOffset: z.number(), total: z.number().nullable(),
    })),
  }),
  lastPlayError: z.string(),
  lastPlayAt: z.number().default(0),
  lastPlayMode: z.string().default(''),
  lastPlayUrl: z.string().default(''),
  playCount: z.number().default(0),
  signCount: z.number().default(0),
  history: z.array(CloudHistoryEntry).default([]),
  alias: z.string().default(''),
  dailyCallBudget: z.number().default(0),
  callsToday: z.number().default(0),
  autoSyncHours: z.number().default(6),
  lastAutoSyncAt: z.number().default(0),
});
export type CloudStatusView = z.infer<typeof CloudStatusView>;

export const CloudAccount = z.object({
  total: z.number().default(0),
  used: z.number().default(0),
  remain: z.number().default(0),
  totalText: z.string().default(''),
  usedText: z.string().default(''),
  remainText: z.string().default(''),
  userName: z.string().default(''),
  userId: z.string().default(''),
  vip: z.boolean().default(false),
  avatarUrl: z.string().default(''),
  alias: z.string().default(''),
});
export type CloudAccount = z.infer<typeof CloudAccount>;

export const AliasView = z.object({ alias: z.string().default('') });
export const RemovedView = z.object({ removed: z.boolean(), filesDeleted: z.boolean() });
export const ClearedView = z.object({ cleared: z.boolean() });

export const CloudConfigExport = z.object({
  format: z.string().optional(),
  version: z.number().optional(),
  exportedAt: z.number().optional(),
  baseUrl: z.string().default(''),
  libraries: z.array(CloudLibrary).default([]),
  options: CloudOptions,
  alias: z.string().default(''),
});
export type CloudConfigExport = z.infer<typeof CloudConfigExport>;

export const LoginStarted = z.object({
  flow: z.string(),
  qr: z.string(),
  svg: z.string(),
  status: z.string(),
  appId: z.string(),
});
export type LoginStarted = z.infer<typeof LoginStarted>;

export const LoginState = z.object({
  status: z.string(),
  message: z.string(),
  authorized: z.boolean(),
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

export const SavedView = z.object({
  saved: z.boolean(),
  errors: z.array(z.string()),
  restartRequired: z.boolean().default(false),
});
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
