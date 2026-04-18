import * as fs from "fs";
import * as path from "path";
import { performance } from "perf_hooks";
import { openStore } from "./mcp-runtime";
import { Store, MetadataFilters } from "./storage/store";
import { SEARCH_DEFAULT_LIMIT, CALLGRAPH_MAX_DEPTH } from "./constants";
import { ReferenceCategory } from "./models/responses";

interface ProfilerConfig {
  dataDir: string;
  outputPath: string;
  repeatCount: number;
  exactQualifiedName: string;
  searchQuery: string;
  impactQualifiedName: string;
  referencesQualifiedName: string;
  traceSourceQualifiedName: string;
  traceTargetQualifiedName: string;
  callersQualifiedName?: string;
  metadataFilters?: MetadataFilters;
}

interface BenchmarkSample {
  query: string;
  runMs: number;
}

function parseArgs(argv: string[]): ProfilerConfig {
  const values = new Map<string, string>();
  for (let i = 0; i < argv.length; i += 1) {
    const current = argv[i];
    if (!current.startsWith("--")) continue;
    const next = argv[i + 1];
    if (!next || next.startsWith("--")) {
      values.set(current, "true");
      continue;
    }
    values.set(current, next);
    i += 1;
  }

  const dataDir = values.get("--data-dir");
  const outputPath = values.get("--output");
  if (!dataDir || !outputPath) {
    throw new Error("Usage: ts-node src/query-profiler.ts --data-dir <.codeatlas> --output <json> [options]");
  }

  return {
    dataDir,
    outputPath,
    repeatCount: Number(values.get("--repeat") ?? "5"),
    exactQualifiedName: values.get("--exact-qualified") ?? "Gameplay::Update",
    searchQuery: values.get("--search-query") ?? "Update",
    callersQualifiedName: values.get("--callers-qualified"),
    impactQualifiedName: values.get("--impact-qualified") ?? values.get("--exact-qualified") ?? "Gameplay::Update",
    referencesQualifiedName: values.get("--references-qualified") ?? values.get("--exact-qualified") ?? "Gameplay::Update",
    traceSourceQualifiedName: values.get("--trace-source-qualified") ?? "Bootstrap",
    traceTargetQualifiedName: values.get("--trace-target-qualified") ?? "ApplyDamage",
  };
}

function buildSymbolMap(store: Store, ids: Iterable<string>) {
  const uniqueIds = Array.from(new Set(Array.from(ids)));
  return new Map(store.getSymbolsByIds(uniqueIds).map((symbol) => [symbol.id, symbol]));
}

function makeCallReferences(
  store: Store,
  calls: { callerId?: string; calleeId?: string; filePath: string; line: number }[],
  targetField: "callerId" | "calleeId",
): Array<{ symbolId: string; qualifiedName: string; filePath: string; line: number }> {
  const symbolMap = buildSymbolMap(
    store,
    calls
      .map((call) => call[targetField])
      .filter((symbolId): symbolId is string => Boolean(symbolId)),
  );

  return calls
    .map((call) => {
      const targetId = call[targetField];
      if (!targetId) return null;
      const symbol = symbolMap.get(targetId);
      if (!symbol) return null;
      return {
        symbolId: symbol.id,
        qualifiedName: symbol.qualifiedName,
        filePath: call.filePath,
        line: call.line,
      };
    })
    .filter((value): value is { symbolId: string; qualifiedName: string; filePath: string; line: number } => value !== null);
}

function profileExactLookup(store: Store, qualifiedName: string) {
  return store.getSymbolByQualifiedName(qualifiedName);
}

function profileSearch(store: Store, query: string) {
  return store.searchSymbols(query, undefined, SEARCH_DEFAULT_LIMIT);
}

function profileCallers(store: Store, qualifiedName: string) {
  const symbol = store.getSymbolByQualifiedName(qualifiedName);
  if (!symbol) return [];
  return makeCallReferences(store, store.getCallers(symbol.id), "callerId");
}

function profileReferences(store: Store, qualifiedName: string) {
  const symbol = store.getSymbolByQualifiedName(qualifiedName);
  if (!symbol) return [];
  const refs = store.getReferences(symbol.id, undefined as ReferenceCategory | undefined, undefined);
  const symbolMap = buildSymbolMap(store, refs.map((reference) => reference.sourceSymbolId));
  return refs
    .map((reference) => {
      const source = symbolMap.get(reference.sourceSymbolId);
      if (!source) return null;
      return {
        ...reference,
        sourceQualifiedName: source.qualifiedName,
      };
    })
    .filter((value) => value !== null);
}

function profileTraceCallPath(store: Store, sourceQualifiedName: string, targetQualifiedName: string, maxDepth = CALLGRAPH_MAX_DEPTH) {
  const source = store.getSymbolByQualifiedName(sourceQualifiedName);
  const target = store.getSymbolByQualifiedName(targetQualifiedName);
  if (!source || !target) {
    return { pathFound: false, steps: [] as Array<{ callerId: string; calleeId: string; filePath: string; line: number }> };
  }

  type QueueItem = { symbolId: string; depth: number; steps: Array<{ callerId: string; calleeId: string; filePath: string; line: number }> };
  const visited = new Set<string>([source.id]);
  const queue: QueueItem[] = [{ symbolId: source.id, depth: 0, steps: [] }];

  while (queue.length > 0) {
    const current = queue.shift()!;
    if (current.depth >= maxDepth) {
      continue;
    }

    for (const call of store.getCallees(current.symbolId)) {
      if (call.calleeId === target.id) {
        return {
          pathFound: true,
          steps: current.steps.concat({
            callerId: call.callerId,
            calleeId: call.calleeId,
            filePath: call.filePath,
            line: call.line,
          }),
        };
      }

      if (visited.has(call.calleeId)) {
        continue;
      }
      visited.add(call.calleeId);
      queue.push({
        symbolId: call.calleeId,
        depth: current.depth + 1,
        steps: current.steps.concat({
          callerId: call.callerId,
          calleeId: call.calleeId,
          filePath: call.filePath,
          line: call.line,
        }),
      });
    }
  }

  return { pathFound: false, steps: [] as Array<{ callerId: string; calleeId: string; filePath: string; line: number }> };
}

function profileImpact(store: Store, qualifiedName: string, maxDepth = 2) {
  const symbol = store.getSymbolByQualifiedName(qualifiedName);
  if (!symbol) {
    return { totalAffectedSymbols: 0, totalAffectedFiles: 0 };
  }

  const impactedSymbolCounts = new Map<string, number>();
  const impactedFileCounts = new Map<string, number>();
  const callerQueue = store.getCallers(symbol.id).map((call) => ({ symbolId: call.callerId, depth: 1 }));
  const calleeQueue = store.getCallees(symbol.id).map((call) => ({ symbolId: call.calleeId, depth: 1 }));
  const seenCallerSymbols = new Set<string>();
  const seenCalleeSymbols = new Set<string>();

  const bumpSymbol = (symbolId: string) => {
    if (symbolId === symbol.id) return;
    impactedSymbolCounts.set(symbolId, (impactedSymbolCounts.get(symbolId) ?? 0) + 1);
  };

  while (callerQueue.length > 0) {
    const current = callerQueue.shift()!;
    if (current.depth > maxDepth || seenCallerSymbols.has(current.symbolId)) continue;
    seenCallerSymbols.add(current.symbolId);
    bumpSymbol(current.symbolId);
    if (current.depth === maxDepth) continue;
    for (const next of store.getCallers(current.symbolId)) {
      callerQueue.push({ symbolId: next.callerId, depth: current.depth + 1 });
    }
  }

  while (calleeQueue.length > 0) {
    const current = calleeQueue.shift()!;
    if (current.depth > maxDepth || seenCalleeSymbols.has(current.symbolId)) continue;
    seenCalleeSymbols.add(current.symbolId);
    bumpSymbol(current.symbolId);
    if (current.depth === maxDepth) continue;
    for (const next of store.getCallees(current.symbolId)) {
      calleeQueue.push({ symbolId: next.calleeId, depth: current.depth + 1 });
    }
  }

  const impactedSymbols = buildSymbolMap(store, impactedSymbolCounts.keys());
  for (const impacted of impactedSymbols.values()) {
    impactedFileCounts.set(impacted.filePath, (impactedFileCounts.get(impacted.filePath) ?? 0) + 1);
  }

  return {
    totalAffectedSymbols: impactedSymbolCounts.size,
    totalAffectedFiles: impactedFileCounts.size,
  };
}

function runMeasured<T>(label: string, repeatCount: number, action: () => T) {
  const samples: BenchmarkSample[] = [];
  let lastResult: T | undefined;
  for (let i = 0; i < repeatCount; i += 1) {
    const started = performance.now();
    lastResult = action();
    samples.push({
      query: label,
      runMs: Number((performance.now() - started).toFixed(3)),
    });
  }
  const values = samples.map((sample) => sample.runMs);
  return {
    label,
    repeatCount,
    minMs: Math.min(...values),
    maxMs: Math.max(...values),
    avgMs: Number((values.reduce((sum, value) => sum + value, 0) / values.length).toFixed(3)),
    lastResult,
    samples,
  };
}

function main() {
  const config = parseArgs(process.argv.slice(2));
  const store = openStore(config.dataDir);

  try {
    const result = {
      schemaVersion: 1,
      recordedAt: new Date().toISOString(),
      dataDir: path.resolve(config.dataDir),
      repeatCount: config.repeatCount,
      profiles: [
        runMeasured(`exact:${config.exactQualifiedName}`, config.repeatCount, () =>
          profileExactLookup(store, config.exactQualifiedName)),
        runMeasured(`search:${config.searchQuery}`, config.repeatCount, () =>
          profileSearch(store, config.searchQuery)),
        runMeasured(`callers:${config.callersQualifiedName ?? config.exactQualifiedName}`, config.repeatCount, () =>
          profileCallers(store, config.callersQualifiedName ?? config.exactQualifiedName)),
        runMeasured(`references:${config.referencesQualifiedName}`, config.repeatCount, () =>
          profileReferences(store, config.referencesQualifiedName)),
        runMeasured(`trace:${config.traceSourceQualifiedName}->${config.traceTargetQualifiedName}`, config.repeatCount, () =>
          profileTraceCallPath(store, config.traceSourceQualifiedName, config.traceTargetQualifiedName)),
        runMeasured(`impact:${config.impactQualifiedName}`, config.repeatCount, () =>
          profileImpact(store, config.impactQualifiedName)),
      ],
    };

    fs.mkdirSync(path.dirname(config.outputPath), { recursive: true });
    fs.writeFileSync(config.outputPath, JSON.stringify(result, null, 2));
    console.log(`Query profile written to ${config.outputPath}`);
  } finally {
    if ("close" in store && typeof (store as { close?: () => void }).close === "function") {
      (store as { close: () => void }).close();
    }
  }
}

main();
