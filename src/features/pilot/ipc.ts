// IPC wrappers for the Atlas Pilot window.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  PilotSummary,
  PilotDetail,
  HistoryEntry,
  ChatMessage,
} from "../../types/rust";

export type { PilotSummary, PilotDetail, HistoryEntry, ChatMessage };
export type {
  Epic,
  EpicTask,
  EpicStatus,
  PilotProject,
  PilotStatus,
  PilotGate,
} from "../../types/rust";

export const pilotList = () => invoke<PilotSummary[]>("pilot_list");

export const pilotGet = (path: string) =>
  invoke<PilotDetail>("pilot_get", { path });

export const pilotHistory = (path: string, number: number) =>
  invoke<HistoryEntry[]>("pilot_history", { path, number });

export const pilotTranscript = (path: string) =>
  invoke<ChatMessage[]>("pilot_transcript", { path });

export const pilotCreate = (parent: string, name: string) =>
  invoke<string>("pilot_create", { parent, name });

export const pilotArtifactRead = (path: string, kind: string) =>
  invoke<string>("pilot_artifact_read", { path, kind });

export const pilotArtifactWrite = (
  path: string,
  kind: string,
  content: string,
) => invoke<void>("pilot_artifact_write", { path, kind, content });

export const pilotApproveGate = (path: string) =>
  invoke<void>("pilot_approve_gate", { path });

export const pilotSendMessage = (path: string, text: string) =>
  invoke<void>("pilot_send_message", { path, text });

export const pilotPause = (path: string) =>
  invoke<void>("pilot_pause", { path });

export const pilotResume = (path: string) =>
  invoke<void>("pilot_resume", { path });

export const pilotInterrupt = (path: string) =>
  invoke<void>("pilot_interrupt", { path });

export const pilotStartPlanning = (path: string) =>
  invoke<void>("pilot_start_planning", { path });

export const pilotStartEpic = (path: string, number: number) =>
  invoke<void>("pilot_start_epic", { path, number });

export const pilotResumeRun = (path: string) =>
  invoke<void>("pilot_resume_run", { path });

/// Subscribe to `pilot:changed`; `cb` receives the affected project path.
export const onPilotChanged = (cb: (project: string) => void) =>
  listen<{ project: string }>("pilot:changed", (e) => cb(e.payload.project));
