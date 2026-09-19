import type { ChangeSet, Document, Workspace } from "./domain";

export interface FileSource {
  listDocuments(workspace: Workspace): Promise<Document[]>;
  readDocument(document: Document): Promise<string>;
  writeDocument(document: Document, content: string): Promise<void>;
}

export interface ChangeDetector {
  start(workspace: Workspace): Promise<void>;
  stop(): Promise<void>;
}

export interface DiffResult {
  changes: ReadonlyArray<{
    type: "insert" | "delete" | "replace";
    oldText: string;
    newText: string;
  }>;
}

export interface DiffEngine {
  compare(base: string, candidate: string): Promise<DiffResult>;
}

export interface AttributionProvider {
  resolve(changeSet: ChangeSet): Promise<{
    source?: string;
    agent?: string;
    reason?: string;
    sessionId?: string;
  }>;
}

export interface VersionStore {
  saveVersion(document: Document, content: string): Promise<string>;
  listVersionIds(document: Document): Promise<string[]>;
  restoreVersion(document: Document, versionId: string): Promise<string>;
}

export interface ReviewPolicy {
  canApply(changeSet: ChangeSet, diskHash: string): Promise<boolean>;
}
