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

const ACTION_REPOSITORY = "sub60/cache-action";
const BINARY_NAME = "cache-action";
const IS_PRIVATE = true; // TODO: remove this once the repository is public

/**
 * @typedef {{ name: string, commit?: { sha?: string } }} GitHubTag
 * @typedef {{ name: string, url: string }} GitHubReleaseAsset
 * @typedef {{ tag_name: string, assets: GitHubReleaseAsset[] }} GitHubRelease
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
 * @param {string} commitSha
 * @param {string} [githubToken]
 * @returns {Promise<string | null>}
 */
const tagForCommit = async (commitSha, githubToken) => {
  const apiUrl = requiredEnv("GITHUB_API_URL");

  for (let page = 1; ; page += 1) {
    const response = await fetch(
      `${apiUrl}/repos/${ACTION_REPOSITORY}/tags?per_page=100&page=${page}`,
      {
        headers: githubApiHeaders(githubToken),
        redirect: "follow",
      },
    );

    if (!response.ok) {
      throw new Error(
        `Failed to list tags in '${ACTION_REPOSITORY}' while resolving commit '${commitSha}': ${response.status} ${response.statusText}`,
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
 * @param {string} actionRef
 * @param {string} [githubToken]
 * @returns {Promise<string>}
 */
const releaseTag = async (actionRef, githubToken) => {
  if (!isCommitSha(actionRef)) {
    return actionRef;
  }

  const tag = await tagForCommit(actionRef, githubToken);

  if (!tag) {
    throw new Error(
      `Action ref '${actionRef}' is a commit without a tag. This action downloads binaries from GitHub releases, which are only published for tags. Pin the action to a tag to use prebuilt artifacts`,
    );
  }

  return tag;
};

/**
 * @param {string} actionRef
 * @param {string} assetName
 * @returns {Promise<{ url: string, headers: Record<string, string> }>}
 */
const directReleaseAssetRequest = async (actionRef, assetName) => {
  const base = `https://github.com/${ACTION_REPOSITORY}/releases`;
  const tag = await releaseTag(actionRef);

  return {
    url: `${base}/download/${tag}/${assetName}`,
    headers: {},
  };
};

/**
 * @param {string} actionRef
 * @param {string} assetName
 * @param {string} [githubToken]
 * @returns {Promise<{ url: string, headers: Record<string, string> }>}
 */
const githubApiReleaseAssetRequest = async (
  actionRef,
  assetName,
  githubToken,
) => {
  const apiUrl = requiredEnv("GITHUB_API_URL");
  const tag = await releaseTag(actionRef, githubToken);
  const releaseUrl = `${apiUrl}/repos/${ACTION_REPOSITORY}/releases/tags/${encodeURIComponent(tag)}`;

  const response = await fetch(releaseUrl, {
    headers: githubApiHeaders(githubToken),
    redirect: "follow",
  });

  if (!response.ok) {
    throw new Error(
      `Failed to resolve release in '${ACTION_REPOSITORY}': ${response.status} ${response.statusText}`,
    );
  }

  const release = /** @type {GitHubRelease} */ (await response.json());
  const asset = release.assets.find(({ name }) => name === assetName);

  if (!asset) {
    throw new Error(
      `Release '${release.tag_name}' in '${ACTION_REPOSITORY}' does not contain asset '${assetName}'`,
    );
  }

  return {
    url: asset.url,
    headers: {
      accept: "application/octet-stream",
      "x-github-api-version": "2022-11-28",
      ...(githubToken ? { authorization: `Bearer ${githubToken}` } : {}),
    },
  };
};

/**
 * @param {string} url
 * @param {string} destinationPath
 * @param {Record<string, string>} [headers]
 * @returns {Promise<void>}
 */
const downloadGzipFile = async (url, destinationPath, headers = {}) => {
  const response = await fetch(url, {
    redirect: "follow",
    headers,
  });

  if (!response.ok) {
    throw new Error(
      `Failed to download '${url}': ${response.status} ${response.statusText}`,
    );
  }

  if (!response.body) {
    throw new Error(`Download response for '${url}' did not contain a body`);
  }

  const input = Readable.fromWeb(response.body);
  const output = fs.createWriteStream(destinationPath, { mode: 0o755 });
  await pipeline(input, zlib.createGunzip(), output);
  await fsp.chmod(destinationPath, 0o755);
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
  let onError = () => { };
  /** @type {(code: number | null, signal: NodeJS.Signals | null) => void} */
  let onExit = () => { };

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
  const authToken = core.getInput("auth-token", { required: true });
  const githubToken = core.getInput("github-token");
  const user = core.getInput("user", { required: true });
  const cache = core.getInput("cache", { required: true });
  const target = platformAssetTarget();
  const runnerTemp = process.env.RUNNER_TEMP || os.tmpdir();
  const daemonDir = await fsp.mkdtemp(
    path.join(runnerTemp, "sub60-cache-action-"),
  );
  const socketPath = path.join(daemonDir, "daemon.sock");
  const binaryPath = path.join(daemonDir, BINARY_NAME);
  const hookPath = path.join(daemonDir, "post-build-hook.sh");
  const configPath = path.join(daemonDir, "nix.conf");
  const daemonLogPath = path.join(daemonDir, "daemon.log");
  const assetName = `${BINARY_NAME}-${target}.gz`;
  const actionRef = requiredEnv("GITHUB_ACTION_REF");

  if (IS_PRIVATE && !githubToken) {
    throw new Error(
      "github-token is required to download release assets from the private cache-action repository",
    );
  }

  const assetRequest = (IS_PRIVATE || isCommitSha(actionRef))
    ? await githubApiReleaseAssetRequest(actionRef, assetName, githubToken)
    : await directReleaseAssetRequest(actionRef, assetName);

  core.info(`Downloading '${assetName}' from '${assetRequest.url}'`);
  await downloadGzipFile(
    assetRequest.url,
    binaryPath,
    assetRequest.headers,
  );

  core.saveState(STATE_SOCKET_PATH, socketPath);
  core.saveState(STATE_BINARY_PATH, binaryPath);
  core.saveState(STATE_DAEMON_LOG_PATH, daemonLogPath);

  await writeHookScript(hookPath, binaryPath, socketPath);
  await writeNixConfig(configPath, hookPath);
  core.exportVariable("NIX_USER_CONF_FILES", mergedUserConfFiles(configPath));

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

  await waitForDaemonReady(child);

  core.info(
    `Started daemon with process ID ${child.pid}, listening on ${socketPath}`,
  );

  child.unref();
};

main().catch((error) => {
  core.setFailed(error.stack || error.message);
});
