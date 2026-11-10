import assert from "node:assert/strict";
import test from "node:test";
import { createJiti } from "jiti";

const jiti = createJiti(import.meta.url);
const { emptyGitWorkspace, loadGitWorkspaces, saveGitWorkspace } = await jiti.import("./git-workspace.ts");

function installStorage(initial = {}) {
  const values = new Map(Object.entries(initial));
  globalThis.window = {
    localStorage: {
      getItem(key) {
        return values.get(key) ?? null;
      },
      setItem(key, value) {
        values.set(key, value);
      },
      removeItem(key) {
        values.delete(key);
      },
    },
  };
  return values;
}

test("an empty store has no workspace", () => {
  installStorage();
  assert.deepEqual(loadGitWorkspaces(), {});
});

test("a workspace round-trips by project key", () => {
  installStorage();
  const state = {
    ...emptyGitWorkspace("/repo"),
    selectedRepoPath: "/repo",
    tab: "history",
    selectedFilePath: "src/app.ts",
    selectedFileScope: "staged",
    selectedCommitSha: "abc1234",
    selectedCommitFilePath: "src/app.ts",
  };
  saveGitWorkspace("/repo", state);
  assert.deepEqual(loadGitWorkspaces()["/repo"], state);
});

test("malformed tab and scope values fall back to the defaults", () => {
  installStorage({
    "pi-star:git-workspaces:v1": JSON.stringify({
      "/repo": {
        selectedRepoPath: "/repo",
        tab: "unknown",
        selectedFileScope: "unknown",
      },
    }),
  });
  assert.deepEqual(loadGitWorkspaces()["/repo"], {
    projectKey: "/repo",
    selectedRepoPath: "/repo",
    tab: "changes",
    selectedFilePath: null,
    selectedFileScope: "all",
    selectedCommitSha: null,
    selectedCommitFilePath: null,
  });
});
