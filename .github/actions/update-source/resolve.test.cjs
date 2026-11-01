"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const { mkdtempSync, readFileSync, rmSync, existsSync } = require("node:fs");
const { tmpdir } = require("node:os");
const { join } = require("node:path");
const { spawnSync } = require("node:child_process");
const { resolveChannelBranch: resolve } = require("./resolve.cjs");
const branches = { stable: "main", beta: "dev", alpha: "career/2.0" };

test("maps each channel independently", () => {
  for (const [channel, branch] of Object.entries(branches)) assert.equal(resolve(channel, branches), branch);
});
test("an explicit branch overrides only this invocation", () => {
  assert.equal(resolve("alpha", branches, "feat/career-2.0"), "feat/career-2.0");
  assert.equal(resolve("alpha", branches), "career/2.0");
});
test("missing channels fail instead of using another channel's branch", () => {
  assert.throws(() => resolve("alpha", { beta: "dev" }), /No source branch/);
  assert.throws(() => resolve("constructor", {}), /No source branch/);
  assert.equal(resolve("alpha", {}, "feature/test"), "feature/test");
});
test("rejects malformed maps and channels", () => {
  for (const map of [null, [], "dev", { alpha: 2 }, { Alpha: "dev" }]) assert.throws(() => resolve("alpha", map));
  for (const channel of ["", "Alpha", "a/b", "a".repeat(33), "alpha\nbranch=main"]) assert.throws(() => resolve(channel, branches));
});
test("rejects ref expressions, output injection, and option-like names", () => {
  for (const branch of ["HEAD", "@", "@{-1}", "dev~1", "refs/heads/dev", "-f", "../dev", "dev.lock", "bad branch", "dev\nbranch=main", "a:b", "a//b", "a\\b"]) {
    assert.throws(() => resolve("alpha", branches, branch), undefined, branch);
  }
});
test("writes exactly one output and never checks out or publishes", () => {
  const dir = mkdtempSync(join(tmpdir(), "hearken-update-source-"));
  try {
    const output = join(dir, "output");
    const run = override => spawnSync(process.execPath, [join(__dirname, "resolve.cjs")], {
      cwd: dir, encoding: "utf8", env: { ...process.env, GITHUB_OUTPUT: output,
        UPDATE_SOURCE_CHANNEL: "alpha", UPDATE_SOURCE_BRANCHES: JSON.stringify(branches), UPDATE_SOURCE_OVERRIDE: override },
    });
    assert.equal(run("").status, 0);
    assert.equal(readFileSync(output, "utf8"), "branch=career/2.0\n");
    assert.equal(existsSync(join(dir, ".git")), false);
    assert.equal(run("dev\nbranch=main").status, 1);
    assert.equal(readFileSync(output, "utf8"), "branch=career/2.0\n");
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
