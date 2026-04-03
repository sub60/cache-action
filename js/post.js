// @ts-check

const { spawn } = require("node:child_process");

/**
 * @param {string} name
 * @returns {string}
 */
const getSavedState = (name) => {
  const envName = `STATE_${name.replace(/ /g, "_")}`;
  return process.env[envName] || "";
};

/**
 * @param {string} binaryPath
 * @param {string} socketPath
 * @returns {Promise<void>}
 */
const runStop = (binaryPath, socketPath) => {
  return new Promise((resolve, reject) => {
    const child = spawn(
      binaryPath,
      ["drain", "--socket", socketPath],
      {
        stdio: "inherit",
        env: process.env,
      },
    );

    child.on("error", reject);
    child.on("exit", (code, signal) => {
      if (code === 0) {
        resolve();
        return;
      }

      if (signal) {
        reject(new Error(`Daemon stop exited via signal '${signal}'.`));
        return;
      }

      reject(new Error(`Daemon stop exited with code ${code}.`));
    });
  });
};

/**
 * @returns {Promise<void>}
 */
const main = async () => {
  const socketPath = getSavedState("socket_path");
  const binaryPath = getSavedState("binary_path");

  if (!socketPath || !binaryPath) {
    console.log("No saved daemon state found; skipping shutdown.");
    return;
  }

  console.log(`Stopping daemon via socket '${socketPath}'.`);
  await runStop(binaryPath, socketPath);
};

main().catch((error) => {
  console.error(error.stack || error.message);
  process.exitCode = 1;
});
