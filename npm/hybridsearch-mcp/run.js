#!/usr/bin/env node

const { spawn } = require("child_process");
const os = require("os");
const path = require("path");

const BINARY_NAME = "hybrid-search";
const PLATFORM_PACKAGES = {
  "darwin-x64": "@lisonfan/hybridsearch-mcp-darwin-x64",
  "darwin-arm64": "@lisonfan/hybridsearch-mcp-darwin-arm64",
  "linux-x64": "@lisonfan/hybridsearch-mcp-linux-x64",
  "linux-arm64": "@lisonfan/hybridsearch-mcp-linux-arm64",
  "win32-x64": "@lisonfan/hybridsearch-mcp-win32-x64",
  "win32-arm64": "@lisonfan/hybridsearch-mcp-win32-arm64"
};

const platform = `${process.platform}-${process.arch}`;
const packageName = PLATFORM_PACKAGES[platform];

if (!packageName) {
  console.error(`Unsupported platform: ${platform}`);
  process.exit(1);
}

let binaryPath;
try {
  const manifestPath = require.resolve(`${packageName}/package.json`);
  const executable = process.platform === "win32" ? `${BINARY_NAME}.exe` : BINARY_NAME;
  binaryPath = path.join(path.dirname(manifestPath), "bin", executable);
} catch (_) {
  console.error(`Unable to find the platform package ${packageName}.`);
  console.error(`Reinstall with: npm install -g hybridsearch-mcp`);
  process.exit(1);
}

const child = spawn(binaryPath, process.argv.slice(2), {
  stdio: "inherit",
  env: process.env
});

for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) {
  process.on(signal, () => {
    if (!child.killed) child.kill(signal);
  });
}

child.on("error", (error) => {
  console.error(`Unable to start ${BINARY_NAME}: ${error.message}`);
  process.exit(1);
});

child.on("exit", (code, signal) => {
  if (signal) process.exit(128 + (os.constants.signals[signal] || 0));
  process.exit(code ?? 0);
});
