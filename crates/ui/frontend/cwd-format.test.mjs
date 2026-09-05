import { test } from "node:test";
import assert from "node:assert/strict";
import { formatCwd } from "./cwd-format.js";

// --- home substitution ---

test("path starting with home (unix separators) → prefix becomes ~", () => {
  const home = "/home/user";
  const path = "/home/user/projects/foo";
  assert.equal(formatCwd(path, home), "~/projects/foo");
});

test("path starting with home (windows backslashes) → prefix becomes ~", () => {
  const home = "C:\\Users\\maurizio";
  const path = "C:\\Users\\maurizio\\Documents\\Codice";
  assert.equal(formatCwd(path, home), "~/Documents/Codice");
});

test("path equal to home → becomes ~ only", () => {
  const home = "/home/user";
  assert.equal(formatCwd("/home/user", home), "~");
});

// --- short path (under maxLen) → returned unchanged (after home substitution) ---

test("short path (after home substitution) → returned as-is", () => {
  const home = "/home/user";
  const path = "/home/user/code";
  const result = formatCwd(path, home, 48);
  assert.equal(result, "~/code");
});

test("path without home, short → returned unchanged", () => {
  const path = "/var/log/app";
  assert.equal(formatCwd(path, "/home/other", 48), "/var/log/app");
});

// --- long path → middle ellipsis preserving start and last segment ---

test("long path → contains ... and ends with last segment", () => {
  const home = "/home/user";
  // Build a path that exceeds 48 chars after ~ substitution.
  // After home → ~, result is "~/projects/company/client/2026/lare-terminal/sources"
  // which is 52 chars > 48.
  const path = "/home/user/projects/company/client/2026/lare-terminal/sources";
  const result = formatCwd(path, home, 48);
  assert.ok(result.includes("..."), `expected ellipsis in: ${result}`);
  assert.ok(result.endsWith("sources"), `expected to end with 'sources', got: ${result}`);
  assert.ok(result.length <= 48, `expected length <= 48, got: ${result.length} (${result})`);
});

test("long path without home match → contains ... and ends with last segment", () => {
  const path = "/very/long/and/deeply/nested/directory/structure/that/exceeds/the/limit/segment";
  const result = formatCwd(path, "/home/user", 48);
  assert.ok(result.includes("..."), `expected ellipsis in: ${result}`);
  assert.ok(result.endsWith("segment"), `expected to end with 'segment', got: ${result}`);
  assert.ok(result.length <= 48, `expected length <= 48, got: ${result.length} (${result})`);
});

// --- home="" → no substitution ---

test("home empty string → no ~ substitution", () => {
  const path = "/home/user/projects";
  assert.equal(formatCwd(path, ""), "/home/user/projects");
});

test("home empty string with long path → ellipsis but no ~", () => {
  const path = "/home/user/very/long/deeply/nested/directory/path/segment";
  const result = formatCwd(path, "", 48);
  assert.ok(!result.startsWith("~"), `should not start with ~, got: ${result}`);
  assert.ok(result.includes("..."), `expected ellipsis in: ${result}`);
  assert.ok(result.endsWith("segment"), `expected to end with 'segment', got: ${result}`);
});
