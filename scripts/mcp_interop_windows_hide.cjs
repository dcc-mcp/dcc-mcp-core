// Keep test-owned external client bridge processes hidden on Windows.
const childProcess = require('node:child_process');
const originalSpawn = childProcess.spawn;
childProcess.spawn = function (command, args, options) {
  return originalSpawn.call(this, command, args, { ...options, windowsHide: true });
};
