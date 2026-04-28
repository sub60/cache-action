// @ts-check

const core = require("./tiny-actions-core");
const fs = require("node:fs");
const fsp = require("node:fs/promises");
const { spawn } = require("node:child_process");
const os = require("node:os");
const path = require("node:path");
const { Readable } = require("node:stream");
const { pipeline } = require("node:stream/promises");
const zlib = require("node:zlib");
const {
  STATE_BINARY_PATH,
  STATE_DAEMON_LOG_PATH,
  STATE_SOCKET_PATH,
} = require("./state");

const binaryName = "cache-action";
// Remove this assignment once the release asset repository is public.
const IS_PRIVATE = true;

const now = () => performance.now();

/**
 * @param {number} startedAt
 * @returns {number}
 */
const elapsedMs = (startedAt) => now() - startedAt;

/**
 * @param {number} ms
 * @returns {string}
 */
const formatMs = (ms) => `${Math.round(ms)}ms`;

/**
 * @param {number} bytes
 * @returns {string}
 */
const formatBytes = (bytes) => {
  if (bytes < 1024) {
    return `${bytes} B`;
  }

  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KiB`;
  }

  return `${(bytes / 1024 / 1024).toFixed(1)} MiB`;
};

/**
 * @param {string} value
 * @returns {boolean}
 */
const inputBool = (value) => {
  return ["1", "true", "yes", "on"].includes(value.trim().toLowerCase());
};

class Timings {
  /**
   * @param {boolean} enabled
   */
  constructor(enabled) {
    this.enabled = enabled;
    /** @type {{ label: string, ms: number, detail?: string }[]} */
    this.entries = [];
  }

  /**
   * @param {string} label
   * @param {number} ms
   * @param {string} [detail]
   */
  add(label, ms, detail) {
    this.entries.push({ label, ms, detail });

    if (this.enabled) {
      core.info(
        `cache-action timing: ${label}: ${formatMs(ms)}${detail ? ` (${detail})` : ""}`,
      );
    }
  }

  /**
   * @param {number} totalMs
   */
  summary(totalMs) {
    if (!this.enabled) {
      return;
    }

    const entries = this.entries
      .map(({ label, ms }) => `${label} ${formatMs(ms)}`)
      .join(", ");
    core.info(`cache-action timing: total ${formatMs(totalMs)} (${entries})`);
  }
}

/**
 * @typedef {{ name: string, commit?: { sha?: string } }} GitHubTag
 * @typedef {{ name: string, url: string }} GitHubReleaseAsset
 * @typedef {{ tag_name: string, assets: GitHubReleaseAsset[] }} GitHubRelease
 * @typedef {{ url: string, headers: Record<string, string>, source: string }} AssetRequest
 */

/**
 * @param {string} name
 * @returns {string}
 */
const requiredEnv = (name) => {
  const value = process.env[name]?.trim();

  if (!value) {
    throw new Error(`${name} is not set`);
  }

  return value;
};

/**
 * @returns {string}
 */
const platformAssetTarget = () => {
  const archByNodeArch = /** @type {Record<string, string>} */ ({
    x64: "x86_64",
    arm64: "aarch64",
  });

  const arch = archByNodeArch[os.arch()];

  if (!arch || !["linux", "darwin"].includes(process.platform)) {
    throw new Error(
      `Unsupported runner platform '${process.platform}' and architecture '${os.arch()}'`,
    );
  }

  return `${arch}-${process.platform}`;
};

/**
 * @param {string} ref
 * @returns {boolean}
 */
const isCommitSha = (ref) => {
  return /^[0-9a-f]{7,40}$/i.test(ref);
};

/**
 * @param {string} [githubToken]
 * @returns {Record<string, string>}
 */
const githubApiHeaders = (githubToken) => {
  const headers = /** @type {Record<string, string>} */ ({
    accept: "application/vnd.github+json",
    "x-github-api-version": "2022-11-28",
  });

  if (githubToken) {
    headers.authorization = `Bearer ${githubToken}`;
  }

  return headers;
};

/**
 * @param {string} [githubToken]
 * @returns {Record<string, string>}
 */
const githubAssetDownloadHeaders = (githubToken) => {
  const headers = /** @type {Record<string, string>} */ ({
    accept: "application/octet-stream",
    "x-github-api-version": "2022-11-28",
  });

  if (githubToken) {
    headers.authorization = `Bearer ${githubToken}`;
  }

  return headers;
};

/**
 * @param {string} actionRepository
 * @param {string} commitSha
 * @param {string} [githubToken]
 * @returns {Promise<string | null>}
 */
const tagForCommit = async (actionRepository, commitSha, githubToken) => {
  const apiUrl = requiredEnv("GITHUB_API_URL");

  for (let page = 1; ; page += 1) {
    const response = await fetch(
      `${apiUrl}/repos/${actionRepository}/tags?per_page=100&page=${page}`,
      {
        headers: githubApiHeaders(githubToken),
        redirect: "follow",
      },
    );

    if (!response.ok) {
      throw new Error(
        `Failed to list tags in '${actionRepository}' while resolving commit '${commitSha}': ${response.status} ${response.statusText}`,
      );
    }

    const tags = /** @type {GitHubTag[]} */ (await response.json());

    if (tags.length === 0) {
      return null;
    }

    const match = tags.find(({ commit }) => commit?.sha?.startsWith(commitSha));

    if (match) {
      return match.name;
    }
  }
};

/**
 * @param {string} actionRepository
 * @param {string} actionRef
 * @param {string} [githubToken]
 * @returns {Promise<string>}
 */
const releaseTag = async (actionRepository, actionRef, githubToken) => {
  if (!isCommitSha(actionRef)) {
    return actionRef;
  }

  const tag = await tagForCommit(actionRepository, actionRef, githubToken);

  if (!tag) {
    throw new Error(
      `Action ref '${actionRef}' is a commit without a tag. This action downloads binaries from GitHub releases, which are only published for tags. Pin the action to a tag to use prebuilt artifacts`,
    );
  }

  return tag;
};

/**
 * @param {string} actionRepository
 * @param {string | undefined} actionRef
 * @param {string} assetName
 * @returns {Promise<AssetRequest>}
 */
const directReleaseAssetRequest = async (
  actionRepository,
  actionRef,
  assetName,
) => {
  const base = `https://github.com/${actionRepository}/releases`;
  const tag = actionRef ? await releaseTag(actionRepository, actionRef) : null;

  return {
    url: tag
      ? `${base}/download/${tag}/${assetName}`
      : `${base}/latest/download/${assetName}`,
    headers: {},
    source: tag ? "direct release URL" : "direct latest release URL",
  };
};

/**
 * @param {string} actionRepository
 * @param {string | undefined} actionRef
 * @param {string} assetName
 * @param {string} [githubToken]
 * @returns {Promise<AssetRequest>}
 */
const githubApiReleaseAssetRequest = async (
  actionRepository,
  actionRef,
  assetName,
  githubToken,
) => {
  const apiUrl = requiredEnv("GITHUB_API_URL");
  const tag = actionRef
    ? await releaseTag(actionRepository, actionRef, githubToken)
    : null;
  const releaseUrl = tag
    ? `${apiUrl}/repos/${actionRepository}/releases/tags/${encodeURIComponent(tag)}`
    : `${apiUrl}/repos/${actionRepository}/releases/latest`;

  const response = await fetch(releaseUrl, {
    headers: githubApiHeaders(githubToken),
    redirect: "follow",
  });

  if (!response.ok) {
    throw new Error(
      `Failed to resolve release in '${actionRepository}': ${response.status} ${response.statusText}`,
    );
  }

  const release = /** @type {GitHubRelease} */ (await response.json());
  const asset = release.assets.find(({ name }) => name === assetName);

  if (!asset) {
    throw new Error(
      `Release '${release.tag_name}' in '${actionRepository}' does not contain asset '${assetName}'`,
    );
  }

  return {
    url: asset.url,
    headers: githubAssetDownloadHeaders(githubToken),
    source: "GitHub API",
  };
};

/**
 * @param {string | undefined} actionRef
 * @returns {boolean}
 */
const shouldUseGitHubApiAsset = (actionRef) => {
  return IS_PRIVATE || Boolean(actionRef && isCommitSha(actionRef));
};

/**
 * @param {string} url
 * @param {string} destinationPath
 * @param {Record<string, string>} [headers]
 * @returns {Promise<{
 *   compressedBytes: number,
 *   outputBytes: number,
 *   contentLength: string | null,
 *   fetchMs: number,
 *   writeMs: number,
 * }>}
 */
const downloadGzipFile = async (url, destinationPath, headers = {}) => {
  const fetchStartedAt = now();
  const response = await fetch(url, {
    redirect: "follow",
    headers,
  });
  const fetchMs = elapsedMs(fetchStartedAt);

  if (!response.ok) {
    throw new Error(
      `Failed to download '${url}': ${response.status} ${response.statusText}`,
    );
  }

  if (!response.body) {
    throw new Error(`Download response for '${url}' did not contain a body`);
  }

  let compressedBytes = 0;
  const input = Readable.fromWeb(response.body);
  input.on("data", (chunk) => {
    compressedBytes += chunk.length;
  });

  const output = fs.createWriteStream(destinationPath, { mode: 0o755 });
  const writeStartedAt = now();
  await pipeline(input, zlib.createGunzip(), output);
  await fsp.chmod(destinationPath, 0o755);

  return {
    compressedBytes,
    outputBytes: output.bytesWritten,
    contentLength: response.headers.get("content-length"),
    fetchMs,
    writeMs: elapsedMs(writeStartedAt),
  };
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
  const nix_user_conf_files = process.env.NIX_USER_CONF_FILES;

  if (nix_user_conf_files && nix_user_conf_files.trim()) {
    return `${configPath}:${nix_user_conf_files}`;
  }

  // Setting $NIX_USER_CONF_FILES disables Nix's default XDG config lookup, so
  // we must include those paths explicitly.

  const xdgConfigHome =
    process.env.XDG_CONFIG_HOME || path.join(os.homedir(), ".config");

  const xdgConfigDirs = (process.env.XDG_CONFIG_DIRS || "/etc/xdg").split(":");

  const defaultConfFiles = [xdgConfigHome, ...xdgConfigDirs].map(
    (dir) => `${dir}/nix/nix.conf`,
  );

  return [configPath, ...defaultConfFiles].join(":");
};

/**
 * @param {import("node:stream").Readable} readyStream
 * @returns {Promise<void>}
 */
const readReady = async (readyStream) => {
  const chunks = [];

  for await (const chunk of readyStream) {
    chunks.push(Buffer.from(chunk));
  }

  const message = Buffer.concat(chunks);

  if (message.length === 0) {
    throw new Error("daemon exited before reporting readiness");
  }

  if (message[0] === 0) {
    return;
  }

  if (message[0] === 1) {
    throw new Error(
      message.subarray(1).toString("utf8") || "daemon startup failed",
    );
  }

  throw new Error(`daemon reported unknown startup status ${message[0]}`);
};

/**
 * @param {import("node:child_process").ChildProcess} child
 * @returns {Promise<void>}
 */
const waitForDaemonReady = async (child) => {
  const readyStream = child.stdio[3];

  if (!(readyStream instanceof Readable)) {
    throw new Error("daemon readiness pipe was not created");
  }

  const readableReadyStream = /** @type {import("node:stream").Readable} */ (
    readyStream
  );

  /** @type {(error: Error) => void} */
  let onError = () => {};
  /** @type {(code: number | null, signal: NodeJS.Signals | null) => void} */
  let onExit = () => {};

  const exitedEarly = new Promise((_, reject) => {
    onError = reject;
    onExit = (code, signal) => {
      reject(
        new Error(
          `daemon exited before readiness: code=${code}, signal=${signal}`,
        ),
      );
    };

    child.once("error", onError);
    child.once("exit", onExit);
  });

  try {
    await Promise.race([readReady(readableReadyStream), exitedEarly]);
  } finally {
    child.off("error", onError);
    child.off("exit", onExit);
  }
};

/**
 * @returns {Promise<void>}
 */
const main = async () => {
  const totalStartedAt = now();
  const authToken = core.getInput("auth-token", { required: true });
  const debugTiming = inputBool(core.getInput("debug-timing"));
  const githubToken = core.getInput("github-token");
  const user = core.getInput("user", { required: true });
  const cache = core.getInput("cache", { required: true });
  const timings = new Timings(debugTiming);
  const target = platformAssetTarget();
  const runnerTemp = process.env.RUNNER_TEMP || os.tmpdir();
  const tempDirStartedAt = now();
  const daemonDir = await fsp.mkdtemp(
    path.join(runnerTemp, "sub60-cache-action-"),
  );
  timings.add("create temp dir", elapsedMs(tempDirStartedAt));
  const socketPath = path.join(daemonDir, "daemon.sock");
  const binaryPath = path.join(daemonDir, binaryName);
  const hookPath = path.join(daemonDir, "post-build-hook.sh");
  const configPath = path.join(daemonDir, "nix.conf");
  const daemonLogPath = path.join(daemonDir, "daemon.log");
  const assetName = `${binaryName}-${target}.gz`;
  const actionRepository =
    process.env.GITHUB_ACTION_REPOSITORY?.trim() || "sub60/cache-action";
  const actionRef = process.env.GITHUB_ACTION_REF?.trim();

  const releaseStartedAt = now();
  const useGitHubApiAsset = shouldUseGitHubApiAsset(actionRef);
  if (IS_PRIVATE && !githubToken) {
    throw new Error(
      "github-token is required to download release assets from the private cache-action repository",
    );
  }

  const assetRequest = useGitHubApiAsset
    ? await githubApiReleaseAssetRequest(
        actionRepository,
        actionRef,
        assetName,
        githubToken,
      )
    : await directReleaseAssetRequest(actionRepository, actionRef, assetName);
  timings.add(
    "resolve release asset",
    elapsedMs(releaseStartedAt),
    assetRequest.source,
  );

  const downloadStartedAt = now();
  core.info(`Downloading '${assetName}' from '${assetRequest.url}'`);
  const download = await downloadGzipFile(
    assetRequest.url,
    binaryPath,
    assetRequest.headers,
  );
  const contentLength = download.contentLength
    ? `${download.contentLength} B content-length`
    : "no content-length";
  timings.add(
    "download and decompress binary",
    elapsedMs(downloadStartedAt),
    `${formatBytes(download.compressedBytes)} compressed, ${formatBytes(download.outputBytes)} decompressed, ${contentLength}, source ${assetRequest.source}, headers ${formatMs(download.fetchMs)}, body+gunzip ${formatMs(download.writeMs)}`,
  );

  core.saveState(STATE_SOCKET_PATH, socketPath);
  core.saveState(STATE_BINARY_PATH, binaryPath);
  core.saveState(STATE_DAEMON_LOG_PATH, daemonLogPath);

  const configStartedAt = now();
  await writeHookScript(hookPath, binaryPath, socketPath);
  await writeNixConfig(configPath, hookPath);
  core.exportVariable("NIX_USER_CONF_FILES", mergedUserConfFiles(configPath));
  timings.add("write action config", elapsedMs(configStartedAt));

  const spawnStartedAt = now();
  const daemonLogFd = fs.openSync(daemonLogPath, "a");
  let child;
  try {
    child = spawn(
      binaryPath,
      [
        "run",
        "--socket",
        socketPath,
        "--ready-fd",
        "3",
        "--auth-token",
        authToken,
        "--user",
        user,
        "--cache",
        cache,
      ],
      {
        detached: true,
        stdio: ["ignore", daemonLogFd, daemonLogFd, "pipe"],
      },
    );
  } finally {
    fs.closeSync(daemonLogFd);
  }
  timings.add("spawn daemon", elapsedMs(spawnStartedAt));

  const readinessStartedAt = now();
  await waitForDaemonReady(child);
  timings.add("wait for daemon readiness", elapsedMs(readinessStartedAt));

  const startupMessageStartedAt = now();
  core.info(
    `Started daemon with process ID ${child.pid}, listening on ${socketPath}`,
  );
  timings.add("print startup message", elapsedMs(startupMessageStartedAt));

  child.unref();
  timings.summary(elapsedMs(totalStartedAt));
};

main().catch((error) => {
  core.setFailed(error.stack || error.message);
});
