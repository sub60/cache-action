// @ts-check

const core = require("@actions/core");
const fs = require("node:fs");
const fsp = require("node:fs/promises");
const { spawn } = require("node:child_process");
const { once } = require("node:events");
const os = require("node:os");
const path = require("node:path");
const { Readable } = require("node:stream");
const { finished } = require("node:stream/promises");
const { setTimeout: delay } = require("node:timers/promises");
const { STATE_SOCKET_PATH, STATE_BINARY_PATH } = require("./state");

const binaryName = "cache-action";

/**
 * @returns {string}
 */
const platformTarget = () => {
  switch (`${process.platform}/${os.arch()}`) {
    case "linux/x64":
      return "x86_64-unknown-linux-musl";
    case "linux/arm64":
      return "aarch64-unknown-linux-musl";
    case "darwin/x64":
      return "x86_64-apple-darwin";
    case "darwin/arm64":
      return "aarch64-apple-darwin";
    default:
      throw new Error(
        `Unsupported runner platform '${process.platform}' and architecture '${os.arch()}'.`,
      );
  }
};

/**
 * @param {string} ref
 * @returns {boolean}
 */
const isAssetRef = (ref) => {
  return (
    /^v?\d+(?:\.\d+)*(?:[-+._][A-Za-z0-9._-]+)?$/.test(ref) ||
    /^[0-9a-f]{7,40}$/i.test(ref)
  );
};

/**
 * @param {string} assetName
 * @returns {string}
 */
const releaseUrl = (assetName) => {
  const actionRepository =
    process.env.GITHUB_ACTION_REPOSITORY || "sub60/cache-action";
  const actionRef = (process.env.GITHUB_ACTION_REF || "").trim();

  if (!actionRef) {
    throw new Error("GITHUB_ACTION_REF is not set.");
  }

  if (!isAssetRef(actionRef)) {
    throw new Error(
      `Action ref '${actionRef}' is not a release tag or commit SHA. Pin the action to a ref that has matching binary assets.`,
    );
  }

  return `https://github.com/${actionRepository}/releases/download/${/^[0-9a-f]{7,40}$/i.test(actionRef) ? `commit-${actionRef}` : actionRef}/${assetName}`;
};

/**
 * @param {string} url
 * @param {string} destinationPath
 * @returns {Promise<void>}
 */
const downloadFile = async (url, destinationPath) => {
  const response = await fetch(url, {
    redirect: "follow",
    headers: {
      "user-agent": "sub60-cache-action",
    },
  });

  if (!response.ok) {
    throw new Error(
      `Failed to download '${url}': ${response.status} ${response.statusText}`,
    );
  }

  if (!response.body) {
    throw new Error(`Download response for '${url}' did not contain a body.`);
  }

  const output = fs.createWriteStream(destinationPath, { mode: 0o755 });
  Readable.fromWeb(response.body).pipe(output);
  await finished(output);
  await fsp.chmod(destinationPath, 0o755);
};

/**
 * @param {string} socketPath
 * @param {number} timeoutMs
 * @returns {Promise<void>}
 */
const waitForSocket = async (socketPath, timeoutMs) => {
  const deadline = Date.now() + timeoutMs;

  while (Date.now() < deadline) {
    try {
      const stats = await fsp.lstat(socketPath);
      if (stats.isSocket()) {
        return;
      }
    } catch (error) {
      if (error.code !== "ENOENT") {
        throw error;
      }
    }

    await delay(100);
  }

  throw new Error(`Timed out waiting for daemon socket '${socketPath}'.`);
};

/**
 * @param {import("node:child_process").ChildProcess} child
 * @param {string} description
 * @returns {Promise<void>}
 */
const ensureChildStarted = async (child, description) => {
  await Promise.race([
    once(child, "spawn").then(() => undefined),
    once(child, "error").then(([error]) => {
      throw new Error(`${description} failed to start: ${error.message}`);
    }),
    once(child, "exit").then(([code, signal]) => {
      throw new Error(
        `${description} exited before becoming ready with code ${code} and signal ${signal}.`,
      );
    }),
  ]);
};

/**
 * @param {string} hookPath
 * @param {string} binaryPath
 * @param {string} socketPath
 * @returns {Promise<void>}
 */
const writeHookScript = async (hookPath, binaryPath, socketPath) => {
  const script = [
    "#!/bin/sh",
    "set -eu",
    "set -f",
    "export IFS=' '",
    `exec "${binaryPath}" push --socket "${socketPath}" $OUT_PATHS`,
    "",
  ].join("\n");

  await fsp.writeFile(hookPath, script, { mode: 0o755 });
  await fsp.chmod(hookPath, 0o755);
};

/**
 * @param {string} configPath
 * @param {string} hookPath
 * @returns {Promise<void>}
 */
const writeNixConfig = async (configPath, hookPath) => {
  const config = `post-build-hook = ${hookPath}\n`;
  await fsp.writeFile(configPath, config, { mode: 0o644 });
};

/**
 * @param {string} configPath
 * @returns {string}
 */
const mergedUserConfFiles = (configPath) => {
  const existing = process.env.NIX_USER_CONF_FILES;

  if (!existing || !existing.trim()) {
    return configPath;
  }

  return `${configPath}${path.delimiter}${existing}`;
};

/**
 * @returns {Promise<void>}
 */
const main = async () => {
  const authToken = core.getInput("auth-token", { required: true });
  const target = platformTarget();
  const runnerTemp = process.env.RUNNER_TEMP || os.tmpdir();
  const daemonDir = await fsp.mkdtemp(path.join(runnerTemp, "sub60-cache-"));
  const socketPath = path.join(daemonDir, "daemon.sock");
  const binaryPath = path.join(daemonDir, binaryName);
  const hookPath = path.join(daemonDir, "post-build-hook.sh");
  const configPath = path.join(daemonDir, "nix.conf");
  const assetName = `${binaryName}-${target}`;
  const url = releaseUrl(assetName);

  core.info(`Downloading daemon '${assetName}' from '${url}'.`);
  await downloadFile(url, binaryPath);

  core.saveState(STATE_SOCKET_PATH, socketPath);
  core.saveState(STATE_BINARY_PATH, binaryPath);

  await writeHookScript(hookPath, binaryPath, socketPath);
  await writeNixConfig(configPath, hookPath);
  core.exportVariable("NIX_USER_CONF_FILES", mergedUserConfFiles(configPath));

  const child = spawn(
    binaryPath,
    ["start", "--socket", socketPath, "--auth-token", authToken],
    {
      detached: true,
      stdio: ["ignore", "inherit", "inherit"],
    },
  );

  await ensureChildStarted(child, "cache daemon");

  child.unref();

  core.info(
    `Started daemon with pid ${child.pid}. Waiting for socket '${socketPath}'.`,
  );
  await waitForSocket(socketPath, 5000);
  core.info("Daemon is ready.");
};

main().catch((error) => {
  core.setFailed(error.stack || error.message);
});
