import * as fs from "fs";
import * as path from "path";

/**
 * Reader for the indexer's append-only activity log (`<dataDir>/indexer.log`,
 * JSON Lines).  Used by the dashboard's live-log tab: an initial backfill via
 * {@link readRecentLogEntries} followed by an SSE tail via {@link ActivityLogTailer}.
 */

export const ACTIVITY_LOG_FILENAME = "indexer.log";

export interface ActivityLogEntry {
  ts: string;
  level: "info" | "warn" | "error";
  event: string;
  message: string;
  pid: number;
}

export function activityLogPath(dataDir: string): string {
  return path.join(dataDir, ACTIVITY_LOG_FILENAME);
}

/** Parse one JSONL line into an entry, or null when malformed. */
function parseLine(line: string): ActivityLogEntry | null {
  const trimmed = line.trim();
  if (!trimmed) return null;
  try {
    const parsed = JSON.parse(trimmed) as Partial<ActivityLogEntry>;
    if (
      typeof parsed.ts === "string" &&
      typeof parsed.event === "string" &&
      typeof parsed.message === "string"
    ) {
      return {
        ts: parsed.ts,
        level: parsed.level === "warn" || parsed.level === "error" ? parsed.level : "info",
        event: parsed.event,
        message: parsed.message,
        pid: typeof parsed.pid === "number" ? parsed.pid : 0,
      };
    }
  } catch {
    // ignore malformed lines (e.g. a partially-flushed final line)
  }
  return null;
}

/**
 * Read up to `limit` most-recent entries from the log for an initial backfill.
 * Returns an empty array when the log does not exist yet.
 */
export function readRecentLogEntries(dataDir: string, limit = 200): ActivityLogEntry[] {
  const file = activityLogPath(dataDir);
  let contents: string;
  try {
    contents = fs.readFileSync(file, "utf8");
  } catch {
    return [];
  }
  const lines = contents.split("\n");
  const entries: ActivityLogEntry[] = [];
  for (let i = Math.max(0, lines.length - limit); i < lines.length; i++) {
    const entry = parseLine(lines[i]);
    if (entry) entries.push(entry);
  }
  return entries;
}

/**
 * Tails the activity log, invoking `onEntry` for every new line appended after
 * the tailer starts.  Polling-based (no native fs.watch dependency) so it works
 * uniformly across platforms and survives log rotation: when the file shrinks
 * (rotated to `.1`) the read offset resets to 0.
 */
export class ActivityLogTailer {
  private readonly file: string;
  private offset = 0;
  private carry = "";
  private timer: NodeJS.Timeout | null = null;

  constructor(
    dataDir: string,
    private readonly onEntry: (entry: ActivityLogEntry) => void,
    private readonly intervalMs = 750,
  ) {
    this.file = activityLogPath(dataDir);
  }

  /** Begin tailing from the current end of the file (does not replay history). */
  start(): void {
    try {
      this.offset = fs.statSync(this.file).size;
    } catch {
      this.offset = 0;
    }
    this.timer = setInterval(() => this.poll(), this.intervalMs);
    if (typeof this.timer.unref === "function") this.timer.unref();
  }

  stop(): void {
    if (this.timer) {
      clearInterval(this.timer);
      this.timer = null;
    }
  }

  private poll(): void {
    let size: number;
    try {
      size = fs.statSync(this.file).size;
    } catch {
      // File not created yet — nothing to do.
      return;
    }
    // Rotation or truncation: restart from the top of the fresh file.
    if (size < this.offset) {
      this.offset = 0;
      this.carry = "";
    }
    if (size === this.offset) return;

    let chunk: string;
    try {
      const fd = fs.openSync(this.file, "r");
      try {
        const buffer = Buffer.alloc(size - this.offset);
        fs.readSync(fd, buffer, 0, buffer.length, this.offset);
        chunk = buffer.toString("utf8");
      } finally {
        fs.closeSync(fd);
      }
    } catch {
      return;
    }
    this.offset = size;

    const text = this.carry + chunk;
    const lines = text.split("\n");
    // The last element is an incomplete line (no trailing newline yet); carry it.
    this.carry = lines.pop() ?? "";
    for (const line of lines) {
      const entry = parseLine(line);
      if (entry) this.onEntry(entry);
    }
  }
}
