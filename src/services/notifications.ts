import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  isPermissionGranted,
  onAction,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { isTauriRuntime } from "./workspace";

export interface AgentSessionEnded {
  source: string;
}

export async function ensureNotificationAccess(): Promise<boolean> {
  if (!isTauriRuntime()) {
    return false;
  }
  try {
    let granted = await isPermissionGranted();
    if (!granted) {
      granted = (await requestPermission()) === "granted";
    }
    return granted;
  } catch {
    return false;
  }
}

export async function showReviewNotification(title: string, body: string): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }
  sendNotification({ title, body });
}

export async function watchNotificationActivation(onActivate: () => void): Promise<UnlistenFn> {
  if (!isTauriRuntime()) {
    return () => {};
  }
  const listener = await onAction(() => onActivate());
  return () => {
    void listener.unregister();
  };
}

export async function updateDockBadge(count: number): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }
  try {
    await invoke("update_dock_badge", { count });
  } catch {
    // The badge is a passive affordance; failures must not interrupt review.
  }
}

export async function watchAgentSessionEnded(
  handler: (event: AgentSessionEnded) => void,
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) {
    return () => {};
  }
  return listen<AgentSessionEnded>("agent-session-ended", (event) => {
    handler(event.payload);
  });
}

export async function takeAgentNotifySource(): Promise<string | null> {
  if (!isTauriRuntime()) {
    return null;
  }
  try {
    return await invoke<string | null>("take_agent_notify");
  } catch {
    return null;
  }
}
