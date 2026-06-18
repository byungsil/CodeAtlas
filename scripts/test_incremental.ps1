param()

$idx     = "F:\dev\CodeAtlas\indexer\target\release\codeatlas-indexer.exe"
$samples = "F:\dev\CodeAtlas\samples\incremental"
$tmpBase = "$env:TEMP\codeatlas_incremental_test"
$sqlite  = "D:\sqlite-tools\sqlite3.exe"

function Query-Symbols($dbPath) {
    & $sqlite -csv $dbPath "SELECT name FROM symbols WHERE type IN ('function','method') ORDER BY name;"
}

function Run-Scenario($name, $presentAfter, $absentAfter) {
    Write-Host "Testing: $name" -NoNewline

    $tmpDir  = "$tmpBase\$name"
    $dataDir = "$tmpDir\.codeatlas"

    if (Test-Path $tmpDir) { Remove-Item -Recurse -Force $tmpDir }
    New-Item -ItemType Directory -Force $tmpDir | Out-Null

    # 1. before 복사
    Copy-Item "$samples\$name\before\*" $tmpDir -Recurse -Force

    # 2. full index
    & $idx $tmpDir --full --workspace-name "test_$name" 2>&1 | Out-Null

    $dbAfterFull = Get-ChildItem $dataDir -Filter "*.db" -ErrorAction SilentlyContinue |
                   Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (-not $dbAfterFull) {
        Write-Host "  -> FAIL: no DB after full index"
        return
    }

    # 3. after 상태 적용 (삭제 + 추가/수정)
    $beforeRoot = "$samples\$name\before"
    $afterRoot  = "$samples\$name\after"
    $beforeRel  = Get-ChildItem $beforeRoot -Recurse -File |
                  ForEach-Object { $_.FullName.Substring($beforeRoot.Length + 1) }
    $afterRel   = Get-ChildItem $afterRoot  -Recurse -File |
                  ForEach-Object { $_.FullName.Substring($afterRoot.Length  + 1) }

    foreach ($f in $beforeRel) {
        if ($afterRel -notcontains $f) {
            $t = Join-Path $tmpDir $f
            if (Test-Path $t) { Remove-Item -Force $t }
        }
    }
    foreach ($f in $afterRel) {
        $dst    = Join-Path $tmpDir $f
        $dstDir = Split-Path $dst -Parent
        if (-not (Test-Path $dstDir)) { New-Item -ItemType Directory -Force $dstDir | Out-Null }
        Copy-Item (Join-Path $afterRoot $f) $dst -Force
    }

    # 4. incremental index
    & $idx $tmpDir --workspace-name "test_$name" 2>&1 | Out-Null

    $dbAfterInc = Get-ChildItem $dataDir -Filter "*.db" -ErrorAction SilentlyContinue |
                  Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (-not $dbAfterInc) {
        Write-Host "  -> FAIL: no DB after incremental index"
        return
    }

    $syms = Query-Symbols $dbAfterInc.FullName

    # 5. 검증
    $failures = @()
    foreach ($sym in $presentAfter) {
        if ($syms -notcontains $sym) { $failures += "MISSING '$sym'" }
    }
    foreach ($sym in $absentAfter) {
        if ($syms -contains $sym) { $failures += "SHOULD_BE_GONE '$sym'" }
    }

    if ($failures.Count -eq 0) {
        Write-Host "  -> PASS"
    } else {
        Write-Host "  -> FAIL: $($failures -join ', ')"
        Write-Host "     symbols in DB: $($syms -join ', ')"
    }
}

Write-Host ""
Write-Host "=== Incremental Indexing Correctness Tests ==="
Write-Host ""

Run-Scenario "add_file"              @("Added")           @()
Run-Scenario "delete_file"           @()                  @("Gone")
Run-Scenario "edit_symbol_rename"    @("Refresh")         @("Update")
Run-Scenario "header_comment_change" @("Stable")          @()
Run-Scenario "mass_churn"            @("B2","C")          @("A","B")
Run-Scenario "rename_move"           @("Helper")          @()

Write-Host ""
Write-Host "=== Done ==="
