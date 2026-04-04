// @ts-check

const core = require("./tiny-actions-core");
const { spawn } = require("node:child_process");
const { once } = require("node:events");
const { STATE_SOCKET_PATH, STATE_BINARY_PATH } = require("./state");

/**
 * @param {string} binaryPath
 * @param {string} socketPath
 * @returns {Promise<void>}
 */
const runDrain = async (binaryPath, socketPath) => {
  const child = spawn(binaryPath, ["drain", "--socket", socketPath], {
    stdio: "inherit",
  });

  const [code, signal] = await Promise.race([
    once(child, "error").then(([error]) => {
      throw error;
    }),
    once(child, "exit"),
  ]);

  if (code === 0) {
    return;
  }

  if (signal) {
    throw new Error(`Daemon drain exited via signal '${signal}'`);
  }

  throw new Error(`Daemon drain exited with code ${code}`);
};

/**
 * @returns {Promise<void>}
 */
const main = async () => {
  const socketPath = core.getState(STATE_SOCKET_PATH);
  const binaryPath = core.getState(STATE_BINARY_PATH);

  if (!socketPath || !binaryPath) {
    core.info("No saved daemon state found; skipping shutdown.");
    return;
  }

  core.info(`Stopping daemon via socket '${socketPath}'`);
  await runDrain(binaryPath, socketPath);
};

main().catch((error) => {
  core.setFailed(error.stack || error.message);
});
