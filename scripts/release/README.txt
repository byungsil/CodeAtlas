CodeAtlas Windows Distribution
==============================

This package contains:
- Rust indexer binary
- Prebuilt Node.js server bundle
- Runtime setup scripts for Node.js and server dependencies
- MCP-ready server bundle and optional convenience launch scripts

Folder layout
-------------
- bin\codeatlas-indexer.exe
- server\dist\...
- server\public\...
- server\package.json
- server\package-lock.json
- setup-prereqs.cmd
- setup-prereqs.ps1
- mcp-config.example.json
- index-workspace.cmd
- watch-workspace.cmd
- start-dashboard.cmd
- start-mcp.cmd

Quick start
-----------
1. Index a workspace
   index-workspace.cmd E:\Dev\your-project --full --workspace-name your-project

2. Install Node.js and server runtime dependencies
   setup-prereqs.cmd

3. Register MCP using mcp-config.example.json as a template

4. Let the MCP server start watcher automatically

Command usage
-------------
index-workspace.cmd <workspace-root> [additional indexer args...]
setup-prereqs.cmd

Advanced / optional manual commands
-----------------------------------
- start-dashboard.cmd <workspace-root>
- watch-workspace.cmd <workspace-root> [additional watcher args...]
- start-mcp.cmd <workspace-root>

Notes
-----
- The dashboard and MCP server resolve the active SQLite generation from <workspace-root>\.codeatlas\current-db.json
- Use one dashboard process per workspace. The dashboard shows the stored workspace name from DB metadata.
- If node is not installed yet, run setup-prereqs.cmd first.
- For normal agent use, start MCP from your client configuration rather than double-clicking start-mcp.cmd.
- The MCP server starts the watcher by default. Set CODEATLAS_WATCHER=false only if you intentionally want static reads.
- start-dashboard.cmd is optional and mainly useful when you want the web dashboard without launching it from MCP runtime options.
- watch-workspace.cmd is only for cases where you intentionally want a standalone watcher outside MCP operation.
- If you want a different port, set CODEATLAS_PORT before starting:
  set CODEATLAS_PORT=3100
