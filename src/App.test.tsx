import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import {
  ensureNotificationAccess,
  showReviewNotification,
  takeAgentNotifySource,
  updateDockBadge,
  watchAgentSessionEnded,
  watchNotificationActivation,
} from "./services/notifications";
import {
  chooseWorkspace,
  discardChangeSet,
  getDocumentVersion,
  getChangeSetReview,
  inspectDroppedPath,
  listDocumentVersions,
  listChangeSets,
  listMarkdownFiles,
  readMarkdownFile,
  resolveChangeSet,
  restoreDocumentVersion,
  saveMarkdownFile,
  startWorkspaceDrop,
  startWorkspaceWatch,
  type ChangeSetSummary,
} from "./services/workspace";

vi.mock("./editor/MarkdownEditor", () => ({
  MarkdownEditor: ({
    value,
    onChange,
  }: {
    value: string;
    onChange: (value: string) => void;
  }) => (
    <textarea
      aria-label="Markdown editor"
      onChange={(event) => onChange(event.target.value)}
      value={value}
    />
  ),
}));

vi.mock("./services/workspace", async () => {
  const actual =
    await vi.importActual<typeof import("./services/workspace")>("./services/workspace");
  return {
    ...actual,
    chooseWorkspace: vi.fn(),
    discardChangeSet: vi.fn(),
    getDocumentVersion: vi.fn(),
    getChangeSetReview: vi.fn(),
    inspectDroppedPath: vi.fn(),
    listDocumentVersions: vi.fn(),
    listMarkdownFiles: vi.fn(),
    listChangeSets: vi.fn(),
    readMarkdownFile: vi.fn(),
    resolveChangeSet: vi.fn(),
    restoreDocumentVersion: vi.fn(),
    saveMarkdownFile: vi.fn(),
    startWorkspaceDrop: vi.fn(),
    startWorkspaceWatch: vi.fn(),
  };
});

vi.mock("./services/notifications", async () => {
  const actual =
    await vi.importActual<typeof import("./services/notifications")>(
      "./services/notifications",
    );
  return {
    ...actual,
    ensureNotificationAccess: vi.fn(async () => true),
    showReviewNotification: vi.fn(async () => {}),
    takeAgentNotifySource: vi.fn(async () => null),
    updateDockBadge: vi.fn(async () => {}),
    watchAgentSessionEnded: vi.fn(async () => () => {}),
    watchNotificationActivation: vi.fn(async () => () => {}),
  };
});

const mockedChooseWorkspace = vi.mocked(chooseWorkspace);
const mockedDiscardChangeSet = vi.mocked(discardChangeSet);
const mockedGetDocumentVersion = vi.mocked(getDocumentVersion);
const mockedGetChangeSetReview = vi.mocked(getChangeSetReview);
const mockedInspectDroppedPath = vi.mocked(inspectDroppedPath);
const mockedListDocumentVersions = vi.mocked(listDocumentVersions);
const mockedListMarkdownFiles = vi.mocked(listMarkdownFiles);
const mockedListChangeSets = vi.mocked(listChangeSets);
const mockedReadMarkdownFile = vi.mocked(readMarkdownFile);
const mockedResolveChangeSet = vi.mocked(resolveChangeSet);
const mockedRestoreDocumentVersion = vi.mocked(restoreDocumentVersion);
const mockedSaveMarkdownFile = vi.mocked(saveMarkdownFile);
const mockedStartWorkspaceDrop = vi.mocked(startWorkspaceDrop);
const mockedStartWorkspaceWatch = vi.mocked(startWorkspaceWatch);
const mockedShowReviewNotification = vi.mocked(showReviewNotification);
const mockedTakeAgentNotifySource = vi.mocked(takeAgentNotifySource);
const mockedUpdateDockBadge = vi.mocked(updateDockBadge);
const mockedWatchAgentSessionEnded = vi.mocked(watchAgentSessionEnded);
const mockedWatchNotificationActivation = vi.mocked(watchNotificationActivation);
const mockedEnsureNotificationAccess = vi.mocked(ensureNotificationAccess);
let emitChangeSet: ((changeSet: ChangeSetSummary) => void) | null = null;
let emitDrop: ((paths: string[]) => void) | null = null;
let emitDragState: ((active: boolean) => void) | null = null;
let emitAgentSession: ((event: { source: string }) => void) | null = null;

function externalChangeSet(id: string): ChangeSetSummary {
  return {
    id,
    relativePath: "notes/product.md",
    changeType: "modified",
    baseVersionId: "version-hash-1",
    baseHash: "hash-1",
    candidateVersionId: "version-hash-2",
    candidateHash: "hash-2",
    status: "pending",
    sourceType: "external",
    detectedAt: 1,
    schemaVersion: 1,
  };
}

describe("App", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.localStorage.clear();
    emitChangeSet = null;
    emitDrop = null;
    emitDragState = null;
    emitAgentSession = null;
    mockedEnsureNotificationAccess.mockResolvedValue(true);
    mockedShowReviewNotification.mockResolvedValue(undefined);
    mockedTakeAgentNotifySource.mockResolvedValue(null);
    mockedUpdateDockBadge.mockResolvedValue(undefined);
    mockedWatchAgentSessionEnded.mockImplementation(async (handler) => {
      emitAgentSession = handler;
      return () => {};
    });
    mockedWatchNotificationActivation.mockResolvedValue(() => {});
    mockedDiscardChangeSet.mockResolvedValue(undefined);
    mockedChooseWorkspace.mockResolvedValue("/workspace");
    mockedListMarkdownFiles.mockResolvedValue([
      { relativePath: "notes/product.md", name: "product.md" },
    ]);
    mockedListChangeSets.mockResolvedValue([]);
    mockedListDocumentVersions.mockResolvedValue([
      {
        id: "version-current",
        relativePath: "notes/product.md",
        contentHash: "hash-1",
        encoding: "utf-8",
        versionType: "snapshot",
        createdAt: 2,
        sourceType: "filesystem",
        schemaVersion: 2,
      },
      {
        id: "version-old",
        relativePath: "notes/product.md",
        contentHash: "hash-old",
        encoding: "utf-8",
        versionType: "editor",
        createdAt: 1,
        sourceType: "editor",
        schemaVersion: 2,
      },
    ]);
    mockedGetDocumentVersion.mockImplementation(async (_root, _path, versionId) => ({
      id: versionId,
      relativePath: "notes/product.md",
      content: versionId === "version-old" ? "# Older product" : "# Product",
      contentHash: versionId === "version-old" ? "hash-old" : "hash-1",
      encoding: "utf-8",
      versionType: versionId === "version-old" ? "editor" : "snapshot",
      createdAt: versionId === "version-old" ? 1 : 2,
      sourceType: versionId === "version-old" ? "editor" : "filesystem",
      schemaVersion: 2,
      metadataJson: "{}",
    }));
    mockedGetChangeSetReview.mockResolvedValue({
      summary: {
        id: "cs-1",
        relativePath: "notes/product.md",
        changeType: "modified",
        baseVersionId: "version-hash-1",
        baseHash: "hash-1",
        candidateVersionId: "version-hash-2",
        candidateHash: "hash-2",
        status: "pending",
        sourceType: "external",
        detectedAt: 1,
        schemaVersion: 1,
      },
      changes: [
        {
          id: "change-1",
          sequence: 0,
          blockType: "paragraph",
          changeType: "rewritten",
          oldStart: 0,
          oldEnd: 20,
          newStart: 0,
          newEnd: 20,
          oldText: "第一句不变。第二句需要修改。",
          newText: "第一句不变。第二句已经修改。",
          oldSegments: [
            { kind: "equal", text: "第一句不变。" },
            { kind: "deleted", text: "第二句需要修改。" },
          ],
          newSegments: [
            { kind: "equal", text: "第一句不变。" },
            { kind: "added", text: "第二句已经修改。" },
          ],
        },
      ],
    });
    mockedReadMarkdownFile.mockResolvedValue({
      relativePath: "notes/product.md",
      content: "# Product",
      contentHash: "hash-1",
      encoding: "utf-8",
    });
    mockedSaveMarkdownFile.mockResolvedValue({ contentHash: "hash-2" });
    mockedInspectDroppedPath.mockResolvedValue({
      rootPath: "/dropped-workspace",
    });
    mockedResolveChangeSet.mockResolvedValue({
      content: "第一句不变。第二句已经修改。",
      contentHash: "hash-resolved",
    });
    mockedRestoreDocumentVersion.mockResolvedValue({
      content: "# Older product",
      contentHash: "hash-restored",
      encoding: "utf-8",
      version: {
        id: "version-restored",
        relativePath: "notes/product.md",
        contentHash: "hash-restored",
        encoding: "utf-8",
        versionType: "restore",
        createdAt: 3,
        sourceType: "history",
        schemaVersion: 2,
      },
    });
    mockedStartWorkspaceWatch.mockImplementation(async (_rootPath, onChangeSet) => {
      emitChangeSet = onChangeSet;
      return async () => {};
    });
    mockedStartWorkspaceDrop.mockImplementation(async (onDrop, onDragState) => {
      emitDrop = onDrop;
      emitDragState = onDragState;
      return async () => {};
    });
  });

  it("exposes the three product views", () => {
    render(<App />);

    expect(screen.getByRole("button", { name: "Editor" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Changes" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "History" })).toBeInTheDocument();
  });

  it("opens a workspace, edits a document, and saves it", async () => {
    render(<App />);

    fireEvent.click(
      screen.getByRole("button", { name: "Open Markdown Workspace" }),
    );
    expect(await screen.findByRole("button", { name: /product\.md/i })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /product\.md/i }));
    const editor = await screen.findByRole("textbox", { name: "Markdown editor" });
    fireEvent.change(editor, { target: { value: "# Updated product" } });
    fireEvent.click(screen.getByRole("button", { name: /save/i }));

    await waitFor(() => {
      expect(mockedSaveMarkdownFile).toHaveBeenCalledWith(
        "/workspace",
        expect.objectContaining({ contentHash: "hash-1" }),
        "# Updated product",
      );
    });
    expect(await screen.findByText("Saved to disk.")).toBeInTheDocument();
  });

  it("opens a dropped folder as the workspace", async () => {
    render(<App />);
    await waitFor(() => expect(mockedStartWorkspaceDrop).toHaveBeenCalled());

    emitDrop?.(["/dropped-workspace"]);

    await waitFor(() => {
      expect(mockedInspectDroppedPath).toHaveBeenCalledWith(
        "/dropped-workspace",
      );
      expect(mockedListMarkdownFiles).toHaveBeenCalledWith(
        "/dropped-workspace",
      );
    });
    expect(
      await screen.findByText(
        "Workspace opened from drop. 1 Markdown file(s) found.",
      ),
    ).toBeInTheDocument();
  });

  it("opens a dropped Markdown file from its parent workspace", async () => {
    mockedInspectDroppedPath.mockResolvedValueOnce({
      rootPath: "/dropped-workspace",
      selectedRelativePath: "notes/product.md",
    });
    render(<App />);
    await waitFor(() => expect(mockedStartWorkspaceDrop).toHaveBeenCalled());

    emitDrop?.(["/dropped-workspace/notes/product.md"]);

    await waitFor(() => {
      expect(mockedReadMarkdownFile).toHaveBeenCalledWith(
        "/dropped-workspace",
        "notes/product.md",
      );
    });
    expect(
      await screen.findByText("notes/product.md opened from drop."),
    ).toBeInTheDocument();
    expect(
      await screen.findByRole("textbox", { name: "Markdown editor" }),
    ).toHaveValue("# Product");
  });

  it("shows the desktop drop target while an item is hovering", async () => {
    render(<App />);
    await waitFor(() => expect(mockedStartWorkspaceDrop).toHaveBeenCalled());

    emitDragState?.(true);
    expect(
      await screen.findByText("Drop a folder or one Markdown file"),
    ).toBeInTheDocument();

    emitDragState?.(false);
    await waitFor(() => {
      expect(
        screen.queryByText("Drop a folder or one Markdown file"),
      ).not.toBeInTheDocument();
    });
  });

  it("rejects multiple dropped items without changing the workspace", async () => {
    render(<App />);
    await waitFor(() => expect(mockedStartWorkspaceDrop).toHaveBeenCalled());

    emitDrop?.(["/one.md", "/two.md"]);

    expect(
      await screen.findByText(
        "Drop only one folder or Markdown file at a time.",
      ),
    ).toBeInTheDocument();
    expect(mockedInspectDroppedPath).not.toHaveBeenCalled();
  });

  it("shows a localized error for a dropped unsupported file", async () => {
    mockedInspectDroppedPath.mockRejectedValueOnce({
      code: "UNSUPPORTED_DROP",
      message: "unsupported",
    });
    render(<App />);
    await waitFor(() => expect(mockedStartWorkspaceDrop).toHaveBeenCalled());

    emitDrop?.(["/dropped-workspace/note.txt"]);

    expect(
      await screen.findByText(
        "Only folders and .md or .markdown files can be dropped here.",
      ),
    ).toBeInTheDocument();
  });

  it("protects an unsaved draft from a dropped workspace", async () => {
    render(<App />);
    fireEvent.click(
      screen.getByRole("button", { name: "Open Markdown Workspace" }),
    );
    fireEvent.click(await screen.findByRole("button", { name: /product\.md/i }));
    fireEvent.change(
      await screen.findByRole("textbox", { name: "Markdown editor" }),
      { target: { value: "# Unsaved before drop" } },
    );

    emitDrop?.(["/dropped-workspace"]);

    expect(
      await screen.findByText(
        "Save the current draft before changing workspaces.",
      ),
    ).toBeInTheDocument();
    expect(mockedInspectDroppedPath).not.toHaveBeenCalled();
  });

  it("keeps the draft when saving fails", async () => {
    mockedSaveMarkdownFile.mockRejectedValue({
      code: "FILE_CHANGED",
      message: "The file changed on disk. Your draft was kept.",
    });
    render(<App />);

    fireEvent.click(
      screen.getByRole("button", { name: "Open Markdown Workspace" }),
    );
    fireEvent.click(await screen.findByRole("button", { name: /product\.md/i }));
    const editor = await screen.findByRole("textbox", { name: "Markdown editor" });
    fireEvent.change(editor, { target: { value: "# Unsaved draft" } });
    fireEvent.click(screen.getByRole("button", { name: /save/i }));

    expect(await screen.findByText(/draft was kept/i)).toBeInTheDocument();
    expect(editor).toHaveValue("# Unsaved draft");
  });

  it("places an external change in the Changes inbox", async () => {
    render(<App />);
    fireEvent.click(
      screen.getByRole("button", { name: "Open Markdown Workspace" }),
    );

    await waitFor(() => expect(mockedStartWorkspaceWatch).toHaveBeenCalled());
    emitChangeSet?.({
      id: "cs-1",
      relativePath: "notes/product.md",
      changeType: "modified",
      baseVersionId: "version-hash-1",
      baseHash: "hash-1",
      candidateVersionId: "version-hash-2",
      candidateHash: "hash-2",
      status: "pending",
      sourceType: "external",
      detectedAt: Date.now(),
      schemaVersion: 1,
    });

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Changes" })).toHaveTextContent("1");
    });
    fireEvent.click(screen.getByRole("button", { name: "Changes" }));
    expect(screen.getByText("notes/product.md")).toBeInTheDocument();
    expect(screen.getByText("External modification")).toBeInTheDocument();
  });

  it("discards an inapplicable change set without touching the file", async () => {
    render(<App />);
    fireEvent.click(
      screen.getByRole("button", { name: "Open Markdown Workspace" }),
    );

    await waitFor(() => expect(mockedStartWorkspaceWatch).toHaveBeenCalled());
    emitChangeSet?.(externalChangeSet("cs-1"));
    fireEvent.click(screen.getByRole("button", { name: "Changes" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "Review notes/product.md" }),
    );

    fireEvent.click(
      await screen.findByRole("button", { name: "Discard" }),
    );

    await waitFor(() =>
      expect(mockedDiscardChangeSet).toHaveBeenCalledWith("cs-1"),
    );
    expect(
      await screen.findByText("Discarded. The file on disk was not modified."),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Review notes/product.md" }),
    ).not.toBeInTheDocument();
  });

  it("shows the self-reported source on attributed change sets", async () => {
    render(<App />);
    fireEvent.click(
      screen.getByRole("button", { name: "Open Markdown Workspace" }),
    );

    await waitFor(() => expect(mockedStartWorkspaceWatch).toHaveBeenCalled());
    emitChangeSet?.({ ...externalChangeSet("cs-1"), source: "Claude Code" });

    fireEvent.click(screen.getByRole("button", { name: "Changes" }));
    await waitFor(() =>
      expect(
        screen.getByText("External modification · Claude Code"),
      ).toBeInTheDocument(),
    );
  });

  it("shows the source on attributed history versions", async () => {
    mockedListDocumentVersions.mockResolvedValue([
      {
        id: "version-external",
        relativePath: "notes/product.md",
        contentHash: "hash-1",
        encoding: "utf-8",
        versionType: "external",
        createdAt: 3,
        sourceType: "external",
        source: "Claude Code",
        schemaVersion: 2,
      },
    ]);
    render(<App />);
    fireEvent.click(
      screen.getByRole("button", { name: "Open Markdown Workspace" }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: /product\.md/i }),
    );
    await screen.findByRole("textbox", { name: "Markdown editor" });
    await waitFor(() => {
      expect(screen.queryByText("Opening")).not.toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: "History" }));

    expect(await screen.findByText(/· Claude Code/)).toBeInTheDocument();
  });

  it("keeps the dock badge in sync with pending change sets", async () => {
    render(<App />);
    fireEvent.click(
      screen.getByRole("button", { name: "Open Markdown Workspace" }),
    );

    await waitFor(() => expect(mockedStartWorkspaceWatch).toHaveBeenCalled());
    expect(mockedUpdateDockBadge).toHaveBeenLastCalledWith(0);

    emitChangeSet?.(externalChangeSet("cs-1"));
    await waitFor(() =>
      expect(mockedUpdateDockBadge).toHaveBeenLastCalledWith(1),
    );
  });

  it("sends one coalesced notification for external changes while unfocused", async () => {
    const hasFocus = vi.spyOn(document, "hasFocus").mockReturnValue(false);
    render(<App />);
    fireEvent.click(
      screen.getByRole("button", { name: "Open Markdown Workspace" }),
    );

    await waitFor(() => expect(mockedStartWorkspaceWatch).toHaveBeenCalled());
    vi.useFakeTimers();
    act(() => {
      emitChangeSet?.(externalChangeSet("cs-1"));
    });
    act(() => {
      emitChangeSet?.(externalChangeSet("cs-2"));
    });
    act(() => {
      vi.advanceTimersByTime(3000);
    });
    vi.useRealTimers();

    expect(mockedShowReviewNotification).toHaveBeenCalledTimes(1);
    expect(mockedShowReviewNotification).toHaveBeenCalledWith(
      "2 document(s) awaiting review",
      expect.stringContaining("notes/product.md"),
    );
    hasFocus.mockRestore();
  });

  it("summarizes pending work when an agent session ends", async () => {
    render(<App />);
    fireEvent.click(
      screen.getByRole("button", { name: "Open Markdown Workspace" }),
    );

    await waitFor(() => expect(mockedStartWorkspaceWatch).toHaveBeenCalled());
    emitChangeSet?.(externalChangeSet("cs-1"));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Changes" })).toHaveTextContent("1"),
    );

    emitAgentSession?.({ source: "Claude Code" });

    await waitFor(() =>
      expect(mockedShowReviewNotification).toHaveBeenCalledWith(
        "Claude Code session finished",
        "1 document(s) with 1 change(s) awaiting review.",
      ),
    );
  });

  it("notifies from a cold start launched with the agent notify flag", async () => {
    mockedTakeAgentNotifySource.mockResolvedValueOnce("Claude Code");
    render(<App />);
    fireEvent.click(
      screen.getByRole("button", { name: "Open Markdown Workspace" }),
    );

    await waitFor(() =>
      expect(mockedShowReviewNotification).toHaveBeenCalledWith(
        "Claude Code session finished",
        "No pending Markdown changes to review.",
      ),
    );
  });

  it("opens a structured review and highlights changed sentences", async () => {
    render(<App />);
    fireEvent.click(
      screen.getByRole("button", { name: "Open Markdown Workspace" }),
    );

    await waitFor(() => expect(mockedStartWorkspaceWatch).toHaveBeenCalled());
    emitChangeSet?.({
      id: "cs-1",
      relativePath: "notes/product.md",
      changeType: "modified",
      baseVersionId: "version-hash-1",
      baseHash: "hash-1",
      candidateVersionId: "version-hash-2",
      candidateHash: "hash-2",
      status: "pending",
      sourceType: "external",
      detectedAt: Date.now(),
      schemaVersion: 1,
    });

    fireEvent.click(screen.getByRole("button", { name: "Changes" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "Review notes/product.md" }),
    );

    expect(await screen.findByText("Paragraph rewritten")).toBeInTheDocument();
    expect(screen.getByText("第二句需要修改。")).toHaveClass("old");
    expect(screen.getByText("第二句已经修改。")).toHaveClass("new");
    expect(screen.getByRole("button", { name: "Accept" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Reject" })).toBeInTheDocument();
  });

  it("applies a complete set of review decisions", async () => {
    render(<App />);
    fireEvent.click(
      screen.getByRole("button", { name: "Open Markdown Workspace" }),
    );
    await waitFor(() => expect(mockedStartWorkspaceWatch).toHaveBeenCalled());
    emitChangeSet?.({
      id: "cs-1",
      relativePath: "notes/product.md",
      changeType: "modified",
      baseVersionId: "version-hash-1",
      baseHash: "hash-1",
      candidateVersionId: "version-hash-2",
      candidateHash: "hash-2",
      status: "pending",
      sourceType: "external",
      detectedAt: Date.now(),
      schemaVersion: 1,
    });

    fireEvent.click(screen.getByRole("button", { name: "Changes" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "Review notes/product.md" }),
    );
    const applyButton = await screen.findByRole("button", {
      name: "Apply decisions",
    });
    expect(applyButton).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: "Accept" }));
    expect(applyButton).toBeEnabled();
    fireEvent.click(applyButton);

    await waitFor(() => {
      expect(mockedResolveChangeSet).toHaveBeenCalledWith("cs-1", [
        { changeId: "change-1", decision: "accepted" },
      ]);
    });
    expect(await screen.findByText("Review applied to disk.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Changes" })).toHaveTextContent("0");
  });

  it("clears superseded chain members when a review resolves", async () => {
    render(<App />);
    fireEvent.click(
      screen.getByRole("button", { name: "Open Markdown Workspace" }),
    );
    await waitFor(() => expect(mockedStartWorkspaceWatch).toHaveBeenCalled());
    emitChangeSet?.(externalChangeSet("cs-1"));
    emitChangeSet?.(externalChangeSet("cs-2"));

    fireEvent.click(screen.getByRole("button", { name: "Changes" }));
    const reviews = await screen.findAllByRole("button", {
      name: "Review notes/product.md",
    });
    expect(reviews).toHaveLength(2);
    fireEvent.click(reviews[0]);

    fireEvent.click(await screen.findByRole("button", { name: "Accept" }));
    fireEvent.click(screen.getByRole("button", { name: "Apply decisions" }));

    await waitFor(() => expect(mockedResolveChangeSet).toHaveBeenCalled());
    await waitFor(() => {
      expect(
        screen.queryByRole("button", { name: "Review notes/product.md" }),
      ).not.toBeInTheDocument();
    });
    expect(screen.getByRole("button", { name: "Changes" })).toHaveTextContent("0");
  });

  it("switches the full interface to Chinese and remembers the preference", () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "中" }));

    expect(screen.getByRole("button", { name: "编辑" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "改动" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "保存" })).toBeInTheDocument();
    expect(window.localStorage.getItem("amr-language")).toBe("zh");
  });

  it("keeps the review pending when the disk changes during review", async () => {
    mockedResolveChangeSet.mockRejectedValue({
      code: "REVIEW_CONFLICT",
      message: "conflict",
    });
    render(<App />);
    fireEvent.click(
      screen.getByRole("button", { name: "Open Markdown Workspace" }),
    );
    await waitFor(() => expect(mockedStartWorkspaceWatch).toHaveBeenCalled());
    emitChangeSet?.({
      id: "cs-1",
      relativePath: "notes/product.md",
      changeType: "modified",
      baseVersionId: "version-hash-1",
      baseHash: "hash-1",
      candidateVersionId: "version-hash-2",
      candidateHash: "hash-2",
      status: "pending",
      sourceType: "external",
      detectedAt: Date.now(),
      schemaVersion: 1,
    });

    fireEvent.click(screen.getByRole("button", { name: "Changes" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "Review notes/product.md" }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "Reject all" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply decisions" }));

    expect(
      await screen.findByText(
        "The file changed again during review. Nothing was overwritten.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Reject" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("button", { name: "Changes" })).toHaveTextContent("1");
  });

  it("shows a persistent version timeline and previews historical content", async () => {
    render(<App />);
    fireEvent.click(
      screen.getByRole("button", { name: "Open Markdown Workspace" }),
    );
    fireEvent.click(await screen.findByRole("button", { name: /product\.md/i }));
    await screen.findByRole("textbox", { name: "Markdown editor" });
    await waitFor(() => {
      expect(screen.queryByText("Opening")).not.toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: "History" }));

    expect(await screen.findByText("2 version(s)")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /File snapshot/ })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    fireEvent.click(screen.getByRole("button", { name: /Editor save/ }));
    expect(await screen.findByText("# Older product")).toBeInTheDocument();
    expect(mockedGetDocumentVersion).toHaveBeenCalledWith(
      "/workspace",
      "notes/product.md",
      "version-old",
    );
  });

  it("restores a selected version and updates the editor content", async () => {
    vi.spyOn(window, "confirm").mockReturnValue(true);
    mockedListDocumentVersions
      .mockResolvedValueOnce([
        {
          id: "version-current",
          relativePath: "notes/product.md",
          contentHash: "hash-1",
          encoding: "utf-8",
          versionType: "snapshot",
          createdAt: 2,
          sourceType: "filesystem",
          schemaVersion: 2,
        },
        {
          id: "version-old",
          relativePath: "notes/product.md",
          contentHash: "hash-old",
          encoding: "utf-8",
          versionType: "editor",
          createdAt: 1,
          sourceType: "editor",
          schemaVersion: 2,
        },
      ])
      .mockResolvedValueOnce([
        {
          id: "version-restored",
          relativePath: "notes/product.md",
          contentHash: "hash-restored",
          encoding: "utf-8",
          versionType: "restore",
          createdAt: 3,
          sourceType: "history",
          schemaVersion: 2,
        },
      ]);
    render(<App />);
    fireEvent.click(
      screen.getByRole("button", { name: "Open Markdown Workspace" }),
    );
    fireEvent.click(await screen.findByRole("button", { name: /product\.md/i }));
    await screen.findByRole("textbox", { name: "Markdown editor" });
    await waitFor(() => {
      expect(screen.queryByText("Opening")).not.toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole("button", { name: "History" }));
    fireEvent.click(await screen.findByRole("button", { name: /Editor save/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Restore this version" }));

    await waitFor(() => {
      expect(mockedRestoreDocumentVersion).toHaveBeenCalledWith(
        "/workspace",
        "notes/product.md",
        "version-old",
      );
    });
    expect(await screen.findByText("Historical version restored.")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Editor" }));
    expect(screen.getByRole("textbox", { name: "Markdown editor" })).toHaveValue(
      "# Older product",
    );
  });

  it("completes the V0.1 path from editing through external review and history restore", async () => {
    vi.spyOn(window, "confirm").mockReturnValue(true);
    mockedGetChangeSetReview.mockResolvedValueOnce({
      summary: {
        id: "cs-e2e",
        relativePath: "notes/product.md",
        changeType: "modified",
        baseVersionId: "version-base",
        baseHash: "hash-base",
        candidateVersionId: "version-candidate",
        candidateHash: "hash-candidate",
        status: "pending",
        sourceType: "external",
        detectedAt: 1,
        schemaVersion: 1,
      },
      changes: [
        {
          id: "change-heading",
          sequence: 0,
          blockType: "heading",
          changeType: "rewritten",
          oldStart: 0,
          oldEnd: 9,
          newStart: 0,
          newEnd: 13,
          oldText: "# Product",
          newText: "# Agent product",
          oldSegments: [{ kind: "deleted", text: "# Product" }],
          newSegments: [{ kind: "added", text: "# Agent product" }],
        },
        {
          id: "change-paragraph",
          sequence: 1,
          blockType: "paragraph",
          changeType: "added",
          oldStart: null,
          oldEnd: null,
          newStart: 15,
          newEnd: 34,
          oldText: "",
          newText: "External revision.",
          oldSegments: [],
          newSegments: [{ kind: "added", text: "External revision." }],
        },
      ],
    });
    mockedResolveChangeSet.mockResolvedValueOnce({
      content: "# Agent product",
      contentHash: "hash-reviewed",
    });

    render(<App />);
    fireEvent.click(
      screen.getByRole("button", { name: "Open Markdown Workspace" }),
    );
    fireEvent.click(await screen.findByRole("button", { name: /product\.md/i }));
    const editor = await screen.findByRole("textbox", { name: "Markdown editor" });
    fireEvent.change(editor, { target: { value: "# Product draft" } });
    fireEvent.click(screen.getByRole("button", { name: /save/i }));
    expect(await screen.findByText("Saved to disk.")).toBeInTheDocument();

    emitChangeSet?.({
      id: "cs-e2e",
      relativePath: "notes/product.md",
      changeType: "modified",
      baseVersionId: "version-base",
      baseHash: "hash-base",
      candidateVersionId: "version-candidate",
      candidateHash: "hash-candidate",
      status: "pending",
      sourceType: "external",
      detectedAt: Date.now(),
      schemaVersion: 1,
    });
    fireEvent.click(screen.getByRole("button", { name: "Changes" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "Review notes/product.md" }),
    );

    const acceptButtons = await screen.findAllByRole("button", { name: "Accept" });
    const rejectButtons = screen.getAllByRole("button", { name: "Reject" });
    fireEvent.click(acceptButtons[0]);
    fireEvent.click(rejectButtons[1]);
    fireEvent.click(screen.getByRole("button", { name: "Apply decisions" }));
    expect(await screen.findByText("Review applied to disk.")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Editor" }));
    expect(screen.getByRole("textbox", { name: "Markdown editor" })).toHaveValue(
      "# Agent product",
    );

    fireEvent.click(screen.getByRole("button", { name: "History" }));
    fireEvent.click(await screen.findByRole("button", { name: /Editor save/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Restore this version" }));
    expect(await screen.findByText("Historical version restored.")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Editor" }));
    expect(screen.getByRole("textbox", { name: "Markdown editor" })).toHaveValue(
      "# Older product",
    );
  });
});
