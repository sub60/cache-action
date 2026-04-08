// @ts-check

const core = require("./tiny-actions-core");
const { spawnSync } = require("node:child_process");
const { STATE_SOCKET_PATH, STATE_BINARY_PATH } = require("./state");

const socketPath = core.getState(STATE_SOCKET_PATH);
const binaryPath = core.getState(STATE_BINARY_PATH);

// If either of these isn't set it means the `main` step failed.
if (!socketPath || !binaryPath) {
  core.info("Action state is missing, skipping post step");
  process.exit(0);
}

const { status } = spawnSync(binaryPath, ["drain", "--socket", socketPath], {
  stdio: "inherit",
});

if (status !== 0) {
  process.exit(status);
}
