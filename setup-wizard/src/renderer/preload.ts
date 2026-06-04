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
  
  // File operations
  selectDirectory: () => ipcRenderer.invoke('select-directory'),
  listDirectory: (dirPath: string) => ipcRenderer.invoke('list-directory', dirPath),
  writeConfig: (configPath: string, data: any) => ipcRenderer.invoke('write-config', configPath, data),
  readConfig: (configPath: string) => ipcRenderer.invoke('read-config', configPath),
  
  // Paths
  getRepoRoot: () => ipcRenderer.invoke('get-repo-root'),
  joinPaths: (...parts: string[]) => ipcRenderer.invoke('join-paths', parts),
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
  
  offLogEntry: () => {
    ipcRenderer.removeAllListeners('log-entry');
  },

  // Process spawning (for launching servers)
  spawnProcess: (command: string, args: string[], options?: { cwd?: string; shell?: boolean }) => 
    ipcRenderer.invoke('spawn-process', command, args, options || {})
});
