// @ts-check

const core = require("./tiny-actions-core");
const { spawnSync } = require("node:child_process");
const {
  printDaemonLog,
  STATE_BINARY_PATH,
  STATE_DAEMON_LOG_PATH,
  STATE_SOCKET_PATH,
} = require("./common");

const post = async () => {
  const socketPath = core.getState(STATE_SOCKET_PATH);
  const binaryPath = core.getState(STATE_BINARY_PATH);
  const daemonLogPath = core.getState(STATE_DAEMON_LOG_PATH);

  const { status } = spawnSync(binaryPath, ["stop", "--socket", socketPath], {
    stdio: "inherit",
  });

  if (status !== 0 && daemonLogPath) {
    await printDaemonLog(daemonLogPath);
  }

  if (status !== 0) {
    process.exitCode = status ?? 1;
  }
};

post().catch((error) => {
  core.setFailed(error.stack || error.message);
});
