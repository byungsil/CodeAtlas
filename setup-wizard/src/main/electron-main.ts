/**
 * CodeAtlas Setup Wizard - Electron Main Process
 * Handles system commands: winget install, cargo build, npm install, etc.
 */

import { app, BrowserWindow, ipcMain, dialog } from 'electron';
import * as path from 'path';
import * as cp from 'child_process';
import * as fs from 'fs';
import { initLogger, log, LogLevel, getRecentLogs, readLogFile, clearLogFile } from './logger';

let mainWindow: BrowserWindow | null = null;

// Initialize logger on app start
const logPath = initLogger();
log(LogLevel.INFO, 'APP', `Setup Wizard started. Log file: ${logPath}`);

function createWindow(): void {
  // Resolve paths relative to project root (parent of main/)
  const rootDir = path.resolve(__dirname, '..');
  
  mainWindow = new BrowserWindow({
    width: 800,
    height: 700,
    minWidth: 720,
    minHeight: 600,
    frame: true,
    title: 'CodeAtlas Setup Wizard',
    icon: path.join(rootDir, 'assets', 'icon.png'),
    webPreferences: {
      nodeIntegration: false,
      contextIsolation: true,
      preload: path.join(rootDir, 'renderer', 'preload.js')
    }
  });

  mainWindow.loadFile(path.join(rootDir, 'src', 'renderer', 'index.html'));
}

// ==================== IPC Handlers ====================

/** Run a shell command and stream output back to renderer */
function runCommand(command: string, args: string[], cwd?: string): Promise<{ success: boolean; stdout: string; stderr: string }> {
  return new Promise((resolve) => {
    const fullCwd = cwd || process.cwd();
    
    if (!fs.existsSync(fullCwd)) {
      log(LogLevel.ERROR, 'COMMAND', `Directory not found: ${fullCwd}`);
      resolve({ success: false, stdout: '', stderr: `Directory not found: ${fullCwd}` });
      return;
    }

    log(LogLevel.INFO, 'COMMAND', `Starting command: ${command} ${args.join(' ')}`, `cwd=${fullCwd}`);

    const child = cp.spawn(command, args, {
      cwd: fullCwd,
      shell: true
    }) as cp.ChildProcessWithoutNullStreams;

    let stdout = '';
    let stderr = '';

    child.stdout.on('data', (data: Buffer) => {
      const text = data.toString();
      stdout += text;
      if (mainWindow && !mainWindow.isDestroyed()) {
        mainWindow.webContents.send('command-output', { type: 'stdout', text });
      }
    });

    child.stderr.on('data', (data: Buffer) => {
      const text = data.toString();
      stderr += text;
      if (mainWindow && !mainWindow.isDestroyed()) {
        mainWindow.webContents.send('command-output', { type: 'stderr', text });
      }
    });

    child.on('close', (code: number) => {
      const success = code === 0;
      log(success ? LogLevel.INFO : LogLevel.ERROR, 'COMMAND', 
        `Command finished with code ${code}`, 
        `${command} ${args.join(' ')}\nstdout: ${stdout.substring(0, 500)}\nstderr: ${stderr.substring(0, 500)}`);
      resolve({ success, stdout, stderr });
    });

    child.on('error', (err: Error) => {
      log(LogLevel.ERROR, 'COMMAND', `Command error: ${err.message}`, `${command} ${args.join(' ')}`);
      resolve({ success: false, stdout, stderr: err.message });
    });
  });
}

/** Check if a command exists */
function checkCommand(name: string): boolean {
  try {
    log(LogLevel.DEBUG, 'CHECK', `Checking for command: ${name}`);
    const result = cp.spawnSync(`where ${name}`, [], { shell: true });
    const found = result.status === 0 && (result.stdout as Buffer).toString().trim().length > 0;
    log(LogLevel.INFO, 'CHECK', `${name} is ${found ? 'available' : 'not found'}`);
    return found;
  } catch (err) {
    const errorMsg = err instanceof Error ? err.message : String(err);
    log(LogLevel.ERROR, 'CHECK', `Error checking ${name}: ${errorMsg}`);
    return false;
  }
}

/** Get version of a command */
function getVersion(name: string): Promise<string> {
  return new Promise((resolve) => {
    cp.exec(`${name} --version`, (error, stdout) => {
      if (error) {
        log(LogLevel.WARN, 'CHECK', `Failed to get version for ${name}`);
        resolve('');
      } else {
        const version = stdout.trim().split('\n')[0];
        log(LogLevel.INFO, 'CHECK', `${name} version: ${version}`);
        resolve(version);
      }
    });
  });
}

/** Install via winget */
function installWithWinget(packageId: string, displayName: string): Promise<{ success: boolean; output: string }> {
  return new Promise((resolve) => {
    log(LogLevel.INFO, 'INSTALL', `Installing ${displayName} (${packageId})`);
    
    if (mainWindow && !mainWindow.isDestroyed()) {
      mainWindow.webContents.send('command-output', { type: 'stdout', text: `Installing ${displayName} with winget...\n` });
    }

    const child = cp.spawn(
      'winget',
      ['install', '--exact', '--id', packageId, '--accept-source-agreements', '--accept-package-agreements'],
      { shell: true }
    ) as cp.ChildProcessWithoutNullStreams;

    let output = '';

    child.stdout.on('data', (data: Buffer) => {
      const text = data.toString();
      output += text;
      if (mainWindow && !mainWindow.isDestroyed()) {
        mainWindow.webContents.send('command-output', { type: 'stdout', text });
      }
    });

    child.stderr.on('data', (data: Buffer) => {
      const text = data.toString();
      output += text;
      if (mainWindow && !mainWindow.isDestroyed()) {
        mainWindow.webContents.send('command-output', { type: 'stderr', text });
      }
    });

    child.on('close', (code: number) => {
      const success = code === 0;
      log(success ? LogLevel.INFO : LogLevel.ERROR, 'INSTALL', 
        `${displayName} installation ${success ? 'succeeded' : 'failed'} (exit code: ${code})`);
      resolve({ success, output });
    });

    child.on('error', (err: Error) => {
      log(LogLevel.ERROR, 'INSTALL', `Winget error for ${displayName}: ${err.message}`);
      resolve({ success: false, output: err.message });
    });
  });
}

/** Select a directory via dialog */
function selectDirectory(): Promise<string> {
  return new Promise((resolve) => {
    if (!mainWindow) {
      log(LogLevel.WARN, 'DIALOG', 'No main window available for directory selection');
      resolve('');
      return;
    }

    try {
      const result = dialog.showOpenDialogSync(mainWindow, {
        properties: ['openDirectory'],
        title: 'Select Codebase Directory'
      });

      if (result && result.length > 0) {
        log(LogLevel.INFO, 'DIALOG', `Selected directory: ${result[0]}`);
        resolve(result[0]);
      } else {
        log(LogLevel.DEBUG, 'DIALOG', 'Directory selection cancelled by user');
        resolve('');
      }
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : String(err);
      log(LogLevel.ERROR, 'DIALOG', `Failed to show directory dialog: ${errorMsg}`);
      resolve('');
    }
  });
}

/** Read directory listing */
function listDirectory(dirPath: string): Promise<string[]> {
  return new Promise((resolve) => {
    if (!fs.existsSync(dirPath)) {
      log(LogLevel.WARN, 'FS', `Directory not found for listing: ${dirPath}`);
      resolve([]);
      return;
    }
    try {
      const entries = fs.readdirSync(dirPath, { withFileTypes: true });
      const dirs = entries.filter(e => e.isDirectory()).map(e => e.name).sort();
      const files = entries.filter(e => e.isFile()).map(e => e.name).sort();
      const result = [...dirs.map(d => d + '/'), ...files];
      log(LogLevel.DEBUG, 'FS', `Listed ${result.length} items in ${dirPath}`);
      resolve(result);
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : String(err);
      log(LogLevel.ERROR, 'FS', `Failed to list directory ${dirPath}: ${errorMsg}`);
      resolve([]);
    }
  });
}

/** Write config file */
function writeConfig(configPath: string, data: any): Promise<boolean> {
  try {
    log(LogLevel.INFO, 'FS', `Writing config to ${configPath}`);
    const dir = path.dirname(configPath);
    if (!fs.existsSync(dir)) {
      fs.mkdirSync(dir, { recursive: true });
      log(LogLevel.DEBUG, 'FS', `Created directory: ${dir}`);
    }
    fs.writeFileSync(configPath, JSON.stringify(data, null, 2), 'utf-8');
    log(LogLevel.INFO, 'FS', `Config written successfully to ${configPath}`);
    return Promise.resolve(true);
  } catch (err) {
    const errorMsg = err instanceof Error ? err.message : String(err);
    log(LogLevel.ERROR, 'FS', `Failed to write config to ${configPath}: ${errorMsg}`);
    return Promise.resolve(false);
  }
}

/** Read config file */
function readConfig(configPath: string): Promise<any> {
  try {
    if (!fs.existsSync(configPath)) {
      log(LogLevel.DEBUG, 'FS', `Config file not found: ${configPath}`);
      return Promise.resolve(null);
    }
    const content = fs.readFileSync(configPath, 'utf-8');
    const data = JSON.parse(content);
    log(LogLevel.DEBUG, 'FS', `Config read from ${configPath} (${Object.keys(data).length} keys)`);
    return Promise.resolve(data);
  } catch (err) {
    const errorMsg = err instanceof Error ? err.message : String(err);
    log(LogLevel.ERROR, 'FS', `Failed to read config ${configPath}: ${errorMsg}`);
    return Promise.resolve(null);
  }
}

/** Get repo root path */
function getRepoRoot(): string {
  const repoRoot = path.resolve(__dirname, '..', '..');
  log(LogLevel.DEBUG, 'PATH', `Repo root: ${repoRoot}`);
  return repoRoot;
}

// ==================== IPC Registration ====================

function emitLogToRenderer(event: any, logEntry: { level: string; step?: string; message: string }) {
  if (mainWindow && !mainWindow.isDestroyed()) {
    mainWindow.webContents.send('log-entry', logEntry);
  }
}

ipcMain.handle('check-command', async (event, name: string) => {
  const exists = checkCommand(name);
  let version = '';
  if (exists) {
    version = await getVersion(name);
  }
  emitLogToRenderer(event, { level: 'INFO', step: 'CHECK', message: `Checked ${name}: ${exists ? 'found' : 'not found'}${version ? ' (' + version + ')' : ''}` });
  return { exists, version };
});

ipcMain.handle('install-winget', async (event, packageId: string, displayName: string) => {
  emitLogToRenderer(event, { level: 'INFO', step: 'INSTALL', message: `Installing ${displayName}...` });
  const result = await installWithWinget(packageId, displayName);
  emitLogToRenderer(event, { 
    level: result.success ? 'INFO' : 'ERROR', 
    step: 'INSTALL', 
    message: `${displayName} installation ${result.success ? 'succeeded' : 'failed'}` 
  });
  return result;
});

ipcMain.handle('run-command', async (event, command: string, args: string[], cwd?: string) => {
  emitLogToRenderer(event, { level: 'INFO', step: 'COMMAND', message: `Running: ${command} ${args.join(' ')}` });
  const result = await runCommand(command, args, cwd);
  emitLogToRenderer(event, { 
    level: result.success ? 'INFO' : 'ERROR', 
    step: 'COMMAND', 
    message: `Command finished with code ${result.success ? '0 (success)' : 'non-zero'}` 
  });
  return result;
});

ipcMain.handle('select-directory', async (event) => {
  emitLogToRenderer(event, { level: 'INFO', step: 'DIALOG', message: 'Opening directory selector...' });
  const result = await selectDirectory();
  if (result) {
    emitLogToRenderer(event, { level: 'INFO', step: 'DIALOG', message: `Selected: ${result}` });
  }
  return result;
});

ipcMain.handle('list-directory', async (event, dirPath: string) => {
  const entries = await listDirectory(dirPath);
  emitLogToRenderer(event, { level: 'INFO', step: 'FS', message: `Listed ${entries.length} items in ${dirPath}` });
  return entries;
});

ipcMain.handle('write-config', async (event, configPath: string, data: any) => {
  emitLogToRenderer(event, { level: 'INFO', step: 'FS', message: `Writing config to ${configPath}` });
  const result = await writeConfig(configPath, data);
  emitLogToRenderer(event, { 
    level: result ? 'INFO' : 'ERROR', 
    step: 'FS', 
    message: `Config ${result ? 'written successfully' : 'failed to write'} at ${configPath}` 
  });
  return result;
});

ipcMain.handle('read-config', async (event, configPath: string) => {
  const data = await readConfig(configPath);
  emitLogToRenderer(event, { level: 'INFO', step: 'FS', message: `Read config from ${configPath}${data ? ' (' + Object.keys(data).length + ' keys)' : ' (not found)'}` });
  return data;
});

ipcMain.handle('get-repo-root', async (event) => {
  const repoRoot = getRepoRoot();
  emitLogToRenderer(event, { level: 'INFO', step: 'PATH', message: `Repository root: ${repoRoot}` });
  return repoRoot;
});

ipcMain.handle('join-paths', async (_event, parts: string[]) => {
  const joined = path.join(...parts);
  emitLogToRenderer(_event, { level: 'INFO', step: 'PATH', message: `Joined paths: ${joined}` });
  return joined;
});

ipcMain.handle('file-exists', async (event, filePath: string) => {
  const exists = fs.existsSync(filePath);
  emitLogToRenderer(event, { level: 'INFO', step: 'FS', message: `File ${exists ? 'found' : 'not found'}: ${filePath}` });
  return exists;
});

ipcMain.handle('spawn-process', async (event, command: string, args: string[], options?: any) => {
  emitLogToRenderer(event, { level: 'INFO', step: 'COMMAND', message: `Spawning: ${command} ${args.join(' ')}` });
  return new Promise((resolve) => {
    const child = cp.spawn(command, args, { ...options, detached: true, stdio: 'ignore' });
    child.unref();
    emitLogToRenderer(event, { level: 'INFO', step: 'COMMAND', message: `Process started (PID: ${child.pid})` });
    resolve({ success: true, pid: child.pid });
  });
});

// Log retrieval IPC handlers
ipcMain.handle('get-recent-logs', async (_event, count: number = 100) => {
  const logs = getRecentLogs(count);
  return logs;
});

ipcMain.handle('read-log-file', async () => {
  const content = readLogFile();
  if (content === null) {
    emitLogToRenderer({ webContents: { send: () => {} } }, { level: 'WARN', step: 'LOG', message: 'No log file found' });
  }
  return content;
});

ipcMain.handle('clear-log-file', async () => {
  const result = clearLogFile();
  if (result) {
    emitLogToRenderer({ webContents: { send: () => {} } }, { level: 'INFO', step: 'LOG', message: 'Log file cleared' });
  }
  return result;
});

// ==================== App Lifecycle ====================

app.whenReady().then(() => {
  createWindow();

  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      createWindow();
    }
  });
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') {
    app.quit();
  }
});
