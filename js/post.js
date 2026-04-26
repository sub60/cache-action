// @ts-check

const core = require("./tiny-actions-core");
const fs = require("node:fs");
const { spawnSync } = require("node:child_process");
const {
  STATE_BINARY_PATH,
  STATE_DAEMON_LOG_PATH,
  STATE_SOCKET_PATH,
} = require("./state");

const socketPath = core.getState(STATE_SOCKET_PATH);
const binaryPath = core.getState(STATE_BINARY_PATH);
const daemonLogPath = core.getState(STATE_DAEMON_LOG_PATH);

// If either of these isn't set it means the `main` step failed.
if (!socketPath || !binaryPath) {
  core.info("Action state is missing, skipping post step");
  process.exit(0);
}

const { status } = spawnSync(binaryPath, ["stop", "--socket", socketPath], {
  stdio: "inherit",
});

if (status !== 0) {
  if (daemonLogPath) {
    try {
      const daemonLog = fs.readFileSync(daemonLogPath, "utf8").trimEnd();
      if (daemonLog) {
        core.info(`Daemon log:\n${daemonLog}`);
      }
    } catch (error) {
      core.info(`Couldn't read daemon log at '${daemonLogPath}': ${error}`);
    }
  }

  process.exit(status ?? 1);
}
