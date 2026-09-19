import {
  ArrowLeft,
  Ban,
  Check,
  ChevronRight,
  Clock3,
  FileText,
  FolderOpen,
  Inbox,
  LoaderCircle,
  PanelLeftClose,
  PanelLeftOpen,
  Plus,
  RotateCcw,
  Save,
  TriangleAlert,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { MarkdownEditor } from "./editor/MarkdownEditor";
import marginpostMark from "./assets/marginpost-mark.png";
import {
  initialLanguage,
  setStoredLanguage,
  translator,
  type Language,
  type Translate,
} from "./i18n";
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
  normalizeWorkspaceError,
  readMarkdownFile,
  resolveChangeSet,
  restoreDocumentVersion,
  saveMarkdownFile,
  startWorkspaceDrop,
  startWorkspaceWatch,
  type ChangeSetSummary,
  type ChangeSetReview,
  type DiffSegment,
  type DocumentVersion,
  type DocumentVersionSummary,
  type DocumentEntry,
  type MarkdownBlockType,
  type OpenedDocument,
  type ReviewDecision,
  type StructuredChange,
} from "./services/workspace";

type Section = "editor" | "changes" | "history";

const sections = [
  { id: "editor", icon: FileText },
  { id: "changes", icon: Inbox },
  { id: "history", icon: Clock3 },
] as const;

function workspaceName(path: string) {
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
}

function fileDepth(path: string) {
  return Math.max(0, path.split("/").length - 1);
}

function detectedTime(timestamp: number, language: Language) {
  return new Intl.DateTimeFormat(language === "zh" ? "zh-CN" : "en", {
    hour: "2-digit",
    minute: "2-digit",
  }).format(timestamp);
}

function versionTime(timestamp: number, language: Language) {
  return new Intl.DateTimeFormat(language === "zh" ? "zh-CN" : "en", {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(timestamp);
}

const versionTypeKeys: Record<
  DocumentVersionSummary["versionType"],
  | "snapshotVersion"
  | "preEditVersion"
  | "editorVersion"
  | "externalVersion"
  | "reviewAcceptedVersion"
  | "reviewRejectedVersion"
  | "reviewMixedVersion"
  | "reviewDiscardedVersion"
  | "preRestoreVersion"
  | "restoreVersionType"
> = {
  snapshot: "snapshotVersion",
  pre_edit: "preEditVersion",
  editor: "editorVersion",
  external: "externalVersion",
  review_accepted: "reviewAcceptedVersion",
  review_rejected: "reviewRejectedVersion",
  review_mixed: "reviewMixedVersion",
  review_discarded: "reviewDiscardedVersion",
  pre_restore: "preRestoreVersion",
  restore: "restoreVersionType",
};

const blockKeys: Record<
  MarkdownBlockType,
  "heading" | "paragraph" | "list" | "quote" | "code" | "table" | "unknown"
> = {
  heading: "heading",
  paragraph: "paragraph",
  list: "list",
  quote: "quote",
  code: "code",
  table: "table",
  unknown: "unknown",
};

function changeLabel(change: StructuredChange, t: Translate) {
  return `${t(blockKeys[change.blockType])} ${t(change.changeType)}`;
}

function translatedError(error: unknown, t: Translate) {
  const normalized = normalizeWorkspaceError(error);
  const keys: Record<string, Parameters<Translate>[0]> = {
    CHANGE_SET_UNAVAILABLE: "reviewUnavailable",
    INCOMPLETE_DECISIONS: "incompleteDecisions",
    REVIEW_CONFLICT: "reviewConflict",
    INVALID_DIFF_PLAN: "invalidDiffPlan",
    FILE_CHANGED: "fileChanged",
    VERSION_UNAVAILABLE: "versionUnavailable",
    HISTORY_STORE_FAILED: "historyStoreFailed",
    UNSUPPORTED_DROP: "unsupportedDrop",
    DROPPED_PATH_UNAVAILABLE: "droppedPathUnavailable",
    DROPPED_FILE_UNAVAILABLE: "droppedFileUnavailable",
  };
  return keys[normalized.code] ? t(keys[normalized.code]) : normalized.message || t("genericError");
}

function DiffText({
  segments,
  side,
}: {
  segments: DiffSegment[];
  side: "old" | "new";
}) {
  return (
    <pre className={`diff-text ${side}`}>
      {segments.map((segment, index) => (
        <mark
          className={segment.kind === "equal" ? "diff-segment equal" : `diff-segment ${side}`}
          key={`${segment.kind}-${index}`}
        >
          {segment.text}
        </mark>
      ))}
    </pre>
  );
}

export function App() {
  const [language, setLanguage] = useState<Language>(initialLanguage);
  const t = useMemo(() => translator(language), [language]);
  const languageRef = useRef(language);
  const [activeSection, setActiveSection] = useState<Section>("editor");
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [rootPath, setRootPath] = useState<string | null>(null);
  const [documents, setDocuments] = useState<DocumentEntry[]>([]);
  const [openedDocument, setOpenedDocument] = useState<OpenedDocument | null>(null);
  const [content, setContent] = useState("");
  const [savedContent, setSavedContent] = useState("");
  const [changeSets, setChangeSets] = useState<ChangeSetSummary[]>([]);
  const [selectedReview, setSelectedReview] = useState<ChangeSetReview | null>(null);
  const [decisions, setDecisions] = useState<Record<string, ReviewDecision>>({});
  const [versions, setVersions] = useState<DocumentVersionSummary[]>([]);
  const [selectedVersion, setSelectedVersion] = useState<DocumentVersion | null>(null);
  const [busy, setBusy] = useState<
    | "opening"
    | "reading"
    | "saving"
    | "reviewing"
    | "applying"
    | "history"
    | "restoring"
    | null
  >(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [dragActive, setDragActive] = useState(false);
  const changeSetsRef = useRef(changeSets);
  const pendingNotifyRef = useRef<ChangeSetSummary[]>([]);
  const pendingNotifyTimerRef = useRef<number | null>(null);

  const dirty = openedDocument !== null && content !== savedContent;
  const openedRelativePath = openedDocument?.relativePath ?? null;
  const resolvedCount = Object.keys(decisions).length;
  const reviewReady =
    selectedReview !== null &&
    selectedReview.changes.length > 0 &&
    resolvedCount === selectedReview.changes.length;

  useEffect(() => {
    languageRef.current = language;
    setStoredLanguage(language);
  }, [language]);

  useEffect(() => {
    changeSetsRef.current = changeSets;
  }, [changeSets]);

  useEffect(() => {
    void ensureNotificationAccess();
  }, []);

  useEffect(() => {
    void updateDockBadge(rootPath ? changeSets.length : 0);
  }, [rootPath, changeSets]);

  const focusChangesView = useCallback(() => {
    setActiveSection("changes");
    void getCurrentWindow().show().catch(() => {});
    void getCurrentWindow().setFocus().catch(() => {});
  }, []);

  const notifyAgentSession = useCallback(
    (source: string, pending: ChangeSetSummary[] = changeSetsRef.current) => {
      const translate = translator(languageRef.current);
      if (pending.length === 0) {
        void showReviewNotification(
          translate("notifyAgentTitle", { source }),
          translate("notifyAgentIdleBody"),
        );
        return;
      }
      void Promise.all(
        pending.map((summary) => getChangeSetReview(summary.id).catch(() => null)),
      ).then((reviews) => {
        const changes = reviews.reduce(
          (total, review) => total + (review?.changes.length ?? 0),
          0,
        );
        void showReviewNotification(
          translate("notifyAgentTitle", { source }),
          translate("notifyAgentBody", { count: pending.length, changes }),
        );
      });
    },
    [],
  );

  useEffect(() => {
    let stopped: (() => void) | null = null;
    void watchAgentSessionEnded((event) => {
      notifyAgentSession(event.source);
    }).then((unlisten) => {
      stopped = unlisten;
    });
    return () => stopped?.();
  }, [notifyAgentSession]);

  useEffect(() => {
    let stopped: (() => void) | null = null;
    void watchNotificationActivation(focusChangesView).then((stop) => {
      stopped = stop;
    });
    return () => stopped?.();
  }, [focusChangesView]);

  const flushPendingNotifications = useCallback(() => {
    if (pendingNotifyTimerRef.current !== null) {
      window.clearTimeout(pendingNotifyTimerRef.current);
      pendingNotifyTimerRef.current = null;
    }
    const queued = pendingNotifyRef.current;
    pendingNotifyRef.current = [];
    if (queued.length === 0 || document.hasFocus()) {
      return;
    }
    const translate = translator(languageRef.current);
    const paths = queued
      .slice(0, 3)
      .map((summary) => summary.relativePath)
      .concat(queued.length > 3 ? ["…"] : [])
      .join("  ");
    void showReviewNotification(
      translate("notifyPendingTitle", { count: queued.length }),
      translate("notifyPendingBody", { paths }),
    );
  }, []);

  const openDocument = useCallback(
    async (document: DocumentEntry) => {
      if (!rootPath || busy) {
        return;
      }
      if (dirty) {
        setError(t("saveDraftFirst"));
        return;
      }

      setBusy("reading");
      setError(null);
      setMessage(null);
      try {
        const opened = await readMarkdownFile(rootPath, document.relativePath);
        setOpenedDocument(opened);
        setContent(opened.content);
        setSavedContent(opened.content);
        setVersions([]);
        setSelectedVersion(null);
        setActiveSection("editor");
      } catch (caught) {
        setError(translatedError(caught, t));
      } finally {
        setBusy(null);
      }
    },
    [busy, dirty, rootPath, t],
  );

  const applyWorkspaceSelection = useCallback(
    async (
      selectedPath: string,
      selectedRelativePath: string | undefined,
      openedByDrop: boolean,
    ) => {
      if (selectedPath === rootPath && !selectedRelativePath) {
        const existingChanges = await listChangeSets();
        setChangeSets(existingChanges);
        setMessage(t("workspaceAlreadyOpen"));
        return;
      }

      const discovered = await listMarkdownFiles(selectedPath);
      const target = selectedRelativePath
        ? discovered.find(
            (document) => document.relativePath === selectedRelativePath,
          )
        : undefined;
      if (selectedRelativePath && !target) {
        throw {
          code: "DROPPED_FILE_UNAVAILABLE",
          message: "The dropped Markdown file could not be opened.",
        };
      }
      const opened = target
        ? await readMarkdownFile(selectedPath, target.relativePath)
        : null;
      const existingChanges =
        selectedPath === rootPath ? await listChangeSets() : [];

      setRootPath(selectedPath);
      setDocuments(discovered);
      setOpenedDocument(opened);
      setContent(opened?.content ?? "");
      setSavedContent(opened?.content ?? "");
      setChangeSets(existingChanges);
      setSelectedReview(null);
      setDecisions({});
      setVersions([]);
      setSelectedVersion(null);
      setActiveSection("editor");
      setMessage(
        openedByDrop
          ? target
            ? t("droppedFileOpened", { path: target.relativePath })
            : t("droppedWorkspaceOpened", { count: discovered.length })
          : discovered.length === 0
            ? t("noMarkdownFound")
            : t("filesFound", { count: discovered.length }),
      );
    },
    [rootPath, t],
  );

  const openWorkspace = useCallback(async () => {
    if (busy) {
      return;
    }
    if (dirty) {
      setError(t("saveDraftBeforeWorkspace"));
      return;
    }

    setBusy("opening");
    setError(null);
    setMessage(null);
    try {
      const selectedPath = await chooseWorkspace(t("openWorkspaceTitle"));
      if (!selectedPath) {
        return;
      }
      await applyWorkspaceSelection(selectedPath, undefined, false);
    } catch (caught) {
      setError(translatedError(caught, t));
    } finally {
      setBusy(null);
    }
  }, [applyWorkspaceSelection, busy, dirty, t]);

  const handleDroppedPaths = useCallback(
    async (paths: string[]) => {
      if (busy) return;
      if (dirty) {
        setError(t("saveDraftBeforeWorkspace"));
        return;
      }
      if (paths.length !== 1) {
        setError(t("dropMultiple"));
        return;
      }

      setBusy("opening");
      setError(null);
      setMessage(null);
      try {
        const selection = await inspectDroppedPath(paths[0]);
        await applyWorkspaceSelection(
          selection.rootPath,
          selection.selectedRelativePath,
          true,
        );
      } catch (caught) {
        setError(translatedError(caught, t));
      } finally {
        setBusy(null);
      }
    },
    [applyWorkspaceSelection, busy, dirty, t],
  );

  useEffect(() => {
    let active = true;
    let stopListening: (() => void) | null = null;

    void startWorkspaceDrop(
      (paths) => {
        if (active) void handleDroppedPaths(paths);
      },
      (dragging) => {
        if (active) setDragActive(dragging);
      },
    )
      .then((stop) => {
        if (!active) {
          void stop();
          return;
        }
        stopListening = stop;
      })
      .catch((caught) => {
        if (active) {
          setError(translatedError(caught, translator(languageRef.current)));
        }
      });

    return () => {
      active = false;
      if (stopListening) void stopListening();
    };
  }, [handleDroppedPaths]);

  const openReview = useCallback(
    async (changeSet: ChangeSetSummary) => {
      if (busy) {
        return;
      }
      setBusy("reviewing");
      setError(null);
      setMessage(null);
      try {
        const review = await getChangeSetReview(changeSet.id);
        if (!review) {
          setError(t("reviewUnavailable"));
          return;
        }
        setSelectedReview(review);
        setDecisions({});
      } catch (caught) {
        setError(translatedError(caught, t));
      } finally {
        setBusy(null);
      }
    },
    [busy, t],
  );

  const decideChange = useCallback((changeId: string, decision: ReviewDecision) => {
    setDecisions((current) => ({ ...current, [changeId]: decision }));
  }, []);

  const decideAll = useCallback(
    (decision: ReviewDecision) => {
      if (!selectedReview) return;
      setDecisions(
        Object.fromEntries(
          selectedReview.changes.map((change) => [change.id, decision]),
        ),
      );
    },
    [selectedReview],
  );

  const discardReview = useCallback(async () => {
    if (!selectedReview || busy) {
      return;
    }
    setBusy("applying");
    setError(null);
    setMessage(null);
    try {
      await discardChangeSet(selectedReview.summary.id);
      setSelectedReview(null);
      setDecisions({});
      if (rootPath) {
        setChangeSets(await listChangeSets());
      }
      setMessage(t("discardedNotice"));
    } catch (caught) {
      setError(translatedError(caught, t));
    } finally {
      setBusy(null);
    }
  }, [busy, rootPath, selectedReview, t]);

  const applyReview = useCallback(async () => {
    if (!selectedReview || !reviewReady || busy) {
      return;
    }
    if (
      dirty &&
      openedDocument?.relativePath === selectedReview.summary.relativePath
    ) {
      setError(t("saveDraftBeforeReview"));
      return;
    }

    setBusy("applying");
    setError(null);
    setMessage(null);
    try {
      const result = await resolveChangeSet(
        selectedReview.summary.id,
        selectedReview.changes.map((change) => ({
          changeId: change.id,
          decision: decisions[change.id],
        })),
      );
      if (rootPath) {
        setChangeSets(await listChangeSets());
      }
      if (openedDocument?.relativePath === selectedReview.summary.relativePath) {
        setOpenedDocument({
          ...openedDocument,
          content: result.content,
          contentHash: result.contentHash,
        });
        setContent(result.content);
        setSavedContent(result.content);
      }
      setSelectedReview(null);
      setDecisions({});
      setMessage(t("reviewApplied"));
    } catch (caught) {
      setError(translatedError(caught, t));
    } finally {
      setBusy(null);
    }
  }, [
    busy,
    decisions,
    dirty,
    openedDocument,
    reviewReady,
    rootPath,
    selectedReview,
    t,
  ]);

  const saveDocument = useCallback(async () => {
    if (!rootPath || !openedDocument || !dirty || busy) {
      return;
    }

    setBusy("saving");
    setError(null);
    setMessage(null);
    try {
      const result = await saveMarkdownFile(rootPath, openedDocument, content);
      setOpenedDocument({ ...openedDocument, content, contentHash: result.contentHash });
      setSavedContent(content);
      setMessage(t("savedToDisk"));
    } catch (caught) {
      setError(translatedError(caught, t));
    } finally {
      setBusy(null);
    }
  }, [busy, content, dirty, openedDocument, rootPath, t]);

  const loadHistory = useCallback(async () => {
    if (!rootPath || !openedRelativePath) {
      setVersions([]);
      setSelectedVersion(null);
      return;
    }
    setBusy("history");
    setError(null);
    try {
      const items = await listDocumentVersions(
        rootPath,
        openedRelativePath,
      );
      setVersions(items);
      if (items.length === 0) {
        setSelectedVersion(null);
        return;
      }
      const detail = await getDocumentVersion(
        rootPath,
        openedRelativePath,
        items[0].id,
      );
      setSelectedVersion(detail);
    } catch (caught) {
      setError(translatedError(caught, t));
    } finally {
      setBusy(null);
    }
  }, [openedRelativePath, rootPath, t]);

  const selectVersion = useCallback(
    async (version: DocumentVersionSummary) => {
      if (!rootPath || !openedDocument || busy) return;
      setBusy("history");
      setError(null);
      try {
        const detail = await getDocumentVersion(
          rootPath,
          openedDocument.relativePath,
          version.id,
        );
        if (!detail) {
          setError(t("versionUnavailable"));
          return;
        }
        setSelectedVersion(detail);
      } catch (caught) {
        setError(translatedError(caught, t));
      } finally {
        setBusy(null);
      }
    },
    [busy, openedDocument, rootPath, t],
  );

  const restoreVersion = useCallback(async () => {
    if (!rootPath || !openedDocument || !selectedVersion || busy) return;
    if (dirty) {
      setError(t("restoreDraftFirst"));
      return;
    }
    if (!window.confirm(t("confirmRestore"))) return;
    setBusy("restoring");
    setError(null);
    setMessage(null);
    try {
      const result = await restoreDocumentVersion(
        rootPath,
        openedDocument.relativePath,
        selectedVersion.id,
      );
      setOpenedDocument({
        ...openedDocument,
        content: result.content,
        contentHash: result.contentHash,
        encoding: result.encoding,
      });
      setContent(result.content);
      setSavedContent(result.content);
      const items = await listDocumentVersions(
        rootPath,
        openedDocument.relativePath,
      );
      setVersions(items);
      const detail = await getDocumentVersion(
        rootPath,
        openedDocument.relativePath,
        result.version.id,
      );
      setSelectedVersion(detail);
      setMessage(t("versionRestored"));
    } catch (caught) {
      setError(translatedError(caught, t));
    } finally {
      setBusy(null);
    }
  }, [busy, dirty, openedDocument, rootPath, selectedVersion, t]);

  useEffect(() => {
    if (activeSection === "history") {
      void loadHistory();
    }
  }, [activeSection, loadHistory]);

  useEffect(() => {
    const handleSaveShortcut = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s") {
        event.preventDefault();
        void saveDocument();
      }
    };
    window.addEventListener("keydown", handleSaveShortcut);
    return () => window.removeEventListener("keydown", handleSaveShortcut);
  }, [saveDocument]);

  useEffect(() => {
    if (!message) {
      return;
    }
    const timeout = window.setTimeout(() => setMessage(null), 2500);
    return () => window.clearTimeout(timeout);
  }, [message]);

  useEffect(() => {
    if (!rootPath) {
      return;
    }

    let active = true;
    let stopWatching: (() => Promise<void>) | null = null;
    const receiveChangeSet = (changeSet: ChangeSetSummary) => {
      if (!active) {
        return;
      }
      setChangeSets((current) =>
        current.some((item) => item.id === changeSet.id)
          ? current
          : [changeSet, ...current],
      );
      setMessage(
        translator(languageRef.current)("changedExternally", {
          path: changeSet.relativePath,
        }),
      );
      pendingNotifyRef.current = [...pendingNotifyRef.current, changeSet];
      if (pendingNotifyTimerRef.current === null) {
        pendingNotifyTimerRef.current = window.setTimeout(() => {
          pendingNotifyTimerRef.current = null;
          flushPendingNotifications();
        }, 3000);
      }
      void listMarkdownFiles(rootPath)
        .then((discovered) => {
          if (active) setDocuments(discovered);
        })
        .catch(() => {});
    };

    void startWorkspaceWatch(rootPath, receiveChangeSet)
      .then(async (stop) => {
        if (!active) {
          await stop();
          return;
        }
        stopWatching = stop;
        const existing = await listChangeSets();
        if (active) {
          setChangeSets(existing);
        }
        const source = await takeAgentNotifySource();
        if (active && source) {
          notifyAgentSession(source, existing);
        }
      })
      .catch((caught) => {
        if (active) {
          setError(translatedError(caught, translator(languageRef.current)));
        }
      });

    return () => {
      active = false;
      if (pendingNotifyTimerRef.current !== null) {
        window.clearTimeout(pendingNotifyTimerRef.current);
        pendingNotifyTimerRef.current = null;
      }
      pendingNotifyRef.current = [];
      if (stopWatching) {
        void stopWatching();
      }
    };
  }, [rootPath, flushPendingNotifications, notifyAgentSession]);

  const statusLabel = useMemo(() => {
    if (busy === "saving") return t("saving");
    if (busy === "reading") return t("opening");
    if (busy === "opening") return t("scanning");
    if (busy === "reviewing") return t("loadingReview");
    if (busy === "applying") return t("applyingReview");
    if (busy === "history") return t("loadingHistory");
    if (busy === "restoring") return t("restoringVersion");
    if (dirty) return t("unsaved");
    if (openedDocument) return t("saved");
    return rootPath ? t("workspaceReady") : t("noWorkspace");
  }, [busy, dirty, openedDocument, rootPath, t]);

  return (
    <main
      className={[
        "app-shell",
        sidebarCollapsed ? "sidebar-collapsed" : "",
        dragActive ? "drag-active" : "",
      ]
        .filter(Boolean)
        .join(" ")}
    >
      {dragActive ? (
        <div className="drop-overlay" role="status">
          <FolderOpen size={24} />
          <strong>{t("dropWorkspaceHint")}</strong>
        </div>
      ) : null}
      <aside className="sidebar">
        <div className="brand-row">
          <div className="brand-mark" aria-hidden="true">
            <img alt="" src={marginpostMark} />
          </div>
          <div className="brand-name">
            <strong>Margin</strong>
            <span>Post</span>
          </div>
          <button
            aria-label={
              sidebarCollapsed ? t("expandSidebar") : t("collapseSidebar")
            }
            className="icon-button"
            onClick={() => setSidebarCollapsed((collapsed) => !collapsed)}
            title={sidebarCollapsed ? t("expandSidebar") : t("collapseSidebar")}
            type="button"
          >
            {sidebarCollapsed ? <PanelLeftOpen size={17} /> : <PanelLeftClose size={17} />}
          </button>
        </div>

        <nav aria-label={t("workspace")}>
          {sections.map(({ id, icon: Icon }) => (
            <button
              aria-label={t(id)}
              className={activeSection === id ? "nav-item active" : "nav-item"}
              key={id}
              onClick={() => {
                setActiveSection(id);
                if (id === "changes") {
                  setSelectedReview(null);
                  setDecisions({});
                }
              }}
              title={t(id)}
              type="button"
            >
              <Icon size={17} />
              <span>{t(id)}</span>
              {id === "changes" ? <small>{changeSets.length}</small> : null}
            </button>
          ))}
        </nav>

        <div className="file-section">
          <div className="section-heading">
            <span className="section-label">
              {rootPath ? workspaceName(rootPath) : t("workspace")}
            </span>
            <button
              aria-label={t("openFolder")}
              className="icon-button compact"
              disabled={busy !== null}
              onClick={() => void openWorkspace()}
              title={t("openFolder")}
              type="button"
            >
              <FolderOpen size={15} />
            </button>
          </div>

          <div className="file-list">
            {documents.map((document) => {
              const hasPendingChange = changeSets.some(
                (changeSet) => changeSet.relativePath === document.relativePath,
              );
              return (
                <button
                  className={
                    openedDocument?.relativePath === document.relativePath
                      ? "file-row active"
                      : "file-row"
                  }
                  key={document.relativePath}
                  onClick={() => void openDocument(document)}
                  style={{ paddingLeft: 9 + fileDepth(document.relativePath) * 14 }}
                  title={document.relativePath}
                  type="button"
                >
                  <FileText size={15} />
                  <span>{document.name}</span>
                  {hasPendingChange ? (
                    <span
                      aria-label={t("pendingExternalChange")}
                      className="change-dot"
                    />
                  ) : null}
                </button>
              );
            })}
          </div>
        </div>

        <div className="sidebar-status">
          <span className={error ? "status-dot error" : "status-dot"} />
          <span>{statusLabel}</span>
        </div>
      </aside>

      <section className="workspace">
        <header className="topbar">
          <div className="document-title">
            <strong>{t(activeSection)}</strong>
            <span>{openedDocument?.relativePath ?? t("noFileOpen")}</span>
          </div>
          <div className="topbar-actions">
            {busy ? <LoaderCircle className="spin" size={15} /> : null}
            <span className={dirty ? "save-state dirty" : "save-state"}>{statusLabel}</span>
            <div aria-label={t("language")} className="language-switch" role="group">
              <button
                aria-pressed={language === "zh"}
                className={language === "zh" ? "active" : ""}
                onClick={() => setLanguage("zh")}
                type="button"
              >
                中
              </button>
              <button
                aria-pressed={language === "en"}
                className={language === "en" ? "active" : ""}
                onClick={() => setLanguage("en")}
                type="button"
              >
                EN
              </button>
            </div>
            <button
              className="command-button"
              disabled={!dirty || busy !== null}
              onClick={() => void saveDocument()}
              type="button"
            >
              <Save size={15} />
              <span>{t("save")}</span>
            </button>
          </div>
        </header>

        <div className="notice-region" aria-live="polite">
          {error ? (
            <div className="notice error">
              <TriangleAlert size={15} />
              <span>{error}</span>
            </div>
          ) : message ? (
            <div className="notice">
              <span>{message}</span>
            </div>
          ) : null}
        </div>

        {activeSection === "editor" ? (
          openedDocument ? (
            <MarkdownEditor value={content} onChange={setContent} />
          ) : (
            <div className="empty-state">
              <div>
                <FolderOpen size={24} />
                <h1>{rootPath ? t("chooseFile") : t("openWorkspace")}</h1>
                <p>
                  {rootPath ? t("chooseFileHelp") : t("openWorkspaceHelp")}
                </p>
                {!rootPath ? (
                  <button
                    aria-label={t("openWorkspaceTitle")}
                    className="primary-button"
                    disabled={busy !== null}
                    onClick={() => void openWorkspace()}
                    type="button"
                  >
                    <FolderOpen size={16} />
                    <span>{t("openFolder")}</span>
                  </button>
                ) : null}
              </div>
            </div>
          )
        ) : activeSection === "changes" ? (
          selectedReview ? (
            <div className="review-view">
              <header className="review-heading">
                <button
                  aria-label={t("backToChanges")}
                  className="icon-button"
                  onClick={() => {
                    setSelectedReview(null);
                    setDecisions({});
                  }}
                  title={t("backToChanges")}
                  type="button"
                >
                  <ArrowLeft size={17} />
                </button>
                <div>
                  <h1>{selectedReview.summary.relativePath}</h1>
                  <span>
                    {t("structuredChanges", {
                      count: selectedReview.changes.length,
                    })}
                    {selectedReview.summary.source
                      ? ` · ${selectedReview.summary.source}`
                      : ""}
                  </span>
                </div>
                <div className="review-actions">
                  <span>
                    {t("decisionsProgress", {
                      resolved: resolvedCount,
                      total: selectedReview.changes.length,
                    })}
                  </span>
                  <button
                    className="secondary-button"
                    onClick={() => decideAll("accepted")}
                    type="button"
                  >
                    <Check size={14} />
                    {t("acceptAll")}
                  </button>
                  <button
                    className="secondary-button"
                    onClick={() => decideAll("rejected")}
                    type="button"
                  >
                    <X size={14} />
                    {t("rejectAll")}
                  </button>
                  <button
                    className="secondary-button"
                    disabled={busy !== null}
                    onClick={() => void discardReview()}
                    type="button"
                  >
                    <Ban size={14} />
                    {t("discard")}
                  </button>
                </div>
              </header>
              {selectedReview.changes.length > 0 ? (
                <div className="review-list">
                  {selectedReview.changes.map((change) => (
                    <article className="review-change" key={change.id}>
                      <header>
                        <strong>{changeLabel(change, t)}</strong>
                        <span>{t("changeNumber", { count: change.sequence + 1 })}</span>
                      </header>
                      {change.oldSegments.length > 0 ? (
                        <div className="diff-side">
                          <span className="diff-sign old" aria-hidden="true">
                            -
                          </span>
                          <DiffText segments={change.oldSegments} side="old" />
                        </div>
                      ) : null}
                      {change.newSegments.length > 0 ? (
                        <div className="diff-side">
                          <span className="diff-sign new" aria-hidden="true">
                            +
                          </span>
                          <DiffText segments={change.newSegments} side="new" />
                        </div>
                      ) : null}
                      <div className="decision-control" role="group">
                        <button
                          aria-pressed={decisions[change.id] === "accepted"}
                          className={
                            decisions[change.id] === "accepted"
                              ? "decision-button accept selected"
                              : "decision-button accept"
                          }
                          onClick={() => decideChange(change.id, "accepted")}
                          type="button"
                        >
                          <Check size={14} />
                          {t("accept")}
                        </button>
                        <button
                          aria-pressed={decisions[change.id] === "rejected"}
                          className={
                            decisions[change.id] === "rejected"
                              ? "decision-button reject selected"
                              : "decision-button reject"
                          }
                          onClick={() => decideChange(change.id, "rejected")}
                          type="button"
                        >
                          <X size={14} />
                          {t("reject")}
                        </button>
                      </div>
                    </article>
                  ))}
                  <footer className="review-footer">
                    <span>
                      {t("decisionsProgress", {
                        resolved: resolvedCount,
                        total: selectedReview.changes.length,
                      })}
                    </span>
                    <button
                      className="apply-button"
                      disabled={!reviewReady || busy !== null}
                      onClick={() => void applyReview()}
                      type="button"
                    >
                      <Check size={15} />
                      {t("applyDecisions")}
                    </button>
                  </footer>
                </div>
              ) : (
                <div className="review-empty">{t("noContentChanges")}</div>
              )}
            </div>
          ) : changeSets.length > 0 ? (
            <div className="changes-view">
              <header className="view-heading">
                <h1>{t("changes")}</h1>
                <span>{t("pending", { count: changeSets.length })}</span>
              </header>
              <div className="change-list">
                {changeSets.map((changeSet) => (
                  <button
                    aria-label={t("reviewFile", { path: changeSet.relativePath })}
                    className="change-row"
                    key={changeSet.id}
                    onClick={() => void openReview(changeSet)}
                    type="button"
                  >
                    <div className="change-kind" aria-hidden="true">
                      {changeSet.changeType === "created" ? (
                        <Plus size={15} />
                      ) : (
                        <FileText size={15} />
                      )}
                    </div>
                    <div className="change-copy">
                      <strong>{changeSet.relativePath}</strong>
                      <span>
                        {changeSet.changeType === "created"
                          ? t("newMarkdownFile")
                          : t("externalModification")}
                        {changeSet.source ? ` · ${changeSet.source}` : ""}
                      </span>
                    </div>
                    <div className="change-meta">
                      <span>{detectedTime(changeSet.detectedAt, language)}</span>
                      <small>{t("pendingReview")}</small>
                    </div>
                    <ChevronRight className="change-arrow" size={16} />
                  </button>
                ))}
              </div>
            </div>
          ) : (
            <div className="empty-state">
              <div>
                <Inbox size={24} />
                <h1>{t("changes")}</h1>
                <p>{t("changesEmptyHelp")}</p>
              </div>
            </div>
          )
        ) : !openedDocument ? (
          <div className="empty-state">
            <div>
              <Clock3 size={24} />
              <h1>{t("history")}</h1>
              <p>{t("historyHelp")}</p>
            </div>
          </div>
        ) : versions.length === 0 ? (
          <div className="empty-state">
            <div>
              <Clock3 size={24} />
              <h1>{t("history")}</h1>
              <p>{busy === "history" ? t("loadingHistory") : t("noVersions")}</p>
            </div>
          </div>
        ) : (
          <div className="history-view">
            <header className="view-heading">
              <div>
                <h1>{openedDocument.relativePath}</h1>
                <span>{t("versionsCount", { count: versions.length })}</span>
              </div>
            </header>
            <div className="history-layout">
              <div className="version-list">
                {versions.map((version) => (
                  <button
                    aria-pressed={selectedVersion?.id === version.id}
                    className={
                      selectedVersion?.id === version.id
                        ? "version-row active"
                        : "version-row"
                    }
                    key={version.id}
                    onClick={() => void selectVersion(version)}
                    type="button"
                  >
                    <Clock3 size={15} />
                    <span>
                      <strong>{t(versionTypeKeys[version.versionType])}</strong>
                      <small>
                        {versionTime(version.createdAt, language)}
                        {version.source ? ` · ${version.source}` : ""}
                      </small>
                    </span>
                    {versions.find(
                      (item) => item.contentHash === openedDocument.contentHash,
                    )?.id === version.id ? (
                      <em>{t("currentVersion")}</em>
                    ) : null}
                  </button>
                ))}
              </div>
              <section className="version-preview">
                {selectedVersion ? (
                  <>
                    <header>
                      <div>
                        <strong>{t(versionTypeKeys[selectedVersion.versionType])}</strong>
                        <span>{versionTime(selectedVersion.createdAt, language)}</span>
                      </div>
                      <button
                        className="secondary-button"
                        disabled={
                          busy !== null ||
                          selectedVersion.contentHash === openedDocument.contentHash
                        }
                        onClick={() => void restoreVersion()}
                        type="button"
                      >
                        <RotateCcw size={14} />
                        {t("restoreVersion")}
                      </button>
                    </header>
                    <pre aria-label={t("versionPreview")}>
                      {selectedVersion.content}
                    </pre>
                  </>
                ) : (
                  <p>{t("selectVersion")}</p>
                )}
              </section>
            </div>
          </div>
        )}
      </section>
    </main>
  );
}
