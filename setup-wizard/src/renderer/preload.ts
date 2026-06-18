/**
 * CodeAtlas Setup Wizard - Preload Script
 * Secure bridge between Electron main process and renderer
 */

import { contextBridge, ipcRenderer } from 'electron';

contextBridge.exposeInMainWorld('codeatlas', {
  // System commands
  checkCommand: (name: string) => ipcRenderer.invoke('check-command', name),
  installWinget: (packageId: string, displayName: string) => ipcRenderer.invoke('install-winget', packageId, displayName),
  runCommand: (command: string, args: string[], cwd?: string) => ipcRenderer.invoke('run-command', command, args, cwd),
  runCommandWithEnv: (command: string, args: string[], cwd?: string, env?: Record<string, string>) =>
    ipcRenderer.invoke('run-command-with-env', command, args, cwd, env || {}),
  
  // File operations
  selectDirectory: () => ipcRenderer.invoke('select-directory'),
  selectFile: (filters?: Array<{name: string; extensions: string[]}>) =>
    ipcRenderer.invoke('select-file', filters),
  listDirectory: (dirPath: string) => ipcRenderer.invoke('list-directory', dirPath),
  findFiles: (rootDir: string, ext: string, maxDepth?: number) =>
    ipcRenderer.invoke('find-files', rootDir, ext, maxDepth ?? 5),
  writeConfig: (configPath: string, data: any) => ipcRenderer.invoke('write-config', configPath, data),
  readConfig: (configPath: string) => ipcRenderer.invoke('read-config', configPath),
  
  // Paths
  getRepoRoot: () => ipcRenderer.invoke('get-repo-root'),
  joinPaths: (...parts: string[]) => ipcRenderer.invoke('join-paths', [...parts]),
  fileExists: (filePath: string) => ipcRenderer.invoke('file-exists', filePath),

  // Log operations
  getRecentLogs: (count?: number) => ipcRenderer.invoke('get-recent-logs', count || 100),
  readLogFile: () => ipcRenderer.invoke('read-log-file'),
  clearLogFile: () => ipcRenderer.invoke('clear-log-file'),
  
  // Event listeners for command output streaming
  onCommandOutput: (callback: (data: { type: string; text: string }) => void) => {
    ipcRenderer.on('command-output', (_event, data) => callback(data));
  },
  
  offCommandOutput: () => {
    ipcRenderer.removeAllListeners('command-output');
  },

  // Event listeners for log entries
  onLogEntry: (callback: (log: { level: string; step?: string; message: string }) => void) => {
    ipcRenderer.on('log-entry', (_event, data) => callback(data));
  },
  
  // Platform info (process object not available in renderer)
  platform: process.platform,

  // PATH management (Windows user PATH in registry)
  checkUserPath: (dirToCheck: string) => ipcRenderer.invoke('check-user-path', dirToCheck),
  addToUserPath: (dirToAdd: string) => ipcRenderer.invoke('add-to-user-path', dirToAdd),

  offLogEntry: () => {
    ipcRenderer.removeAllListeners('log-entry');
  },

  // Process spawning (for launching servers)
  spawnProcess: (command: string, args: string[], options?: { cwd?: string; shell?: boolean }) => 
    ipcRenderer.invoke('spawn-process', command, args, options || {}),

  // App paths
  getAppDataPath: () => ipcRenderer.invoke('get-appdata-path'),

  // Directory operations
  createDirectory: (dirPath: string) => ipcRenderer.invoke('create-directory', dirPath),

  // Agent instructions
  copyInstructions: (dest: string) => ipcRenderer.invoke('copy-instructions', dest)
});
