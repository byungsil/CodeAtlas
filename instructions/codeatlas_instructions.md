---
applyTo: "**"
description: "Use when: C++ symbol analysis, call hierarchy, caller/callee tracing, references, overrides, propagation, impact analysis, gameplay architecture investigation."
---

# GitHub Copilot Instructions for CodeAtlas MCP Tools

## Purpose
CodeAtlas provides symbol-aware analysis for the C++ codebase. Use it when the task depends on relationships between functions, classes, methods, references, overrides, callers, callees, or change impact.

Use ea-context-foundation for broad semantic discovery. Use CodeAtlas for precise C++ symbol-level investigation after a relevant symbol, class, file, or subsystem has been identified. If the user explicitly asks for call hierarchy, references, overrides, impact analysis, or a specific C++ symbol, activate CodeAtlas immediately.

## When To Use CodeAtlas

Use CodeAtlas for tasks involving:

- Call hierarchy: who calls this function, what this function calls, recursive caller/callee paths.
- Symbol lookup: where a function, class, method, enum, or member is defined.
- References: all known uses of a symbol across the workspace.
- Class structure: class members, inheritance, overrides, and implementations.
- Impact analysis: what could be affected by changing a function, class, enum, flag, or data flow.
- Propagation analysis: how a value, flag, decision, or state flows through gameplay code.
- Gameplay architecture investigation: tracing AI, behavior, shot, pass, dribble, trait, physics, and animation code paths.

Prefer CodeAtlas over plain text search when the question is about relationships rather than keyword occurrence.

## Deferred Tool Activation

CodeAtlas tools may be deferred. Before calling a CodeAtlas tool that is not already loaded, use `tool_search` with a capability-oriented query such as:

- `C++ symbol lookup and references`
- `C++ call hierarchy callers callees`
- `CodeAtlas impact analysis`
- `CodeAtlas symbol propagation analysis`
- `list C++ file symbols and class members`
- `method overload and override analysis`

After `tool_search` returns the relevant tool definitions, use the loaded CodeAtlas tool directly. Do not repeatedly call `tool_search` for a tool family that has already been loaded in the current session.

## Available Tool Families

Use the most specific available CodeAtlas capability:

- Symbol lookup and references: locate definitions and references for known symbols.
- Call graph analysis: find callers, callees, recursive callers, and call paths.
- File symbol listing: inspect the symbols declared or defined in a file.
- C++ symbol analysis: inspect classes, functions, methods, members, and type relationships.
- Override and overload analysis: inspect virtual dispatch, overrides, overload sets, and implementations.
- Symbol propagation analysis: trace how a symbol, value, flag, or state influences downstream code.
- Impact analysis: estimate affected code paths before changing shared gameplay behavior.

## Workflow

1. If the task is broad or the relevant symbol is unknown, start with ea-context-foundation or local text search to identify candidate files and symbols.
2. Once a candidate C++ symbol, class, function, or file is known, use CodeAtlas to inspect relationships.
3. Verify CodeAtlas results locally before citing or editing code. Read the actual files and confirm line numbers, call sites, symbol names, and control flow.
4. Treat recovered, macro-sensitive, or inferred CodeAtlas edges as leads, not facts. Confirm them with local source reads.
5. Use local tools such as `read_file`, `grep_search`, `file_search`, or language-server usages to supplement and verify the graph result.
6. For non-trivial gameplay changes, use CodeAtlas impact or propagation analysis before proposing or implementing edits.

## Coordination With ea-context-foundation

The two MCP systems have different strengths:

- ea-context-foundation: broad natural-language discovery, subsystem mapping, finding likely files.
- CodeAtlas: precise C++ symbol relationships, call graph, references, overrides, and impact.

If both apply, use ea-context-foundation first for broad discovery, then CodeAtlas for symbol-level verification and relationship analysis. If the user asks directly about a known symbol or call hierarchy, CodeAtlas can be the first tool after activation.

## Required Verification

Never rely on CodeAtlas output alone for final conclusions or code edits. Always verify locally before reporting:

- The file exists in the workspace.
- The symbol still exists under the reported name.
- The cited call site or reference is present in the current file content.
- The reported relationship is valid in the local source.
- Any line numbers are current before linking or describing code.

If CodeAtlas returns no result, do not conclude the symbol is absent until you also check with local search.

## Reporting Guidance

When CodeAtlas materially informs the answer, briefly state what relationship was checked, for example:

- `Checked callers of Foo::Bar with CodeAtlas, then verified the call sites locally.`
- `Used CodeAtlas impact analysis for TraitFlag propagation and confirmed the relevant files locally.`

When referencing code in the final response, link to local workspace files rather than reporting raw MCP paths or stale line numbers.

## Constraints

- Do not build or compile only to validate CodeAtlas findings unless the user explicitly asks.
- Do not perform source control operations.
- Do not edit code based solely on inferred call graph data.
- Prefer minimal, targeted follow-up searches over broad repeated scans once CodeAtlas has identified the relevant symbol graph.