"use strict";
const { spawnSync } = require("node:child_process");
const { appendFileSync } = require("node:fs");

// Match hearken-update's manifest channel rules. This helper selects source;
// the packaged game still accepts only signed manifests for its channel.
const isChannel = value => typeof value === "string" && /^[a-z0-9-]{1,32}$/.test(value);
function validateBranch(branch) {
  if (typeof branch !== "string" || !branch || branch === "HEAD" || branch === "@" ||
      branch.startsWith("-") || branch.startsWith("refs/")) {
    throw new Error("Use a branch name, not a tag, commit expression, or full ref.");
  }
  // Full-ref validation avoids --branch's expansion of expressions like @{-1}.
  // Arguments never pass through a shell. Git needs no checkout for this check.
  const check = spawnSync("git", ["check-ref-format", `refs/heads/${branch}`], { encoding: "utf8" });
  if (check.error) throw new Error("Git is required to validate the source branch.");
  if (check.status !== 0) throw new Error("The source branch is not a valid Git branch name.");
  return branch;
}
function resolveChannelBranch(channel, branches, override = "") {
  if (!isChannel(channel)) throw new Error("Invalid update channel; use 1-32 lowercase letters, digits, or hyphens.");
  if (!branches || typeof branches !== "object" || Array.isArray(branches)) {
    throw new Error("channel-branches must be a JSON object of channel to branch.");
  }
  for (const [name, branch] of Object.entries(branches)) {
    if (!isChannel(name)) throw new Error("The branch map contains an invalid channel.");
    validateBranch(branch);
  }
  if (typeof override !== "string") throw new Error("The branch override must be a string.");
  if (override) return validateBranch(override);
  if (!Object.hasOwn(branches, channel)) throw new Error(`No source branch is configured for channel ${channel}.`);
  return branches[channel];
}
module.exports = { resolveChannelBranch };
if (require.main === module) {
  try {
    const branches = JSON.parse(process.env.UPDATE_SOURCE_BRANCHES || "{}");
    const branch = resolveChannelBranch(process.env.UPDATE_SOURCE_CHANNEL, branches,
      process.env.UPDATE_SOURCE_OVERRIDE || "");
    if (!process.env.GITHUB_OUTPUT) throw new Error("GITHUB_OUTPUT is missing.");
    appendFileSync(process.env.GITHUB_OUTPUT, `branch=${branch}\n`, "utf8");
  } catch (error) {
    console.error(`Update source: ${error.message}`);
    process.exitCode = 1;
  }
}
