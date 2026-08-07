// @ts-check

const core = require("./tiny-actions-core");
const fsp = require("node:fs/promises");

const STATE_SOCKET_PATH = "socket_path";
const STATE_BINARY_PATH = "binary_path";
const STATE_DAEMON_LOG_PATH = "daemon_log_path";

/**
 * @param {string} daemonLogPath
 * @returns {Promise<void>}
 */
const printDaemonLog = async (daemonLogPath) => {
  try {
    const daemonLog = (await fsp.readFile(daemonLogPath, "utf8")).trimEnd();
    if (daemonLog) {
      core.info(`Daemon log:\n${daemonLog}`);
    }
  } catch (error) {
    core.info(`Couldn't read daemon log at '${daemonLogPath}': ${error}`);
  }
};

module.exports = {
  printDaemonLog,
  STATE_BINARY_PATH,
  STATE_DAEMON_LOG_PATH,
  STATE_SOCKET_PATH,
};
