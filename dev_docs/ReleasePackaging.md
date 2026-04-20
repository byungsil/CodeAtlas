# Release Packaging

## Purpose

This document describes the current developer workflow for producing the Windows distribution bundle for CodeAtlas.

Use this when you need to:

- build a release bundle from the repository
- verify the generated bundle before sharing it
- understand what is intentionally included or excluded from the distribution

## Current Packaging Model

The Windows release bundle contains:

- the Rust indexer binary
- the compiled Node server bundle
- dashboard/MCP launch scripts
- setup scripts that install external runtime prerequisites on the target machine

The bundle does **not** include a bundled Node runtime.

Instead:

- the user runs `setup-prereqs.cmd`
- that script ensures Node.js is available
- it then installs the packaged server runtime dependencies inside the extracted bundle

## Build Command

From the repository root:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\package-release.ps1
```

Optional:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\package-release.ps1 -SkipZip
```

Use `-SkipZip` when you only want to refresh the staging folder during local iteration.

## Outputs

Default output location:

- staging folder: `release/CodeAtlas-windows-x64-v<version>`
- zip archive: `release/CodeAtlas-windows-x64-v<version>.zip`

The version is read from `indexer/Cargo.toml`.

## What The Packaging Script Does

`scripts/package-release.ps1` currently:

1. builds the Rust indexer in release mode
2. compiles the server TypeScript bundle
3. creates a clean staging directory
4. copies release assets into the staging directory
5. optionally creates the final zip archive

Included in the bundle:

- `bin/codeatlas-indexer.exe`
- `server/dist/...`
- `server/public/...`
- `server/package.json`
- `server/package-lock.json`
- `setup-prereqs.cmd`
- `setup-prereqs.ps1`
- `index-workspace.cmd`
- `watch-workspace.cmd`
- `start-dashboard.cmd`
- `start-mcp.cmd`
- `mcp-config.example.json`
- `README.txt`

Not included in the bundle:

- `.tools/node`
- preinstalled `server/node_modules`
- workspace-local `.codeatlas` data
- temporary smoke-test folders

## Runtime Setup In The Bundle

After extracting the bundle, the expected runtime preparation flow is:

```powershell
setup-prereqs.cmd
```

That script:

1. ensures Node.js is installed
2. runs `npm ci --omit=dev` inside the extracted `server` folder

This step is required before starting the dashboard or MCP server from the extracted bundle.

## Expected User Flow

Typical end-user flow:

1. extract the zip
2. run `setup-prereqs.cmd`
3. run `index-workspace.cmd <workspace> --full`
4. register MCP using `mcp-config.example.json`

Notes:

- MCP is the primary runtime entry point
- watcher is expected to run by default when MCP starts
- dashboard is optional

## Smoke Test Checklist

Before publishing a bundle, verify at least the following on an extracted copy of the release:

1. Run:

```powershell
setup-prereqs.cmd
```

2. Index a small workspace:

```powershell
index-workspace.cmd E:\Dev\CodeAtlas\samples\ambiguity --full
```

3. Confirm that:

- `<workspace>\.codeatlas\index.db` is created
- indexing completes successfully

4. Start the dashboard manually if needed:

```powershell
start-dashboard.cmd E:\Dev\CodeAtlas\samples\ambiguity
```

5. Verify:

- `http://localhost:<port>/dashboard/` opens
- `/dashboard/api/overview` returns valid data

6. Smoke-test MCP startup:

```powershell
start-mcp.cmd E:\Dev\CodeAtlas\samples\ambiguity
```

At minimum, confirm it does not fail immediately during startup.

## Known Packaging Constraints

- The bundle currently targets Windows.
- The release assumes the target machine can install Node.js externally.
- Native Node modules are installed on the target machine during `setup-prereqs.cmd`; they are not copied from the build machine.
- If the dashboard fails with a native module load error, re-run `setup-prereqs.cmd` and confirm it completed inside the extracted bundle.

## Source Files

Primary packaging files:

- `scripts/package-release.ps1`
- `scripts/release/README.txt`
- `scripts/release/setup-prereqs.ps1`
- `scripts/release/setup-prereqs.cmd`
- `scripts/release/index-workspace.cmd`
- `scripts/release/watch-workspace.cmd`
- `scripts/release/start-dashboard.cmd`
- `scripts/release/start-mcp.cmd`
- `scripts/release/mcp-config.example.json`
