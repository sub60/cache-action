// @ts-check

const core = require("./tiny-actions-core");
const { spawnSync } = require("node:child_process");
const { STATE_SOCKET_PATH, STATE_BINARY_PATH } = require("./state");

const socketPath = core.getState(STATE_SOCKET_PATH);
const binaryPath = core.getState(STATE_BINARY_PATH);

if (!socketPath || !binaryPath) {
  core.info("No saved daemon state found; skipping shutdown.");
  process.exit(0);
}

core.info(`Stopping daemon via socket '${socketPath}'`);

const { status } = spawnSync(binaryPath, ["drain", "--socket", socketPath], {
  stdio: "inherit",
});

if (status !== 0) {
  process.exit(status);
}
