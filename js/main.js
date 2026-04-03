// @ts-check

const fs = require("node:fs");
const fsp = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");
const { Readable } = require("node:stream");
const { spawn } = require("node:child_process");

const STATE_SOCKET_PATH = "socket_path";
const STATE_BINARY_PATH = "binary_path";
const STATE_DAEMON_DIR = "daemon_dir";
const STATE_HOOK_PATH = "hook_path";
const STATE_CONFIG_PATH = "config_path";

/**
 * @param {string} name
 * @returns {string}
 */
const getRequiredInput = (name) => {
  const envName = `INPUT_${name.replace(/ /g, "_").toUpperCase()}`;
  const value = process.env[envName];

  if (!value) {
    throw new Error(`Missing required input '${name}'.`);
  }

  return value;
};

/**
 * @param {string | undefined} filePath
 * @param {string} line
 * @returns {void}
 */
const appendFileLine = (filePath, line) => {
  if (!filePath) {
    throw new Error("Expected GitHub Actions state file path in GITHUB_STATE.");
  }

  fs.appendFileSync(filePath, `${line}${os.EOL}`);
};

/**
 * @param {string} name
 * @param {string} value
 * @returns {void}
 */
const saveState = (name, value) => {
  appendFileLine(process.env.GITHUB_STATE, `${name}=${value}`);
};

/**
 * @param {string} name
 * @param {string} value
 * @returns {void}
 */
const exportEnv = (name, value) => {
  const envFile = process.env.GITHUB_ENV;

  if (!envFile) {
    throw new Error("Expected GitHub Actions env file path in GITHUB_ENV.");
  }

  const delimiter = `SUB60_EOF_${Date.now()}_${Math.random().toString(16).slice(2)}`;
  appendFileLine(envFile, `${name}<<${delimiter}`);
  appendFileLine(envFile, value);
  appendFileLine(envFile, delimiter);
};

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
const isReleaseLikeRef = (ref) => {
  return /^v?\d+(?:\.\d+)*(?:[-+._][A-Za-z0-9._-]+)?$/.test(ref);
};

/**
 * @param {string} assetName
 * @returns {string}
 */
const releaseUrl = (assetName) => {
  const actionRepository =
    process.env.GITHUB_ACTION_REPOSITORY || "sub60/cache-action";
  const actionRef = (process.env.GITHUB_ACTION_REF || "").trim();

  if (actionRef && isReleaseLikeRef(actionRef)) {
    return `https://github.com/${actionRepository}/releases/download/${actionRef}/${assetName}`;
  }

  return `https://github.com/${actionRepository}/releases/latest/download/${assetName}`;
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

  await new Promise((resolve, reject) => {
    Readable.fromWeb(response.body).pipe(output);
    output.on("finish", resolve);
    output.on("error", reject);
  });

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

    await new Promise((resolve) => setTimeout(resolve, 100));
  }

  throw new Error(`Timed out waiting for daemon socket '${socketPath}'.`);
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
  const authToken = getRequiredInput("auth-token");
  const target = platformTarget();
  const runnerTemp = process.env.RUNNER_TEMP || os.tmpdir();
  const daemonDir = await fsp.mkdtemp(path.join(runnerTemp, "sub60-cache-"));
  const socketPath = path.join(daemonDir, "daemon.sock");
  const logPath = path.join(daemonDir, "daemon.log");
  const binaryPath = path.join(daemonDir, "sub60-cache-daemon");
  const hookPath = path.join(daemonDir, "post-build-hook.sh");
  const configPath = path.join(daemonDir, "nix.conf");
  const assetName = `sub60-cache-daemon-${target}`;
  const url = releaseUrl(assetName);

  console.log(`Downloading daemon '${assetName}' from '${url}'.`);
  await downloadFile(url, binaryPath);

  saveState(STATE_SOCKET_PATH, socketPath);
  saveState(STATE_BINARY_PATH, binaryPath);
  saveState(STATE_DAEMON_DIR, daemonDir);
  saveState(STATE_HOOK_PATH, hookPath);
  saveState(STATE_CONFIG_PATH, configPath);

  await writeHookScript(hookPath, binaryPath, socketPath);
  await writeNixConfig(configPath, hookPath);
  exportEnv("NIX_USER_CONF_FILES", mergedUserConfFiles(configPath));

  const logHandle = fs.openSync(logPath, "a");
  const child = spawn(
    binaryPath,
    ["start", "--socket", socketPath, "--auth-token", authToken],
    {
      detached: true,
      stdio: ["ignore", logHandle, logHandle],
      env: process.env,
    },
  );

  child.unref();
  fs.closeSync(logHandle);

  console.log(
    `Started daemon with pid ${child.pid}. Waiting for socket '${socketPath}'.`,
  );
  await waitForSocket(socketPath, 5000);
  console.log("Daemon is ready.");
};

main().catch((error) => {
  console.error(error.stack || error.message);
  process.exitCode = 1;
});
