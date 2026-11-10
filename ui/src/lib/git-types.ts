export type GitFileStatusKind =
  | "modified"
  | "added"
  | "deleted"
  | "renamed"
  | "untracked"
  | "conflict";

export interface GitFileStatus {
  filePath: string;
  status: GitFileStatusKind;
  code: "M" | "A" | "D" | "R" | "U" | "C";
  indexStatus: string;
  worktreeStatus: string;
}

export interface GitStatusResponse {
  isGitRepository: boolean;
  repositoryRoot: string | null;
  branch?: string | null;
  upstream?: string | null;
  ahead?: number;
  behind?: number;
  stagedCount?: number;
  unstagedCount?: number;
  untrackedCount?: number;
  conflictsCount?: number;
  files: GitFileStatus[];
  additions: number;
  deletions: number;
}

export interface GitRepoSummary {
  path: string;
  name: string;
  relativePath: string;
  isCurrent: boolean;
  branch: string | null;
  head: string | null;
  ahead: number;
  behind: number;
  staged: number;
  unstaged: number;
  untracked: number;
  conflicts: number;
  dirty: boolean;
}

export interface GitHistoryCommit {
  sha: string;
  shortSha: string;
  parents: string[];
  author: string;
  authorEmail: string;
  date: string;
  subject: string;
  refs: string[];
}

export interface GitCommitFile {
  path: string;
  oldPath?: string | null;
  status: "added" | "modified" | "deleted" | "renamed" | "copied" | string;
  additions: number;
  deletions: number;
  binary: boolean;
}

export interface GitCommitDetail extends GitHistoryCommit {
  body: string;
  additions: number;
  deletions: number;
  files: GitCommitFile[];
}

export interface GitFileDiffResponse {
  supported: boolean;
  status?: GitFileStatusKind;
  patch?: string;
}
