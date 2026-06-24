/**
 * CodeAtlas Setup Wizard - Electron Main Process
 * Handles system commands: winget install, cargo build, npm install, etc.
 */

import { app, BrowserWindow, ipcMain, dialog } from 'electron';
import * as path from 'path';
import * as cp from 'child_process';
import * as fs from 'fs';
import * as os from 'os';
import { initLogger, log, LogLevel, getRecentLogs, readLogFile, clearLogFile } from './logger';

let mainWindow: BrowserWindow | null = null;

// Track all spawned child processes so they can be killed on quit
const activeChildren = new Set<cp.ChildProcess>();

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

    // Ensure LLVM (libclang.dll) and common tool paths are resolvable at runtime.
    const llvmBin = 'C:\\Program Files\\LLVM\\bin';
    const currentPath: string = (process.env['PATH'] || process.env['Path'] || '');
    const augmentedPath = currentPath.includes(llvmBin) ? currentPath : `${currentPath};${llvmBin}`;
    const spawnEnv = { ...process.env, PATH: augmentedPath };

    const child = cp.spawn(command, args, {
      cwd: fullCwd,
      shell: true,
      env: spawnEnv
    }) as cp.ChildProcessWithoutNullStreams;
    activeChildren.add(child);

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
      activeChildren.delete(child);
      const success = code === 0;
      log(success ? LogLevel.INFO : LogLevel.ERROR, 'COMMAND', 
        `Command finished with code ${code}`, 
        `${command} ${args.join(' ')}\nstdout: ${stdout.substring(0, 500)}\nstderr: ${stderr.substring(0, 500)}`);
      resolve({ success, stdout, stderr });
    });

    child.on('error', (err: Error) => {
      activeChildren.delete(child);
      log(LogLevel.ERROR, 'COMMAND', `Command error: ${err.message}`, `${command} ${args.join(' ')}`);
      resolve({ success: false, stdout, stderr: err.message });
    });
  });
}

/** Run a shell command with extra environment variables, streaming output to renderer */
function runCommandWithEnv(
  command: string,
  args: string[],
  cwd: string | undefined,
  extraEnv: Record<string, string>
): Promise<{ success: boolean; stdout: string; stderr: string }> {
  return new Promise((resolve) => {
    const fullCwd = cwd || process.cwd();

    if (!fs.existsSync(fullCwd)) {
      log(LogLevel.ERROR, 'COMMAND', `Directory not found: ${fullCwd}`);
      resolve({ success: false, stdout: '', stderr: `Directory not found: ${fullCwd}` });
      return;
    }

    const mergedEnv = { ...process.env, ...extraEnv };
    log(LogLevel.INFO, 'COMMAND', `Starting command (with env): ${command} ${args.join(' ')}`,
      `cwd=${fullCwd}, extraEnv=${JSON.stringify(extraEnv)}`);

    const child = cp.spawn(command, args, {
      cwd: fullCwd,
      shell: true,
      env: mergedEnv
    }) as cp.ChildProcessWithoutNullStreams;
    activeChildren.add(child);

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
      activeChildren.delete(child);
      const success = code === 0;
      log(success ? LogLevel.INFO : LogLevel.ERROR, 'COMMAND',
        `Command (with env) finished with code ${code}`,
        `${command} ${args.join(' ')}\nstdout: ${stdout.substring(0, 500)}\nstderr: ${stderr.substring(0, 500)}`);
      resolve({ success, stdout, stderr });
    });

    child.on('error', (err: Error) => {
      activeChildren.delete(child);
      log(LogLevel.ERROR, 'COMMAND', `Command (with env) error: ${err.message}`, `${command} ${args.join(' ')}`);
      resolve({ success: false, stdout, stderr: err.message });
    });
  });
}

/** Check if a command exists, with fallback to known Windows install paths */
function checkCommand(name: string): boolean {
  try {
    log(LogLevel.DEBUG, 'CHECK', `Checking for command: ${name}`);
    const result = cp.spawnSync(`where ${name}`, [], { shell: true });
    const found = result.status === 0 && (result.stdout as Buffer).toString().trim().length > 0;
    if (found) {
      log(LogLevel.INFO, 'CHECK', `${name} is available`);
      return true;
    }
    // Fallback: check known installation paths on Windows
    if (process.platform === 'win32') {
      const knownPaths: Record<string, string[]> = {
        clang: [
          'C:\\Program Files\\LLVM\\bin\\clang.exe',
          'C:\\Program Files (x86)\\LLVM\\bin\\clang.exe',
        ],
      };
      const fallbacks = knownPaths[name] ?? [];
      for (const p of fallbacks) {
        if (require('fs').existsSync(p)) {
          log(LogLevel.INFO, 'CHECK', `${name} found at known path: ${p}`);
          return true;
        }
      }
    }
    log(LogLevel.INFO, 'CHECK', `${name} not found`);
    return false;
  } catch (err) {
    const errorMsg = err instanceof Error ? err.message : String(err);
    log(LogLevel.ERROR, 'CHECK', `Error checking ${name}: ${errorMsg}`);
    return false;
  }
}

/** Get version of a command, falling back to known install paths on Windows */
function getVersion(name: string): Promise<string> {
  // Resolve the executable: prefer PATH, fall back to known paths on Windows
  const knownPaths: Record<string, string[]> = {
    clang: [
      'C:\\Program Files\\LLVM\\bin\\clang.exe',
      'C:\\Program Files (x86)\\LLVM\\bin\\clang.exe',
    ],
  };
  let exe = name;
  if (process.platform === 'win32') {
    const fallbacks = knownPaths[name] ?? [];
    for (const p of fallbacks) {
      if (require('fs').existsSync(p)) {
        exe = `"${p}"`;
        break;
      }
    }
  }
  return new Promise((resolve) => {
    cp.exec(`${exe} --version`, (error, stdout) => {
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
    activeChildren.add(child);

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
      activeChildren.delete(child);
      const success = code === 0;
      log(success ? LogLevel.INFO : LogLevel.ERROR, 'INSTALL', 
        `${displayName} installation ${success ? 'succeeded' : 'failed'} (exit code: ${code})`);
      resolve({ success, output });
    });

    child.on('error', (err: Error) => {
      activeChildren.delete(child);
      log(LogLevel.ERROR, 'INSTALL', `Winget error for ${displayName}: ${err.message}`);
      resolve({ success: false, output: err.message });
    });
  });
}

/** Select a file via dialog with optional type filters */
function selectFile(filters?: Array<{ name: string; extensions: string[] }>): Promise<string> {
  return new Promise((resolve) => {
    if (!mainWindow) {
      log(LogLevel.WARN, 'DIALOG', 'No main window available for file selection');
      resolve('');
      return;
    }
    try {
      const result = dialog.showOpenDialogSync(mainWindow, {
        properties: ['openFile'],
        title: 'Select File',
        filters: filters || [{ name: 'All Files', extensions: ['*'] }]
      });
      if (result && result.length > 0) {
        log(LogLevel.INFO, 'DIALOG', `Selected file: ${result[0]}`);
        resolve(result[0]);
      } else {
        log(LogLevel.DEBUG, 'DIALOG', 'File selection cancelled by user');
        resolve('');
      }
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : String(err);
      log(LogLevel.ERROR, 'DIALOG', `Failed to show file dialog: ${errorMsg}`);
      resolve('');
    }
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

/**
 * Read the Rust indexer version from `indexer/Cargo.toml`.
 * This is the single source of truth for the indexer version; the indexer
 * binary exposes no `--version` flag. Returns `null` when unavailable so the
 * UI can fail gracefully rather than showing a wrong value.
 */
function readIndexerVersion(): string | null {
  try {
    const cargoToml = path.join(getRepoRoot(), 'indexer', 'Cargo.toml');
    const contents = fs.readFileSync(cargoToml, 'utf8');
    // Match the first `version = "..."` in the [package] section. The package
    // version is declared before [dependencies], so the first match is correct.
    const match = contents.match(/^\s*version\s*=\s*"([^"]+)"/m);
    return match ? match[1] : null;
  } catch (err) {
    log(LogLevel.WARN, 'VERSION', `Failed to read indexer version: ${(err as Error).message}`);
    return null;
  }
}

// ==================== IPC Registration ====================

function emitLogToRenderer(event: any, logEntry: { level: string; step?: string; message: string }) {
  if (mainWindow && !mainWindow.isDestroyed()) {
    mainWindow.webContents.send('log-entry', logEntry);
  }
}

ipcMain.handle('copy-instructions', async (_event, dest: string, mode: string = 'copy') => {
  const fs = require('fs') as typeof import('fs');
  const pathMod = require('path') as typeof import('path');
  try {
    const src = pathMod.join(__dirname, '../../instructions/codeatlas_instructions.md');
    if (!fs.existsSync(src)) {
      return { success: false, error: 'Source file not found: ' + src };
    }

    if (mode === 'claude-ref') {
      // dest = workspaceRoot
      // 1) Copy instructions file into <workspace>/.codeatlas/codeatlas_instructions.md
      const instrDest = pathMod.join(dest, '.codeatlas', 'codeatlas_instructions.md');
      const instrDestDir = pathMod.dirname(instrDest);
      if (!fs.existsSync(instrDestDir)) {
        fs.mkdirSync(instrDestDir, { recursive: true });
      }
      fs.copyFileSync(src, instrDest);
      log(LogLevel.INFO, 'INSTRUCTIONS', `Copied instructions to: ${instrDest}`);

      // 2) Append @import line to CLAUDE.md (create if absent, skip if ref already present)
      const claudeMd = pathMod.join(dest, 'CLAUDE.md');
      const refLine = '@.codeatlas/codeatlas_instructions.md';
      let claudeWarning: string | undefined;
      try {
        const existing = fs.existsSync(claudeMd) ? fs.readFileSync(claudeMd, 'utf8') : '';
        if (!existing.includes(refLine)) {
          const separator = existing.length > 0 && !existing.endsWith('\n') ? '\n' : '';
          fs.appendFileSync(claudeMd, `${separator}\n${refLine}\n`, 'utf8');
          log(LogLevel.INFO, 'INSTRUCTIONS', `Appended import ref to CLAUDE.md: ${claudeMd}`);
        } else {
          log(LogLevel.INFO, 'INSTRUCTIONS', `CLAUDE.md already contains the import ref`);
        }
      } catch (claudeErr) {
        const msg = claudeErr instanceof Error ? claudeErr.message : String(claudeErr);
        claudeWarning = `Failed to write CLAUDE.md (file may be locked by another process): ${msg}\nPlease add the following line to CLAUDE.md manually: ${refLine}`;
        log(LogLevel.WARN, 'INSTRUCTIONS', claudeWarning);
      }
      return { success: true, claudeMd, instrDest, claudeWarning };
    }

    // Default: plain copy
    const destDir = pathMod.dirname(dest);
    if (!fs.existsSync(destDir)) {
      fs.mkdirSync(destDir, { recursive: true });
    }
    fs.copyFileSync(src, dest);
    log(LogLevel.INFO, 'INSTRUCTIONS', `Copied instructions to: ${dest}`);
    return { success: true };
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    log(LogLevel.ERROR, 'INSTRUCTIONS', `Failed to copy instructions: ${msg}`);
    return { success: false, error: msg };
  }
});

ipcMain.handle('apply-mcp-config', async (_event, opts: {
  workspacePath: string;
  dataDir: string;
  port: string;
  extensions?: string[];
}) => {
  const fs = require('fs') as typeof import('fs');
  const pathMod = require('path') as typeof import('path');
  try {
    const repoRoot = getRepoRoot();
    const fwd = (p: string) => p.replace(/\\/g, '/');

    const mcpJsPath  = pathMod.join(repoRoot, 'server', 'dist', 'mcp.js');
    const indexerExe = pathMod.join(
      repoRoot, 'indexer', 'target', 'release',
      process.platform === 'win32' ? 'codeatlas-indexer.exe' : 'codeatlas-indexer'
    );

    const mcpEntry: Record<string, unknown> = {
      type: 'stdio',
      command: 'node',
      args: [
        fwd(mcpJsPath),
        fwd(opts.dataDir),
      ],
      env: {
        CODEATLAS_WORKSPACE:          fwd(opts.workspacePath),
        CODEATLAS_PORT:               opts.port,
        CODEATLAS_DASHBOARD_AUTOOPEN: 'false',
        CODEATLAS_WATCHER:            'true',
        CODEATLAS_INDEXER_PATH:       fwd(indexerExe),
        CODEATLAS_INDEXER_STACK_BYTES:'134217728',
      },
    };

    // Pin the extension set the wizard indexed with so the MCP-spawned watcher
    // reuses exactly those extensions on every incremental run. Without this the
    // watcher falls back to the indexer's hard-coded default set, which can differ
    // from the user's selection and silently force a full rebuild.
    const exts = (opts.extensions ?? []).map((e) => e.trim()).filter(Boolean);
    if (exts.length > 0) {
      (mcpEntry.env as Record<string, string>).CODEATLAS_INDEX_EXTENSIONS = exts.join(',');
    }

    // Merge into <workspace>/.vscode/mcp.json
    const vscodeDir  = pathMod.join(opts.workspacePath, '.vscode');
    const mcpJsonPath = pathMod.join(vscodeDir, 'mcp.json');
    if (!fs.existsSync(vscodeDir)) fs.mkdirSync(vscodeDir, { recursive: true });

    let mcpConfig: { servers?: Record<string, unknown> } = { servers: {} };
    if (fs.existsSync(mcpJsonPath)) {
      try { mcpConfig = JSON.parse(fs.readFileSync(mcpJsonPath, 'utf8')); } catch {}
    }
    if (!mcpConfig.servers) mcpConfig.servers = {};
    mcpConfig.servers['codeatlas'] = mcpEntry;

    fs.writeFileSync(mcpJsonPath, JSON.stringify(mcpConfig, null, 2), 'utf8');
    log(LogLevel.INFO, 'MCP', `MCP config written to: ${mcpJsonPath}`);
    return { success: true, dest: mcpJsonPath };
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    log(LogLevel.ERROR, 'MCP', `Failed to write MCP config: ${msg}`);
    return { success: false, error: msg };
  }
});

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

ipcMain.handle('run-command-with-env', async (event, command: string, args: string[], cwd: string | undefined, env: Record<string, string>) => {
  emitLogToRenderer(event, { level: 'INFO', step: 'COMMAND', message: `Running with env: ${command} ${args.join(' ')}` });
  const result = await runCommandWithEnv(command, args, cwd, env || {});
  emitLogToRenderer(event, {
    level: result.success ? 'INFO' : 'ERROR',
    step: 'COMMAND',
    message: `Command finished with code ${result.success ? '0 (success)' : 'non-zero'}`
  });
  return result;
});

ipcMain.handle('select-file', async (event, filters?: Array<{ name: string; extensions: string[] }>) => {
  emitLogToRenderer(event, { level: 'INFO', step: 'DIALOG', message: 'Opening file selector...' });
  const result = await selectFile(filters);
  if (result) {
    emitLogToRenderer(event, { level: 'INFO', step: 'DIALOG', message: `Selected: ${result}` });
  }
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

ipcMain.handle('find-files', async (event, rootDir: string, ext: string, maxDepth: number) => {
  const depth = typeof maxDepth === 'number' ? maxDepth : 5;
  const results: string[] = [];
  const extLower = ext.toLowerCase();

  function walk(dir: string, currentDepth: number) {
    if (currentDepth > depth) return;
    let entries: fs.Dirent[];
    try {
      entries = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      if (entry.isFile() && entry.name.toLowerCase().endsWith(extLower)) {
        results.push(path.join(dir, entry.name));
      } else if (entry.isDirectory() && !entry.name.startsWith('.') && entry.name !== 'node_modules') {
        walk(path.join(dir, entry.name), currentDepth + 1);
      }
    }
  }

  walk(rootDir, 0);
  log(LogLevel.INFO, 'FS', `find-files: found ${results.length} *${ext} files under ${rootDir} (maxDepth=${depth})`);
  return results;
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

ipcMain.handle('get-indexer-version', async () => {
  return readIndexerVersion();
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

ipcMain.handle('file-mtime', async (_event, filePath: string) => {
  try {
    const stat = fs.statSync(filePath);
    return stat.mtimeMs;
  } catch {
    return 0;
  }
});

ipcMain.handle('spawn-process', async (event, command: string, args: string[], options?: any) => {
  emitLogToRenderer(event, { level: 'INFO', step: 'COMMAND', message: `Spawning: ${command} ${args.join(' ')}` });
  return new Promise((resolve) => {
    const logFile = path.join(process.env.APPDATA || os.homedir(), 'CodeAtlas', 'logs', `${command}-${Date.now()}.log`);
    
    const child = cp.spawn(command, args, { ...options, detached: true });
    if (child.stdout) {
      child.stdout.pipe(fs.createWriteStream(logFile, { flags: 'a' }));
    }
    if (child.stderr) {
      child.stderr.pipe(fs.createWriteStream(logFile, { flags: 'a' }));
    }
    
    child.unref();
    emitLogToRenderer(event, { level: 'INFO', step: 'COMMAND', message: `Process started (PID: ${child.pid})` });
    resolve({ success: true, pid: child.pid });
  });
});

ipcMain.handle('get-appdata-path', async () => {
  const appData = process.env.APPDATA || os.homedir();
  emitLogToRenderer(null as any, { level: 'INFO', step: 'PATH', message: `AppData path: ${appData}` });
  return appData;
});

ipcMain.handle('create-directory', async (_event, dirPath: string) => {
  try {
    const parentDir = path.dirname(dirPath);
    if (!fs.existsSync(parentDir)) {
      log(LogLevel.ERROR, 'FS', `Parent directory does not exist: ${parentDir}`);
      emitLogToRenderer(_event as any, { level: 'ERROR', step: 'FS', message: `Parent directory does not exist: ${parentDir}` });
      throw new Error(`Parent directory does not exist: ${parentDir}`);
    }
    
    const beforeExists = fs.existsSync(dirPath);
    log(LogLevel.INFO, 'FS', `Before mkdir - exists: ${beforeExists}, path: ${dirPath}`);
    emitLogToRenderer(_event as any, { level: 'INFO', step: 'FS', message: `Before mkdir - exists: ${beforeExists}, path: ${dirPath}` });
    
    if (!beforeExists) {
      fs.mkdirSync(dirPath, { recursive: true });
      const afterExists = fs.existsSync(dirPath);
      log(LogLevel.INFO, 'FS', `After mkdir - exists: ${afterExists}, path: ${dirPath}`);
      emitLogToRenderer(_event as any, { level: 'INFO', step: 'FS', message: `Created directory: ${dirPath}` });
    } else {
      emitLogToRenderer(_event as any, { level: 'DEBUG', step: 'FS', message: `Directory already exists: ${dirPath}` });
    }
    
    // Remove hidden attribute on Windows so folder is visible in Explorer
    if (process.platform === 'win32') {
      try {
        const { execSync } = require('child_process');
        execSync(`attrib -h "${dirPath}"`, { shell: 'cmd.exe' });
        log(LogLevel.DEBUG, 'FS', `Removed hidden attribute from ${dirPath}`);
      } catch (err: any) {
        log(LogLevel.WARN, 'FS', `Failed to remove hidden attribute: ${err.message}`);
      }
    }
    
    return true;
  } catch (err) {
    const errorMsg = err instanceof Error ? err.message : String(err);
    emitLogToRenderer(_event as any, { level: 'ERROR', step: 'FS', message: `Failed to create directory ${dirPath}: ${errorMsg}` });
    throw new Error(errorMsg);
  }
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

ipcMain.handle('check-user-path', async (event, dirToCheck: string) => {
  if (process.platform !== 'win32') return { inPath: false, platform: process.platform };
  try {
    const script = `[Environment]::GetEnvironmentVariable('PATH', 'User')`;
    const userPath = cp.execSync(`powershell -NoProfile -NonInteractive -Command "${script}"`, { encoding: 'utf8' }).trim();
    const inPath = userPath.split(';').some((p: string) => p.toLowerCase() === dirToCheck.toLowerCase());
    emitLogToRenderer(event, { level: 'INFO', step: 'PATH', message: `check-user-path "${dirToCheck}": ${inPath ? 'found' : 'not found'}` });
    return { inPath };
  } catch (err: any) {
    log(LogLevel.WARN, 'PATH', `check-user-path failed: ${err.message}`);
    return { inPath: false, error: err.message };
  }
});

ipcMain.handle('add-to-user-path', async (event, dirToAdd: string) => {
  if (process.platform !== 'win32') return { success: false, message: 'Windows only' };
  try {
    const getScript = `[Environment]::GetEnvironmentVariable('PATH', 'User')`;
    const userPath = cp.execSync(`powershell -NoProfile -NonInteractive -Command "${getScript}"`, { encoding: 'utf8' }).trim();
    const entries = userPath ? userPath.split(';') : [];
    if (entries.some((p: string) => p.toLowerCase() === dirToAdd.toLowerCase())) {
      emitLogToRenderer(event, { level: 'INFO', step: 'PATH', message: `Already in user PATH: ${dirToAdd}` });
      return { success: true, alreadyPresent: true };
    }
    const newPath = userPath ? `${userPath};${dirToAdd}` : dirToAdd;
    // Use -EncodedCommand to safely pass the path without injection risk
    const psScript = `[Environment]::SetEnvironmentVariable('PATH', '${newPath.replace(/'/g, "''")}', 'User')`;
    const encoded = Buffer.from(psScript, 'utf16le').toString('base64');
    cp.execSync(`powershell -NoProfile -NonInteractive -EncodedCommand ${encoded}`, { encoding: 'utf8' });
    log(LogLevel.INFO, 'PATH', `Added to user PATH: ${dirToAdd}`);
    emitLogToRenderer(event, { level: 'INFO', step: 'PATH', message: `Added to user PATH: ${dirToAdd}` });
    return { success: true, alreadyPresent: false };
  } catch (err: any) {
    log(LogLevel.ERROR, 'PATH', `add-to-user-path failed: ${err.message}`);
    emitLogToRenderer(event, { level: 'ERROR', step: 'PATH', message: `Failed to add to PATH: ${err.message}` });
    return { success: false, message: err.message };
  }
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

app.on('before-quit', () => {
  // Kill all tracked child processes so Electron can exit cleanly
  for (const child of activeChildren) {
    try { child.kill(); } catch (_) { /* ignore */ }
  }
  activeChildren.clear();
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') {
    app.quit();
  }
});
