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

  // Event listeners for command output streaming
  onCommandOutput: (callback: (data: { type: string; text: string }) => void) => {
    ipcRenderer.on('command-output', (_event, data) => callback(data));
  },
  
  offCommandOutput: () => {
    ipcRenderer.removeAllListeners('command-output');
  }
});
