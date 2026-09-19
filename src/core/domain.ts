export type Identifier = string;

export interface Workspace {
  id: Identifier;
  rootPath: string;
  displayName: string;
  schemaVersion: number;
}

export interface Document {
  id: Identifier;
  workspaceId: Identifier;
  relativePath: string;
  contentHash: string;
  currentVersionId: Identifier | null;
  schemaVersion: number;
}

export type ChangeSetStatus =
  | "pending"
  | "partially_resolved"
  | "accepted"
  | "rejected"
  | "superseded"
  | "failed";

export interface ChangeSet {
  id: Identifier;
  documentId: Identifier;
  baseVersionId: Identifier;
  candidateVersionId: Identifier;
  status: ChangeSetStatus;
  sourceType: "external" | "editor" | "unknown";
  source?: string;
  agent?: string;
  reason?: string;
  sessionId?: string;
  schemaVersion: number;
  metadata: Record<string, unknown>;
}
