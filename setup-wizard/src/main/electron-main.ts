/**
 * CodeAtlas Setup Wizard - Electron Main Process
 * Handles system commands: winget install, cargo build, npm install, etc.
 */

import { app, BrowserWindow, ipcMain, dialog } from 'electron';
import * as path from 'path';
import * as cp from 'child_process';
import * as fs from 'fs';

let mainWindow: BrowserWindow | null = null;

function createWindow(): void {
  // Resolve paths relative to project root (parent of main/)
  const rootDir = path.resolve(__dirname, '..');
  
  mainWindow = new BrowserWindow({
    width: 720,
    height: 580,
    minWidth: 640,
    minHeight: 500,
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
      resolve({ success: false, stdout: '', stderr: `Directory not found: ${fullCwd}` });
      return;
    }

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
      resolve({ success: code === 0, stdout, stderr });
    });

    child.on('error', (err: Error) => {
      resolve({ success: false, stdout, stderr: err.message });
    });
  });
}

/** Check if a command exists */
function checkCommand(name: string): boolean {
  try {
    const result = cp.spawnSync(`where ${name}`, [], { shell: true });
    return result.status === 0 && (result.stdout as Buffer).toString().trim().length > 0;
  } catch {
    return false;
  }
}

/** Get version of a command */
function getVersion(name: string): Promise<string> {
  return new Promise((resolve) => {
    cp.exec(`${name} --version`, (error, stdout) => {
      if (error) resolve('');
      else resolve(stdout.trim().split('\n')[0]);
    });
  });
}

/** Install via winget */
function installWithWinget(packageId: string, displayName: string): Promise<{ success: boolean; output: string }> {
  return new Promise((resolve) => {
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
      resolve({ success: code === 0, output });
    });
  });
}

/** Select a directory via dialog */
function selectDirectory(): Promise<string> {
  return new Promise((resolve) => {
    if (!mainWindow) {
      resolve('');
      return;
    }

    const result = dialog.showOpenDialogSync(mainWindow, {
      properties: ['openDirectory']
    });

    if (result && result.length > 0) {
      resolve(result[0]);
    } else {
      resolve('');
    }
  });
}

/** Read directory listing */
function listDirectory(dirPath: string): Promise<string[]> {
  return new Promise((resolve) => {
    if (!fs.existsSync(dirPath)) {
      resolve([]);
      return;
    }
    try {
      const entries = fs.readdirSync(dirPath, { withFileTypes: true });
      const dirs = entries.filter(e => e.isDirectory()).map(e => e.name).sort();
      const files = entries.filter(e => e.isFile()).map(e => e.name).sort();
      resolve([...dirs.map(d => d + '/'), ...files]);
    } catch {
      resolve([]);
    }
  });
}

/** Write config file */
function writeConfig(configPath: string, data: any): Promise<boolean> {
  try {
    const dir = path.dirname(configPath);
    if (!fs.existsSync(dir)) {
      fs.mkdirSync(dir, { recursive: true });
    }
    fs.writeFileSync(configPath, JSON.stringify(data, null, 2), 'utf-8');
    return Promise.resolve(true);
  } catch (err) {
    return Promise.resolve(false);
  }
}

/** Read config file */
function readConfig(configPath: string): Promise<any> {
  try {
    if (!fs.existsSync(configPath)) {
      return Promise.resolve(null);
    }
    const content = fs.readFileSync(configPath, 'utf-8');
    return Promise.resolve(JSON.parse(content));
  } catch {
    return Promise.resolve(null);
  }
}

/** Get repo root path */
function getRepoRoot(): string {
  return path.resolve(__dirname, '..', '..');
}

// ==================== IPC Registration ====================

ipcMain.handle('check-command', async (_event, name: string) => {
  const exists = checkCommand(name);
  let version = '';
  if (exists) {
    version = await getVersion(name);
  }
  return { exists, version };
});

ipcMain.handle('install-winget', async (_event, packageId: string, displayName: string) => {
  return installWithWinget(packageId, displayName);
});

ipcMain.handle('run-command', async (_event, command: string, args: string[], cwd?: string) => {
  return runCommand(command, args, cwd);
});

ipcMain.handle('select-directory', async () => {
  return selectDirectory();
});

ipcMain.handle('list-directory', async (_event, dirPath: string) => {
  return listDirectory(dirPath);
});

ipcMain.handle('write-config', async (_event, configPath: string, data: any) => {
  return writeConfig(configPath, data);
});

ipcMain.handle('read-config', async (_event, configPath: string) => {
  return readConfig(configPath);
});

ipcMain.handle('get-repo-root', async () => {
  return getRepoRoot();
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
