import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";

export interface DocumentEntry {
  relativePath: string;
  name: string;
}

export interface OpenedDocument {
  relativePath: string;
  content: string;
  contentHash: string;
  encoding: "utf-8" | "utf-8-bom";
}

export interface SaveResult {
  contentHash: string;
}

export interface DroppedWorkspaceSelection {
  rootPath: string;
  selectedRelativePath?: string;
}

export interface WorkspaceError {
  code: string;
  message: string;
}

export interface ChangeSetSummary {
  id: string;
  relativePath: string;
  changeType: "created" | "modified";
  baseVersionId: string;
  baseHash: string;
  candidateVersionId: string;
  candidateHash: string;
  status: "pending";
  sourceType: "external";
  source?: string;
  agent?: string;
  reason?: string;
  detectedAt: number;
  schemaVersion: number;
}

export type MarkdownBlockType =
  | "heading"
  | "paragraph"
  | "list"
  | "quote"
  | "code"
  | "table"
  | "unknown";

export interface DiffSegment {
  kind: "equal" | "added" | "deleted";
  text: string;
}

export interface StructuredChange {
  id: string;
  sequence: number;
  blockType: MarkdownBlockType;
  changeType: "added" | "deleted" | "rewritten";
  oldStart: number | null;
  oldEnd: number | null;
  newStart: number | null;
  newEnd: number | null;
  oldText: string;
  newText: string;
  oldSegments: DiffSegment[];
  newSegments: DiffSegment[];
}

export interface ChangeSetReview {
  summary: ChangeSetSummary;
  changes: StructuredChange[];
}

export type ReviewDecision = "accepted" | "rejected";

export interface ResolveResult {
  content: string;
  contentHash: string;
}

export interface DocumentVersionSummary {
  id: string;
  relativePath: string;
  contentHash: string;
  encoding: "utf-8" | "utf-8-bom";
  versionType:
    | "snapshot"
    | "pre_edit"
    | "editor"
    | "external"
    | "review_accepted"
    | "review_rejected"
    | "review_mixed"
    | "review_discarded"
    | "pre_restore"
    | "restore";
  createdAt: number;
  sourceType: "filesystem" | "editor" | "external" | "review" | "history";
  source?: string;
  agent?: string;
  reason?: string;
  schemaVersion: number;
}

export interface DocumentVersion extends DocumentVersionSummary {
  content: string;
  metadataJson: string;
}

export interface RestoreResult {
  content: string;
  contentHash: string;
  encoding: "utf-8" | "utf-8-bom";
  version: DocumentVersionSummary;
}

export type StopWorkspaceWatch = () => Promise<void>;
export type StopWorkspaceDrop = UnlistenFn;

const browserDocuments: Record<string, string> = {
  "README.md": `# MarginPost

Open a local folder, choose a Markdown file, and keep control of every save.

## AMR-003

- Local workspace
- External change monitoring
- File-level review inbox
`,
  "notes/product-principles.markdown": `# Product principles

The editor stays calm while the work remains understandable and reversible.
`,
};
let browserChanges: ChangeSetSummary[] = [];
let browserReviews: Record<string, ChangeSetReview> = {};
let browserSnapshots: Record<string, { base: string; candidate: string }> = {};
let browserVersions: Record<string, DocumentVersion[]> = {};
let browserVersionSequence = 0;

function browserHash(relativePath: string, content: string) {
  return `preview-${relativePath}-${content.length}-${content.charCodeAt(0) || 0}`;
}

function recordBrowserVersion(
  relativePath: string,
  content: string,
  versionType: DocumentVersionSummary["versionType"],
  sourceType: DocumentVersionSummary["sourceType"],
  deduplicate = false,
  encoding: DocumentVersionSummary["encoding"] = "utf-8",
  metadataJson = "{}",
) {
  const versions = browserVersions[relativePath] ?? [];
  const contentHash = browserHash(relativePath, content);
  if (deduplicate) {
    const existing = versions.find((version) => version.contentHash === contentHash);
    if (existing) return existing;
  }
  browserVersionSequence += 1;
  const version: DocumentVersion = {
    id: `preview-version-${Date.now()}-${browserVersionSequence}`,
    relativePath,
    content,
    contentHash,
    encoding,
    versionType,
    createdAt: Date.now() + browserVersionSequence,
    sourceType,
    schemaVersion: 2,
    metadataJson,
  };
  browserVersions[relativePath] = [version, ...versions];
  return version;
}

export function isTauriRuntime() {
  return "__TAURI_INTERNALS__" in window;
}

export async function inspectDroppedPath(
  droppedPath: string,
): Promise<DroppedWorkspaceSelection> {
  if (!isTauriRuntime()) {
    throw {
      code: "UNSUPPORTED_DROP",
      message: "Desktop file dropping is unavailable in browser preview.",
    };
  }
  return invoke<DroppedWorkspaceSelection>("inspect_dropped_path", {
    droppedPath,
  });
}

export async function startWorkspaceDrop(
  onDrop: (paths: string[]) => void,
  onDragState: (active: boolean) => void,
): Promise<StopWorkspaceDrop> {
  if (!isTauriRuntime()) {
    return async () => {};
  }

  return getCurrentWebview().onDragDropEvent((event) => {
    if (event.payload.type === "enter" || event.payload.type === "over") {
      onDragState(true);
      return;
    }
    onDragState(false);
    if (event.payload.type === "drop") {
      onDrop(event.payload.paths);
    }
  });
}

export function normalizeWorkspaceError(error: unknown): WorkspaceError {
  if (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    "message" in error
  ) {
    return {
      code: String(error.code),
      message: String(error.message),
    };
  }

  return {
    code: "UNKNOWN_ERROR",
    message: error instanceof Error ? error.message : String(error),
  };
}

export async function chooseWorkspace(dialogTitle = "Open Markdown Workspace"): Promise<string | null> {
  if (!isTauriRuntime()) {
    return "/Browser Preview/agent-markdown-reviewer";
  }

  const selection = await open({
    directory: true,
    multiple: false,
    title: dialogTitle,
  });

  return typeof selection === "string" ? selection : null;
}

export async function listMarkdownFiles(rootPath: string): Promise<DocumentEntry[]> {
  if (!isTauriRuntime()) {
    return Object.keys(browserDocuments)
      .sort()
      .map((relativePath) => ({
        relativePath,
        name: relativePath.split("/").at(-1) ?? relativePath,
      }));
  }

  return invoke<DocumentEntry[]>("list_markdown_files", { rootPath });
}

export async function readMarkdownFile(
  rootPath: string,
  relativePath: string,
): Promise<OpenedDocument> {
  if (!isTauriRuntime()) {
    const content = browserDocuments[relativePath];
    if (content === undefined) {
      throw { code: "FILE_UNAVAILABLE", message: "The preview file is unavailable." };
    }
    return {
      relativePath,
      content,
      contentHash: recordBrowserVersion(
        relativePath,
        content,
        "snapshot",
        "filesystem",
        true,
      ).contentHash,
      encoding: "utf-8",
    };
  }

  return invoke<OpenedDocument>("read_markdown_file", {
    rootPath,
    relativePath,
  });
}

export async function saveMarkdownFile(
  rootPath: string,
  document: OpenedDocument,
  content: string,
): Promise<SaveResult> {
  if (!isTauriRuntime()) {
    recordBrowserVersion(
      document.relativePath,
      browserDocuments[document.relativePath] ?? document.content,
      "pre_edit",
      "editor",
      true,
      document.encoding,
    );
    browserDocuments[document.relativePath] = content;
    const version = recordBrowserVersion(
      document.relativePath,
      content,
      "editor",
      "editor",
      false,
      document.encoding,
    );
    return { contentHash: version.contentHash };
  }

  return invoke<SaveResult>("save_markdown_file", {
    rootPath,
    relativePath: document.relativePath,
    content,
    expectedHash: document.contentHash,
    encoding: document.encoding,
  });
}

export async function listChangeSets(): Promise<ChangeSetSummary[]> {
  if (!isTauriRuntime()) {
    return [...browserChanges];
  }
  return invoke<ChangeSetSummary[]>("list_change_sets");
}

export async function getChangeSetReview(id: string): Promise<ChangeSetReview | null> {
  if (!isTauriRuntime()) {
    return browserReviews[id] ?? null;
  }
  return invoke<ChangeSetReview | null>("get_change_set_review", { id });
}

export async function resolveChangeSet(
  id: string,
  decisions: Array<{ changeId: string; decision: ReviewDecision }>,
): Promise<ResolveResult> {
  if (!isTauriRuntime()) {
    const review = browserReviews[id];
    const snapshot = browserSnapshots[id];
    if (!review || !snapshot) {
      throw { code: "CHANGE_SET_UNAVAILABLE", message: "Change set unavailable." };
    }
    let content = snapshot.base;
    for (const decision of decisions) {
      if (decision.decision !== "accepted") continue;
      const change = review.changes.find((item) => item.id === decision.changeId);
      if (!change) continue;
      if (change.oldText) {
        content = content.replace(change.oldText, change.newText);
      } else {
        content = `${content.trimEnd()}\n\n${change.newText}\n`;
      }
    }
    recordBrowserVersion(
      review.summary.relativePath,
      snapshot.candidate,
      "external",
      "external",
      true,
    );
    browserDocuments[review.summary.relativePath] = content;
    const acceptedCount = decisions.filter(
      (decision) => decision.decision === "accepted",
    ).length;
    const versionType =
      acceptedCount === decisions.length
        ? "review_accepted"
        : acceptedCount === 0
          ? "review_rejected"
          : "review_mixed";
    const version = recordBrowserVersion(
      review.summary.relativePath,
      content,
      versionType,
      "review",
      false,
      "utf-8",
      JSON.stringify({ changeSetId: id }),
    );
    browserChanges = browserChanges.filter((changeSet) => changeSet.id !== id);
    delete browserReviews[id];
    delete browserSnapshots[id];
    return {
      content,
      contentHash: version.contentHash,
    };
  }
  return invoke<ResolveResult>("resolve_change_set", { id, decisions });
}

export async function listDocumentVersions(
  rootPath: string,
  relativePath: string,
): Promise<DocumentVersionSummary[]> {
  if (!isTauriRuntime()) {
    return (browserVersions[relativePath] ?? [])
      .filter((version) => version.versionType !== "review_discarded")
      .map(({ content: _content, metadataJson: _metadata, ...summary }) => summary);
  }
  const versions = await invoke<DocumentVersionSummary[]>("list_document_versions", {
    rootPath,
    relativePath,
  });
  return versions.filter((version) => version.versionType !== "review_discarded");
}

export async function discardChangeSet(id: string): Promise<void> {
  if (!isTauriRuntime()) {
    browserChanges = browserChanges.filter((change) => change.id !== id);
    delete browserReviews[id];
    delete browserSnapshots[id];
    return;
  }
  await invoke("discard_change_set", { id });
}

export async function getDocumentVersion(
  rootPath: string,
  relativePath: string,
  versionId: string,
): Promise<DocumentVersion | null> {
  if (!isTauriRuntime()) {
    return (
      browserVersions[relativePath]?.find((version) => version.id === versionId) ??
      null
    );
  }
  return invoke<DocumentVersion | null>("get_document_version", {
    rootPath,
    relativePath,
    versionId,
  });
}

export async function restoreDocumentVersion(
  rootPath: string,
  relativePath: string,
  versionId: string,
): Promise<RestoreResult> {
  if (!isTauriRuntime()) {
    const target = browserVersions[relativePath]?.find(
      (version) => version.id === versionId,
    );
    if (!target) {
      throw { code: "VERSION_UNAVAILABLE", message: "Version unavailable." };
    }
    const current = browserDocuments[relativePath];
    if (current === undefined) {
      throw { code: "FILE_UNAVAILABLE", message: "File unavailable." };
    }
    recordBrowserVersion(
      relativePath,
      current,
      "pre_restore",
      "history",
      true,
    );
    browserDocuments[relativePath] = target.content;
    const restored = recordBrowserVersion(
      relativePath,
      target.content,
      "restore",
      "history",
      false,
      target.encoding,
      JSON.stringify({ targetVersionId: target.id }),
    );
    return {
      content: restored.content,
      contentHash: restored.contentHash,
      encoding: restored.encoding,
      version: restored,
    };
  }
  return invoke<RestoreResult>("restore_document_version", {
    rootPath,
    relativePath,
    versionId,
  });
}

export async function startWorkspaceWatch(
  rootPath: string,
  onChangeSet: (changeSet: ChangeSetSummary) => void,
): Promise<StopWorkspaceWatch> {
  if (!isTauriRuntime()) {
    browserChanges = [];
    browserReviews = {};
    browserSnapshots = {};
    const timer = window.setTimeout(() => {
      const baseContent = browserDocuments["README.md"] ?? "";
      const candidateContent = `${baseContent.replace("## AMR-003", "## AMR-005").trimEnd()}

External tools can now place file-level changes into the review inbox.
`;
      const changeSet: ChangeSetSummary = {
        id: `preview-change-${Date.now()}`,
        relativePath: "README.md",
        changeType: "modified",
        baseVersionId: `preview-version-${baseContent.length}`,
        baseHash: `preview-base-${baseContent.length}`,
        candidateVersionId: `preview-version-${candidateContent.length}`,
        candidateHash: `preview-candidate-${candidateContent.length}`,
        status: "pending",
        sourceType: "external",
        detectedAt: Date.now(),
        schemaVersion: 1,
      };
      browserChanges = [changeSet];
      browserSnapshots[changeSet.id] = {
        base: baseContent,
        candidate: candidateContent,
      };
      browserReviews[changeSet.id] = {
        summary: changeSet,
        changes: [
          {
            id: "change-1",
            sequence: 0,
            blockType: "heading",
            changeType: "rewritten",
            oldStart: 0,
            oldEnd: 0,
            newStart: 0,
            newEnd: 0,
            oldText: "## AMR-003",
            newText: "## AMR-005",
            oldSegments: [{ kind: "deleted", text: "## AMR-003" }],
            newSegments: [{ kind: "added", text: "## AMR-005" }],
          },
          {
            id: "change-2",
            sequence: 1,
            blockType: "paragraph",
            changeType: "added",
            oldStart: null,
            oldEnd: null,
            newStart: 0,
            newEnd: 0,
            oldText: "",
            newText:
              "External tools can now place file-level changes into the review inbox.",
            oldSegments: [],
            newSegments: [
              {
                kind: "added",
                text:
                  "External tools can now place file-level changes into the review inbox.",
              },
            ],
          },
        ],
      };
      onChangeSet(changeSet);
    }, 700);

    return async () => {
      window.clearTimeout(timer);
      browserChanges = [];
      browserReviews = {};
      browserSnapshots = {};
    };
  }

  let unlisten: UnlistenFn | null = await listen<ChangeSetSummary>(
    "change-set-created",
    (event) => onChangeSet(event.payload),
  );
  try {
    await invoke("start_workspace_watch", { rootPath });
  } catch (error) {
    unlisten();
    unlisten = null;
    throw error;
  }

  return async () => {
    unlisten?.();
    unlisten = null;
    await invoke("stop_workspace_watch");
  };
}
