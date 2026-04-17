import * as fs from "fs";
import * as path from "path";
import * as crypto from "crypto";
import { parseCppFile, RawCallSite } from "./parser/cpp-parser";
import { JsonStore, IndexData } from "./storage/json-store";
import { DATA_DIR_NAME } from "./constants";
import { Symbol } from "./models/symbol";
import { Call } from "./models/call";
import { FileRecord } from "./models/file-record";

function findCppFiles(dir: string): string[] {
  const results: string[] = [];
  const entries = fs.readdirSync(dir, { withFileTypes: true });
  for (const entry of entries) {
    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory() && entry.name !== DATA_DIR_NAME) {
      results.push(...findCppFiles(fullPath));
    } else if (/\.(cpp|h|hpp|cc|cxx|inl|inc)$/.test(entry.name)) {
      results.push(fullPath);
    }
  }
  return results;
}

function hashContent(content: string): string {
  return crypto.createHash("sha256").update(content).digest("hex");
}

function mergeSymbols(allSymbols: Symbol[]): Symbol[] {
  const byId = new Map<string, Symbol>();
  for (const sym of allSymbols) {
    const existing = byId.get(sym.id);
    if (!existing) {
      byId.set(sym.id, { ...sym });
    } else {
      if (sym.filePath.endsWith(".cpp") && existing.filePath.endsWith(".h")) {
        existing.filePath = sym.filePath;
        existing.line = sym.line;
        existing.endLine = sym.endLine;
        if (sym.signature) existing.signature = sym.signature;
      }
    }
  }
  return Array.from(byId.values());
}

function resolveCallSites(rawCalls: RawCallSite[], symbols: Symbol[], localTypes: Map<string, Map<string, string>>): Call[] {
  const calls: Call[] = [];
  const seen = new Set<string>();

  for (const raw of rawCalls) {
    const caller = symbols.find((s) => s.id === raw.callerId);
    if (!caller) continue;

    let callee: Symbol | undefined;

    if (raw.receiver) {
      const typeName = resolveReceiverType(raw.receiver, raw.callerId, raw.filePath, symbols, localTypes);
      if (typeName) {
        callee = symbols.find(
          (s) => s.name === raw.calledName && s.parentId && s.parentId.endsWith(typeName),
        );
      }
    }

    if (!callee && caller.parentId) {
      callee = symbols.find(
        (s) => s.name === raw.calledName && s.parentId === caller.parentId,
      );
    }

    if (!callee) {
      callee = symbols.find(
        (s) => s.name === raw.calledName && (s.type === "function" || s.type === "method"),
      );
    }

    if (callee && callee.id !== raw.callerId) {
      const key = `${raw.callerId}->${callee.id}@${raw.filePath}:${raw.line}`;
      if (!seen.has(key)) {
        seen.add(key);
        calls.push({
          callerId: raw.callerId,
          calleeId: callee.id,
          filePath: raw.filePath,
          line: raw.line,
        });
      }
    }
  }

  return calls;
}

function resolveReceiverType(
  receiver: string,
  callerId: string,
  filePath: string,
  symbols: Symbol[],
  localTypes: Map<string, Map<string, string>>,
): string | undefined {
  if (receiver.startsWith("m_")) {
    const caller = symbols.find((s) => s.id === callerId);
    if (caller?.parentId) {
      const parent = symbols.find((s) => s.id === caller.parentId);
      if (parent) {
        const fieldMap = localTypes.get(parent.id);
        if (fieldMap) return fieldMap.get(receiver);
      }
    }
  }

  const fileLocals = localTypes.get(`file:${filePath}:${callerId}`);
  if (fileLocals) {
    const t = fileLocals.get(receiver);
    if (t) return t;
  }

  return undefined;
}

function buildLocalTypeMap(files: Map<string, string>, symbols: Symbol[]): Map<string, Map<string, string>> {
  const result = new Map<string, Map<string, string>>();
  const typeRegex = /(\w+)(?:\s*[*&])?\s+(\w+)\s*[=;(,)]/g;
  const memberRegex = /^\s+(\w+)\*?\s+(m_\w+)/;

  for (const [filePath, content] of files) {
    const lines = content.split("\n");

    for (const sym of symbols) {
      if (sym.type !== "function" && sym.type !== "method") continue;
      if (sym.filePath !== filePath) continue;

      const localMap = new Map<string, string>();
      for (let i = sym.line - 1; i < sym.endLine && i < lines.length; i++) {
        typeRegex.lastIndex = 0;
        let m: RegExpExecArray | null;
        while ((m = typeRegex.exec(lines[i])) !== null) {
          const typeName = m[1];
          const varName = m[2];
          if (symbols.some((s) => s.name === typeName && (s.type === "class" || s.type === "struct"))) {
            localMap.set(varName, typeName);
          }
        }
      }
      if (localMap.size > 0) {
        result.set(`file:${filePath}:${sym.id}`, localMap);
      }
    }

    for (const sym of symbols) {
      if (sym.type !== "class" && sym.type !== "struct") continue;
      if (sym.filePath !== filePath) continue;

      const fieldMap = new Map<string, string>();
      for (let i = sym.line; i < sym.endLine && i < lines.length; i++) {
        const mMatch = lines[i].match(memberRegex);
        if (mMatch) {
          const typeName = mMatch[1];
          const fieldName = mMatch[2];
          fieldMap.set(fieldName, typeName);
        }
      }
      if (fieldMap.size > 0) {
        result.set(sym.id, fieldMap);
      }
    }
  }

  return result;
}

function main(): void {
  const workspaceRoot = process.argv[2];
  if (!workspaceRoot) {
    console.error("Usage: ts-node src/cli.ts <workspace-root>");
    process.exit(1);
  }

  const resolvedRoot = path.resolve(workspaceRoot);
  if (!fs.existsSync(resolvedRoot)) {
    console.error(`Directory not found: ${resolvedRoot}`);
    process.exit(1);
  }

  const dataDir = path.join(resolvedRoot, DATA_DIR_NAME);
  const store = new JsonStore(dataDir);

  console.log(`Indexing: ${resolvedRoot}`);
  const startTime = Date.now();

  const cppFiles = findCppFiles(resolvedRoot);
  console.log(`Found ${cppFiles.length} C++ files`);

  const rawSymbols: Symbol[] = [];
  const rawCalls: RawCallSite[] = [];
  const fileContents = new Map<string, string>();
  const fileRecords: FileRecord[] = [];

  for (const filePath of cppFiles) {
    const relativePath = path.relative(resolvedRoot, filePath).replace(/\\/g, "/");
    const content = fs.readFileSync(filePath, "utf-8");
    fileContents.set(relativePath, content);

    try {
      const result = parseCppFile(relativePath, content);
      rawSymbols.push(...result.symbols);
      rawCalls.push(...result.rawCalls);

      fileRecords.push({
        path: relativePath,
        contentHash: hashContent(content),
        lastIndexed: new Date().toISOString(),
        symbolCount: result.symbols.length,
      });

      console.log(`  ${relativePath}: ${result.symbols.length} symbols, ${result.rawCalls.length} raw calls`);
    } catch (err) {
      console.error(`  FAILED: ${relativePath}: ${err}`);
    }
  }

  const symbols = mergeSymbols(rawSymbols);
  const localTypes = buildLocalTypeMap(fileContents, symbols);
  const calls = resolveCallSites(rawCalls, symbols, localTypes);

  const data: IndexData = { symbols, calls, files: fileRecords };
  store.save(data);

  const elapsed = Date.now() - startTime;
  console.log(`\nDone in ${elapsed}ms`);
  console.log(`  Symbols: ${symbols.length} (merged from ${rawSymbols.length} raw)`);
  console.log(`  Calls: ${calls.length} (resolved from ${rawCalls.length} raw)`);
  console.log(`  Files: ${fileRecords.length}`);
  console.log(`  Output: ${dataDir}`);
}

main();
