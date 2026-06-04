/**
 * CodeAtlas Setup Wizard - Centralized Logger
 * Writes to both console and log file with structured format.
 */

import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';

export enum LogLevel {
  INFO = 'INFO',
  WARN = 'WARN',
  ERROR = 'ERROR',
  DEBUG = 'DEBUG'
}

export interface LogEntry {
  timestamp: string;
  level: LogLevel;
  step?: string;
  message: string;
  details?: string;
}

let logFilePath: string | null = null;
const logBuffer: LogEntry[] = [];
const MAX_BUFFER_SIZE = 500; // Keep last N entries in memory for UI display

export function initLogger(): string {
  const appDataDir = process.env.APPDATA || path.join(os.homedir(), '.codeatlas');
  const logsDir = path.join(appDataDir, 'CodeAtlas', 'logs');
  
  if (!fs.existsSync(logsDir)) {
    fs.mkdirSync(logsDir, { recursive: true });
  }

  const dateStr = new Date().toISOString().split('T')[0].replace(/-/g, '');
  logFilePath = path.join(logsDir, `setup-wizard-${dateStr}.log`);
  
  // Write header
  fs.writeFileSync(logFilePath, 
    `=== CodeAtlas Setup Wizard Log ===\n` +
    `Started: ${new Date().toISOString()}\n` +
    `Platform: ${process.platform} ${process.arch}\n` +
    `Node: ${process.version}\n` +
    `Electron: process.versions.electron\n` +
    `${'='.repeat(60)}\n\n`,
    { encoding: 'utf-8' }
  );

  log(`INFO`, null, `Logger initialized at ${logFilePath}`);
  return logFilePath;
}

export function getLogPath(): string | null {
  return logFilePath;
}

export function log(level: LogLevel | string, step: string | null, message: string, details?: string): void {
  const entry: LogEntry = {
    timestamp: new Date().toISOString(),
    level: (level as LogLevel) || LogLevel.INFO,
    step: step || undefined,
    message,
    details
  };

  // Add to in-memory buffer (circular)
  logBuffer.push(entry);
  if (logBuffer.length > MAX_BUFFER_SIZE) {
    logBuffer.shift();
  }

  // Format for console output
  const timeStr = new Date().toISOString().replace('T', ' ').substring(0, 23);
  const stepPrefix = entry.step ? `[${entry.step}] ` : '';
  const colorCode = getLevelColor(level);
  const formattedMessage = `${timeStr} [${level}] ${stepPrefix}${message}`;

  // Console output with colors
  switch (level) {
    case LogLevel.ERROR:
      console.error(`\x1b[31m${formattedMessage}\x1b[0m`);
      break;
    case LogLevel.WARN:
      console.warn(`\x1b[33m${formattedMessage}\x1b[0m`);
      break;
    case LogLevel.DEBUG:
      if (process.env.NODE_ENV !== 'production') {
        console.debug(`\x1b[90m${formattedMessage}\x1b[0m`);
      }
      break;
    default:
      console.log(formattedMessage);
  }

  // Write to file
  if (logFilePath) {
    const logLine = `[${entry.timestamp}] [${level}] ${stepPrefix}${message}\n`;
    fs.appendFileSync(logFilePath, logLine, { encoding: 'utf-8' });
    
    if (details) {
      fs.appendFileSync(logFilePath, `  Details: ${details}\n`, { encoding: 'utf-8' });
    }
    fs.appendFileSync(logFilePath, '\n', { encoding: 'utf-8' });
  }
}

export function getRecentLogs(count: number = 100): LogEntry[] {
  return logBuffer.slice(-count);
}

export function readLogFile(): string | null {
  if (!logFilePath || !fs.existsSync(logFilePath)) {
    return null;
  }
  try {
    return fs.readFileSync(logFilePath, 'utf-8');
  } catch (err) {
    console.error(`Failed to read log file: ${err}`);
    return null;
  }
}

export function clearLogFile(): boolean {
  if (!logFilePath) return false;
  try {
    fs.writeFileSync(logFilePath, '', { encoding: 'utf-8' });
    return true;
  } catch (err) {
    console.error(`Failed to clear log file: ${err}`);
    return false;
  }
}

function getLevelColor(level: LogLevel | string): string {
  switch (level) {
    case LogLevel.ERROR: return '\x1b[31m'; // Red
    case LogLevel.WARN: return '\x1b[33m';  // Yellow
    case LogLevel.DEBUG: return '\x1b[90m'; // Gray
    default: return '';
  }
}

// Expose for Electron main process
(global as any).log = log;
(global as any).LogLevel = LogLevel;
