/**
 * CodeAtlas Setup Wizard - Frontend Controller
 * Manages step navigation, tool installation, and build processes.
 */

// ==================== State ====================
let currentStep = 0;
const totalSteps = 7; // 0-6 (welcome, prereqs, indexer, server, workspace, indexing, complete)
const setupData = {
  tools: {},
  indexerBuilt: false,
  serverInstalled: false,
  workspacePath: '',
  config: {},
  selectedLangs: new Set()
};

// Supported languages and their extensions
const LANGUAGES = [
  { name: 'C/C++', key: 'cpp', icon: '🔧', extensions: ['c', 'cpp', 'h', 'hpp', 'cc', 'cxx', 'inl', 'inc'] },
  { name: 'Python', key: 'python', icon: '🐍', extensions: ['py'] },
  { name: 'TypeScript/TSX', key: 'typescript', icon: '📘', extensions: ['ts', 'tsx'] },
  { name: 'Rust', key: 'rust', icon: '🦀', extensions: ['rs'] },
  { name: 'Lua', key: 'lua', icon: '🌙', extensions: ['lua'] }
];

// Build/install state tracking
let isBuildingIndexer = false;
let isInstallingServer = false;
let isIndexing = false;

// Log state
let logEntries = [];
const MAX_LOG_DISPLAY = 200;

// ==================== Step Navigation ====================

function showStep(stepIndex) {
  // Hide all steps
  document.querySelectorAll('.wizard-step').forEach(el => el.classList.remove('active'));
  
  // Show target step
  const stepMap = ['welcome', 'prereqs', 'indexer', 'server', 'workspace', 'indexing', 'complete'];
  const targetId = `step-${stepMap[stepIndex]}`;
  document.getElementById(targetId).classList.add('active');

  // Update progress indicators
  updateProgress(stepIndex);

  // Update footer buttons
  updateFooter(stepIndex);

  // Auto-run step-specific logic
  if (stepIndex === 5) buildLangGrid();

  currentStep = stepIndex;

  // Auto-run step-specific logic
  if (stepIndex === 1) runPrereqCheck();
}

async function nextStep() {
  // Prevent navigation during build/install
  if (isBuildingIndexer || isInstallingServer) {
    addLogEntry('WARN', 'NAV', 'Cannot navigate while build/install is in progress');
    return;
  }
  
  if (currentStep < totalSteps - 1) {
    showStep(currentStep + 1);
  }
}

async function handleNextStep() {
  // Create data directory before moving to next step from workspace config (step 4)
  if (currentStep === 4 && setupData.workspacePath) {
    const dataDir = document.getElementById('dataDirs').value.trim();
    if (dataDir) {
      addLogEntry('INFO', 'FS', `Creating data directory: ${dataDir}`);
      try {
        await window.codeatlas.createDirectory(dataDir);
        addLogEntry('INFO', 'FS', `Data directory created successfully`);
      } catch (err) {
        addLogEntry('WARN', 'FS', `Failed to create data directory: ${err.message}`);
        // Continue anyway - user can create manually
      }
    }
  }

  // Save indexing config before moving to complete step (step 5 -> 6)
  if (currentStep === 5) {
    const allExts = [];
    for (const lang of LANGUAGES) {
      if (setupData.selectedLangs.has(lang.key)) {
        allExts.push(...lang.extensions);
      }
    }
    addLogEntry('INFO', 'INDEXING', `Saving indexing config: ${allExts.length} extensions (${setupData.selectedLangs.size} languages)`);
  }
  
  nextStep();
}

function goToStep(stepIndex) {
  if (stepIndex >= 0 && stepIndex < totalSteps) {
    showStep(stepIndex);
  }
}

function updateProgress(activeStep) {
  const items = document.querySelectorAll('.step-item');
  const connectors = document.querySelectorAll('.step-connector');

  items.forEach((item, i) => {
    item.classList.remove('active', 'completed');
    if (i === activeStep) item.classList.add('active');
    else if (i < activeStep) item.classList.add('completed');
  });

  connectors.forEach((conn, i) => {
    conn.classList.toggle('active', i < activeStep);
  });
}

function updateFooter(stepIndex) {
  const btnBack = document.getElementById('btnBack');
  const btnNext = document.getElementById('btnNext');

  // Back button visibility
  if (stepIndex === 0) {
    btnBack.classList.add('hidden');
  } else {
    btnBack.classList.remove('hidden');
  }

  // Next/Finish button logic
  if (stepIndex === totalSteps - 1) {
    // Complete step
    btnNext.textContent = '닫기';
    btnNext.onclick = () => window.close();
    btnNext.disabled = false;
  } else if (stepIndex === 0) {
    // Welcome step
    btnNext.textContent = '시작하기 →';
    btnNext.onclick = nextStep;
    btnNext.disabled = false;
  } else if (stepIndex === 1) {
    // Prereqs step - check if all tools are installed
    const allInstalled = Object.values(setupData.tools).every(t => t.installed);
    btnNext.textContent = allInstalled ? '다음 →' : '완료';
    btnNext.onclick = nextStep;
    btnNext.disabled = false;
  } else if (stepIndex === 2) {
    // Indexer build step - disable until build completes
    btnNext.textContent = setupData.indexerBuilt ? '다음 →' : '다음 →';
    btnNext.onclick = nextStep;
    btnNext.disabled = !setupData.indexerBuilt || isBuildingIndexer;
  } else if (stepIndex === 3) {
    // Server install step - disable until install completes
    btnNext.textContent = setupData.serverInstalled ? '다음 →' : '다음 →';
    btnNext.onclick = nextStep;
    btnNext.disabled = !setupData.serverInstalled || isInstallingServer;
  } else if (stepIndex === 4) {
    // Workspace config step - create data dir on next
    btnNext.textContent = '다음 →';
    btnNext.onclick = handleNextStep;
    btnNext.disabled = false;
  } else if (stepIndex === 5) {
    // Indexing config step - disable until indexing completes
    btnNext.textContent = setupData.indexingDone ? '다음 →' : '다음 →';
    btnNext.onclick = nextStep;
    btnNext.disabled = !setupData.indexingDone || isIndexing;
  } else if (isBuildingIndexer || isInstallingServer) {
    // During any build/install operation
    btnNext.textContent = '작업 중...';
    btnNext.disabled = true;
  } else {
    // Default: all other steps
    btnNext.textContent = '다음 →';
    btnNext.onclick = nextStep;
    btnNext.disabled = false;
  }
}

// ==================== Logging System ====================

function addLogEntry(level, step, message) {
  const entry = { level, step, message, timestamp: new Date().toISOString() };
  logEntries.push(entry);
  
  // Keep only recent entries in memory
  if (logEntries.length > MAX_LOG_DISPLAY) {
    logEntries = logEntries.slice(-MAX_LOG_DISPLAY);
  }

  // Update UI if logs panel is visible
  updateLogsPanel();
}

function updateLogsPanel() {
  const logsContent = document.getElementById('logsContent');
  if (!logsContent) return;

  let html = '';
  for (const entry of logEntries) {
    const timeStr = new Date(entry.timestamp).toLocaleTimeString();
    const levelClass = `log-${entry.level.toLowerCase()}`;
    const stepTag = entry.step ? `<span class="log-step">[${entry.step}]</span>` : '';
    
    html += `<div class="log-entry ${levelClass}">`;
    html += `<span class="log-time">${timeStr}</span> `;
    html += `${stepTag}`;
    html += `<span class="log-message">${escapeHtml(entry.message)}</span>`;
    html += `</div>`;
  }

  logsContent.innerHTML = html || '<div class="log-entry log-info"><span class="log-time">--:--:--</span> <span class="log-message">로그가 아직 없습니다...</span></div>';
  
  // Auto-scroll to bottom
  logsContent.scrollTop = logsContent.scrollHeight;
}

function escapeHtml(text) {
  const div = document.createElement('div');
  div.textContent = text;
  return div.innerHTML;
}

async function loadLogs() {
  try {
    const recentLogs = await window.codeatlas.getRecentLogs(100);
    logEntries = recentLogs.map(log => ({
      level: log.level,
      step: log.step || '',
      message: log.message,
      timestamp: log.timestamp || new Date().toISOString()
    }));
    updateLogsPanel();
  } catch (err) {
    console.error('Failed to load logs:', err);
  }
}

function clearLogs() {
  logEntries = [];
  updateLogsPanel();
  window.codeatlas.clearLogFile().catch(err => console.error('Failed to clear logs:', err));
}

// ==================== Step 1: Prerequisites Check ====================

async function runPrereqCheck() {
  addLogEntry('INFO', 'PREREQS', 'Starting prerequisites check...');
  
  const prereqs = [
    { name: 'node', wingetId: 'OpenJS.NodeJS.LTS', displayName: 'Node.js LTS' },
    { name: 'npm', wingetId: 'OpenJS.NodeJS.LTS', displayName: 'npm (included with Node.js)' },
    { name: 'cargo', wingetId: 'Rustlang.Rustup', displayName: 'Rust toolchain' }
  ];

  for (const prereq of prereqs) {
    try {
      addLogEntry('INFO', 'CHECK', `Checking for ${prereq.name}...`);
      const result = await window.codeatlas.checkCommand(prereq.name);
      setupData.tools[prereq.name] = result;

      if (result.exists) {
        addLogEntry('INFO', 'CHECK', `${prereq.name} is available: ${result.version || 'installed'}`);
      } else {
        addLogEntry('WARN', 'CHECK', `${prereq.name} not found - installation may be needed`);
      }

      updatePrereqUI(prereq.name, result);
    } catch (err) {
      addLogEntry('ERROR', 'CHECK', `Failed to check ${prereq.name}: ${err.message}`);
    }
  }

  // Check if any need installation
  const needsInstall = prereqs.some(p => !setupData.tools[p.name].exists);
  
  const statusEl = document.getElementById('prereqStatus');
  if (needsInstall) {
    statusEl.className = 'status-message info';
    statusEl.textContent = '일부 도구가 필요합니다. 각 도구 옆의 "설치하기" 버튼을 클릭하세요.';
    addLogEntry('INFO', 'PREREQS', 'Some tools need installation');
  } else {
    statusEl.className = 'status-message success';
    statusEl.textContent = '✅ 모든 필수 도구가 설치되어 있습니다!';
    addLogEntry('INFO', 'PREREQS', 'All prerequisites are installed');
  }
}

function updatePrereqUI(toolName, result) {
  const item = document.querySelector(`.prereq-item[data-tool="${toolName}"]`);
  const versionEl = document.getElementById(`version-${toolName}`);
  const installBtn = document.getElementById(`install-${toolName}`);

  if (!item || !versionEl) return;

  if (result.exists && result.version) {
    item.classList.add('installed');
    versionEl.textContent = `✅ ${result.version}`;
    installBtn.classList.add('hidden');
  } else if (result.exists) {
    item.classList.add('installed');
    versionEl.textContent = '✅ 설치됨';
    installBtn.classList.add('hidden');
  } else {
    item.classList.remove('installed', 'installing');
    versionEl.textContent = '❌ 미설치';
    installBtn.classList.remove('hidden');
  }
}

async function installTool(toolName) {
  const toolMap = {
    node: { wingetId: 'OpenJS.NodeJS.LTS', displayName: 'Node.js LTS' },
    npm: { wingetId: 'OpenJS.NodeJS.LTS', displayName: 'npm (included with Node.js)' },
    cargo: { wingetId: 'Rustlang.Rustup', displayName: 'Rust toolchain' }
  };

  const config = toolMap[toolName];
  if (!config) return;

  addLogEntry('INFO', 'INSTALL', `Starting installation of ${config.displayName}...`);

  const item = document.querySelector(`.prereq-item[data-tool="${toolName}"]`);
  const installBtn = document.getElementById(`install-${toolName}`);
  const versionEl = document.getElementById(`version-${toolName}`);

  // Mark as installing
  item.classList.add('installing');
  item.classList.remove('installed', 'missing');
  installBtn.disabled = true;
  installBtn.textContent = '설치 중...';

  try {
    const result = await window.codeatlas.installWinget(config.wingetId, config.displayName);
    
    if (result.success) {
      item.classList.remove('installing');
      item.classList.add('installed');
      versionEl.textContent = '✅ 설치 완료! (다시 검사하려면 페이지 새로고침)';
      
      addLogEntry('INFO', 'INSTALL', `${config.displayName} installed successfully`);
      
      // Update status
      const statusEl = document.getElementById('prereqStatus');
      statusEl.className = 'status-message success';
      statusEl.textContent = `✅ ${config.displayName}이(가) 성공적으로 설치되었습니다.`;
    } else {
      item.classList.remove('installing');
      item.classList.add('missing');
      versionEl.textContent = '❌ 설치 실패';
      
      addLogEntry('ERROR', 'INSTALL', `${config.displayName} installation failed: ${result.output?.substring(0, 200) || 'unknown error'}`);
      
      const statusEl = document.getElementById('prereqStatus');
      statusEl.className = 'status-message error';
      statusEl.textContent = `❌ ${config.displayName} 설치가 실패했습니다. 수동으로 설치해주세요.`;
    }
  } catch (err) {
    item.classList.remove('installing');
    item.classList.add('missing');
    versionEl.textContent = '❌ 오류';
    
    addLogEntry('ERROR', 'INSTALL', `Installation error for ${config.displayName}: ${err.message}`);
    
    const statusEl = document.getElementById('prereqStatus');
    statusEl.className = 'status-message error';
    statusEl.textContent = `❌ 설치 중 오류 발생: ${err.message}`;
  }

  installBtn.disabled = false;
}

// ==================== Step 2: Build Indexer ====================

async function buildIndexer() {
  const outputEl = document.getElementById('indexerOutput');
  const statusEl = document.getElementById('indexerStatus');
  const btn = event.target;

  addLogEntry('INFO', 'BUILD', 'Starting Rust indexer build...');
  
  isBuildingIndexer = true; // Disable next button
  btn.disabled = true;
  btn.textContent = '빌드 중...';
  outputEl.textContent = '';

  let timerInterval;
  const startTime = Date.now();

  try {
    // Listen for command output
    window.codeatlas.onCommandOutput((data) => {
      if (data.type === 'stdout' || data.type === 'stderr') {
        outputEl.textContent += data.text;
        outputEl.scrollTop = outputEl.scrollHeight;
        
        // Also log to logs panel
        const prefix = data.type === 'stderr' ? '[STDERR] ' : '';
        addLogEntry(data.type === 'stderr' ? 'WARN' : 'INFO', 'BUILD', prefix + data.text.trim());
      }
    });

    const repoRoot = await window.codeatlas.getRepoRoot();
    const indexerPath = await window.codeatlas.joinPaths(repoRoot, 'indexer');
    
    statusEl.className = 'status-message info';
    const baseMessage = '🔨 Rust 인덱서를 빌드하고 있습니다...';
    statusEl.textContent = baseMessage;
    addLogEntry('INFO', 'BUILD', `Building in: ${indexerPath}`);

    timerInterval = setInterval(() => {
      const elapsed = Math.floor((Date.now() - startTime) / 1000);
      const mins = Math.floor(elapsed / 60);
      const secs = elapsed % 60;
      const timeStr = `${mins}:${secs.toString().padStart(2, '0')}`;
      statusEl.textContent = `${baseMessage} (${timeStr})`;
    }, 1000);

    const result = await window.codeatlas.runCommand('cargo', ['build', '--release'], indexerPath);

    if (result.success) {
      setupData.indexerBuilt = true;
      statusEl.className = 'status-message success';
      statusEl.textContent = '✅ Rust 인덱서 빌드 완료!';
      outputEl.textContent += '\n\n✅ 빌드 성공!\n';
      addLogEntry('INFO', 'BUILD', 'Rust indexer build completed successfully');
    } else {
      statusEl.className = 'status-message error';
      statusEl.textContent = `❌ 빌드 실패: ${result.stderr || result.stdout}`;
      outputEl.textContent += `\n\n❌ 빌드 실패\n`;
      addLogEntry('ERROR', 'BUILD', `Build failed: ${result.stderr || result.stdout}`);
    }
  } catch (err) {
    statusEl.className = 'status-message error';
    statusEl.textContent = `❌ 오류: ${err.message}`;
    addLogEntry('ERROR', 'BUILD', `Build error: ${err.message}`);
  } finally {
    if (timerInterval) clearInterval(timerInterval);
    btn.disabled = false;
    btn.textContent = '빌드 시작';
    isBuildingIndexer = false;
    updateFooter(currentStep); // Re-evaluate next button state
  }
}

// ==================== Step 3: Server Setup ====================

async function installServer() {
  const outputEl = document.getElementById('serverOutput');
  const statusEl = document.getElementById('serverStatus');
  const btn = event.target;

  addLogEntry('INFO', 'SERVER', 'Starting server setup...');
  
  isInstallingServer = true; // Disable next button
  btn.disabled = true;
  btn.textContent = '설치 중...';
  outputEl.textContent = '';

  let timerInterval;
  const startTime = Date.now();

  try {
    // Listen for command output
    window.codeatlas.onCommandOutput((data) => {
      if (data.type === 'stdout' || data.type === 'stderr') {
        outputEl.textContent += data.text;
        outputEl.scrollTop = outputEl.scrollHeight;
        
        const prefix = data.type === 'stderr' ? '[STDERR] ' : '';
        addLogEntry(data.type === 'stderr' ? 'WARN' : 'INFO', 'SERVER', prefix + data.text.trim());
      }
    });

    const repoRoot = await window.codeatlas.getRepoRoot();
    const serverPath = await window.codeatlas.joinPaths(repoRoot, 'server');

    statusEl.className = 'status-message info';
    const baseMessage = '📦 npm 의존성을 설치하고 있습니다...';
    statusEl.textContent = baseMessage;

    timerInterval = setInterval(() => {
      const elapsed = Math.floor((Date.now() - startTime) / 1000);
      const mins = Math.floor(elapsed / 60);
      const secs = elapsed % 60;
      const timeStr = `${mins}:${secs.toString().padStart(2, '0')}`;
      statusEl.textContent = `${baseMessage} (${timeStr})`;
    }, 1000);

    addLogEntry('INFO', 'SERVER', `Installing in: ${serverPath}`);

    // Step 1: npm install
    const installResult = await window.codeatlas.runCommand('npm', ['install'], serverPath);
    
    if (!installResult.success) {
      statusEl.className = 'status-message error';
      statusEl.textContent = `❌ npm install 실패: ${installResult.stderr || installResult.stdout}`;
      btn.disabled = false;
      btn.textContent = '설치 시작';
      addLogEntry('ERROR', 'SERVER', `npm install failed: ${installResult.stderr || installResult.stdout}`);
      return;
    }

    addLogEntry('INFO', 'SERVER', 'npm install completed successfully');

    // Step 2: Build TypeScript
    statusEl.className = 'status-message info';
    statusEl.textContent = '🔨 TypeScript를 컴파일하고 있습니다...';

    const buildResult = await window.codeatlas.runCommand('npx', ['tsc'], serverPath);

    if (buildResult.success) {
      setupData.serverInstalled = true;
      statusEl.className = 'status-message success';
      statusEl.textContent = '✅ 서버 설치 및 컴파일 완료!';
      outputEl.textContent += '\n\n✅ 서버 준비 완료!\n';
      addLogEntry('INFO', 'SERVER', 'Server build completed successfully');
    } else {
      // Build might fail due to TypeScript errors, but npm install succeeded
      setupData.serverInstalled = true;
      statusEl.className = 'status-message info';
      statusEl.textContent = '⚠️ npm 설치는 완료되었으나 컴파일에 문제가 있습니다. (계속 진행 가능)';
      addLogEntry('WARN', 'SERVER', `TypeScript compilation had issues: ${buildResult.stderr || buildResult.stdout}`);
    }
  } catch (err) {
    statusEl.className = 'status-message error';
    statusEl.textContent = `❌ 오류: ${err.message}`;
    addLogEntry('ERROR', 'SERVER', `Server setup error: ${err.message}`);
  } finally {
    if (timerInterval) clearInterval(timerInterval);
    btn.disabled = false;
    btn.textContent = '설치 시작';
    isInstallingServer = false;
    updateFooter(currentStep); // Re-evaluate next button state
  }
}

// ==================== Step 5: Indexing Configuration ====================

function buildLangGrid() {
  const grid = document.getElementById('langGrid');
  if (!grid) return;

  let html = '';
  for (const lang of LANGUAGES) {
    const selected = setupData.selectedLangs.has(lang.key);
    html += `
      <div class="lang-card ${selected ? 'selected' : ''}" data-lang="${lang.key}" onclick="toggleLang('${lang.key}')">
        <span class="lang-icon">${lang.icon}</span>
        <div class="lang-info">
          <span class="lang-name">${lang.name}</span>
          <span class="lang-exts">.${lang.extensions.join(', .')}</span>
        </div>
        <span class="lang-check ${selected ? 'checked' : ''}">✓</span>
      </div>`;
  }
  grid.innerHTML = html;
  updateExtensionTags();
}

function toggleLang(key) {
  if (setupData.selectedLangs.has(key)) {
    setupData.selectedLangs.delete(key);
  } else {
    setupData.selectedLangs.add(key);
  }
  buildLangGrid();
}

function selectAllLangs() {
  for (const lang of LANGUAGES) {
    setupData.selectedLangs.add(lang.key);
  }
  buildLangGrid();
}

function deselectAllLangs() {
  setupData.selectedLangs.clear();
  buildLangGrid();
}

function updateExtensionTags() {
  const container = document.getElementById('extensionTags');
  if (!container) return;

  const allExts = [];
  for (const lang of LANGUAGES) {
    if (setupData.selectedLangs.has(lang.key)) {
      allExts.push(...lang.extensions);
    }
  }

  if (allExts.length === 0) {
    container.innerHTML = '<span class="tag tag-empty">선택된 확장자가 없습니다</span>';
  } else {
    let html = '';
    for (const ext of allExts) {
      html += `<span class="tag">.${ext}</span>`;
    }
    container.innerHTML = html;
  }

  // Update status message (only if not currently indexing)
  const statusEl = document.getElementById('indexingStatus');
  if (!isIndexing) {
    if (allExts.length === 0) {
      statusEl.className = 'status-message warn';
      statusEl.textContent = `⚠️ 선택된 확장자가 없습니다. ${LANGUAGES.length}개 언어 중 최소 하나를 선택하세요.`;
    } else {
      statusEl.className = 'status-message info';
      statusEl.textContent = `${allExts.length}개 확장자 (${setupData.selectedLangs.size}/${LANGUAGES.length} 언어)가 선택되었습니다. 인덱싱을 시작하려면 아래 버튼을 클릭하세요.`;
    }
  }
}

// ==================== Step 5: Run Indexing ====================

async function runIndexing() {
  const outputEl = document.getElementById('indexingOutput');
  const statusEl = document.getElementById('indexingStatus');
  const btn = document.getElementById('btnStartIndexing');

  // Validate workspace path exists
  if (!setupData.workspacePath) {
    addLogEntry('ERROR', 'INDEXING', 'Workspace path not set. Please configure workspace first.');
    statusEl.className = 'status-message error';
    statusEl.textContent = '❌ 워크스페이스 경로가 설정되지 않았습니다. "작업 공간" 단계에서 경로를 먼저 설정해주세요.';
    return;
  }

  // Validate at least one language is selected
  if (setupData.selectedLangs.size === 0) {
    addLogEntry('ERROR', 'INDEXING', 'No languages selected for indexing.');
    statusEl.className = 'status-message error';
    statusEl.textContent = '❌ 최소 하나의 언어를 선택해주세요.';
    return;
  }

  // Collect extensions
  const allExts = [];
  for (const lang of LANGUAGES) {
    if (setupData.selectedLangs.has(lang.key)) {
      allExts.push(...lang.extensions);
    }
  }

  addLogEntry('INFO', 'INDEXING', `Starting indexing: ${allExts.length} extensions (${setupData.selectedLangs.size} languages)`);

  isIndexing = true;
  btn.disabled = true;
  btn.textContent = '인덱싱 중...';
  outputEl.textContent = '';

  let timerInterval;
  const startTime = Date.now();

  try {
    // Listen for command output
    window.codeatlas.onCommandOutput((data) => {
      if (data.type === 'stdout' || data.type === 'stderr') {
        outputEl.textContent += data.text;
        outputEl.scrollTop = outputEl.scrollHeight;

        const prefix = data.type === 'stderr' ? '[STDERR] ' : '';
        addLogEntry(data.type === 'stderr' ? 'WARN' : 'INFO', 'INDEXING', prefix + data.text.trim());
      }
    });

    const repoRoot = await window.codeatlas.getRepoRoot();
    const indexerPath = await window.codeatlas.joinPaths(repoRoot, 'indexer');
    const releaseBin = await window.codeatlas.joinPaths(indexerPath, 'target', 'release', 'codeatlas-indexer.exe');

    // Check if release binary exists; fall back to debug
    let binExists = await window.codeatlas.fileExists(releaseBin);
    let binPath = releaseBin;

    if (!binExists) {
      const debugBin = await window.codeatlas.joinPaths(indexerPath, 'target', 'debug', 'codeatlas-indexer.exe');
      binExists = await window.codeatlas.fileExists(debugBin);
      if (binExists) {
        binPath = debugBin;
        addLogEntry('INFO', 'INDEXING', `Release binary not found, using debug binary: ${debugBin}`);
      }
    }

    if (!binExists) {
      statusEl.className = 'status-message error';
      statusEl.textContent = '❌ 인덱서 바이너리가 없습니다. "Rust 인덱서 빌드" 단계를 먼저 실행해주세요.';
      addLogEntry('ERROR', 'INDEXING', 'Indexer binary not found. Please build the indexer first.');
      return;
    }

    statusEl.className = 'status-message info';
    const baseMessage = `🚀 인덱싱을 시작합니다... (${allExts.join(', ')})`;
    statusEl.textContent = baseMessage;
    addLogEntry('INFO', 'INDEXING', `Running: ${binPath} "${setupData.workspacePath}" --extensions ${allExts.join(',')}`);

    timerInterval = setInterval(() => {
      const elapsed = Math.floor((Date.now() - startTime) / 1000);
      const mins = Math.floor(elapsed / 60);
      const secs = elapsed % 60;
      const timeStr = `${mins}:${secs.toString().padStart(2, '0')}`;
      statusEl.textContent = `${baseMessage} (${timeStr})`;
    }, 1000);

    const result = await window.codeatlas.runCommand(
      binPath,
      [setupData.workspacePath, '--extensions', allExts.join(',')],
      indexerPath
    );

    if (result.success) {
      setupData.indexingDone = true;
      statusEl.className = 'status-message success';
      statusEl.textContent = '✅ 인덱싱 완료!\n\n인덱싱 결과가 .codeatlas 디렉토리에 저장되었습니다.';
      outputEl.textContent += '\n\n✅ 인덱싱 성공!\n';
      addLogEntry('INFO', 'INDEXING', 'Indexing completed successfully');
    } else {
      statusEl.className = 'status-message error';
      statusEl.textContent = `❌ 인덱싱 실패: ${result.stderr || result.stdout}`;
      outputEl.textContent += `\n\n❌ 인덱싱 실패\n${result.stdout}\n${result.stderr}`;
      addLogEntry('ERROR', 'INDEXING', `Indexing failed: ${result.stderr || result.stdout}`);
    }
  } catch (err) {
    statusEl.className = 'status-message error';
    statusEl.textContent = `❌ 오류: ${err.message}`;
    addLogEntry('ERROR', 'INDEXING', `Indexing error: ${err.message}`);
  } finally {
    if (timerInterval) clearInterval(timerInterval);
    btn.disabled = false;
    btn.textContent = '인덱싱 재시작';
    isIndexing = false;
    updateFooter(currentStep);
  }
}

// ==================== Step 4: Workspace Configuration ====================

async function selectWorkspace() {
  const pathInput = document.getElementById('workspacePath');
  const statusEl = document.getElementById('workspaceStatus');

  addLogEntry('INFO', 'WORKSPACE', 'Opening directory selector...');

  try {
    const selected = await window.codeatlas.selectDirectory();
    
    if (selected) {
      pathInput.value = selected;
      setupData.workspacePath = selected;

      // Auto-set data directory to .codeatlas inside workspace
      const dataDirsInput = document.getElementById('dataDirs');
      dataDirsInput.value = await window.codeatlas.joinPaths(selected, '.codeatlas');

      // Show directory listing preview
      statusEl.className = 'status-message info';
      statusEl.textContent = `📁 선택된 경로: ${selected}`;

      addLogEntry('INFO', 'WORKSPACE', `Selected workspace: ${selected}`);

      const entries = await window.codeatlas.listDirectory(selected);
      if (entries.length > 0) {
        statusEl.textContent += `\n\n📂 미리보기 (${entries.length}개 항목):\n${entries.slice(0, 10).join(', ')}`;
        addLogEntry('INFO', 'WORKSPACE', `Workspace contains ${entries.length} items`);
      } else {
        statusEl.textContent += '\n\n⚠️ 디렉토리가 비어 있거나 접근할 수 없습니다.';
        addLogEntry('WARN', 'WORKSPACE', 'Selected directory is empty or inaccessible');
      }
    } else {
      addLogEntry('INFO', 'WORKSPACE', 'Directory selection cancelled');
    }
  } catch (err) {
    const statusEl = document.getElementById('workspaceStatus');
    statusEl.className = 'status-message error';
    statusEl.textContent = `❌ 경로 선택 실패: ${err.message}`;
    addLogEntry('ERROR', 'WORKSPACE', `Failed to select directory: ${err.message}`);
  }
}

// ==================== Step 5: Complete ====================

function populateSummary() {
  const summaryEl = document.getElementById('setupSummary');
  
  let html = '';
  
  // Tools status
  for (const [name, data] of Object.entries(setupData.tools)) {
    const icon = data.exists ? '✅' : '❌';
    const version = data.version || '(버전 미확인)';
    html += `<div class="summary-item"><span class="summary-label">${icon} ${name}</span><span class="summary-value">${version}</span></div>`;
  }

  // Build status
  html += `<div class="summary-item"><span class="summary-label">🔨 인덱서</span><span class="summary-value">${setupData.indexerBuilt ? '✅ 빌드 완료' : '⏭️ 건너뜀'}</span></div>`;
  html += `<div class="summary-item"><span class="summary-label">📦 서버</span><span class="summary-value">${setupData.serverInstalled ? '✅ 설치 완료' : '⏭️ 건너뜀'}</span></div>`;

  // Workspace
  if (setupData.workspacePath) {
    html += `<div class="summary-item"><span class="summary-label">📁 워크스페이스</span><span class="summary-value">${setupData.workspacePath}</span></div>`;
  }

  // Indexing config
  const exts = [];
  for (const lang of LANGUAGES) {
    if (setupData.selectedLangs.has(lang.key)) {
      exts.push(...lang.extensions);
    }
  }
  if (exts.length > 0) {
    html += `<div class="summary-item"><span class="summary-label">📝 인덱싱 대상</span><span class="summary-value">.${exts.join(', .')}</span></div>`;
  }

  // Indexing status
  if (setupData.indexingDone) {
    html += `<div class="summary-item"><span class="summary-label">🚀 인덱싱</span><span class="summary-value">✅ 완료</span></div>`;
  } else {
    html += `<div class="summary-item"><span class="summary-label">🚀 인덱싱</span><span class="summary-value">⏭️ 건너뜀</span></div>`;
  }

  summaryEl.innerHTML = html;
}

async function launchCodeAtlas() {
  try {
    addLogEntry('INFO', 'LAUNCH', 'Starting CodeAtlas...');
    
    const repoRoot = await window.codeatlas.getRepoRoot();
    
    // Save configuration - single dataDir path
    let dataDir = document.getElementById('dataDirs').value.trim();
    if (!dataDir && setupData.workspacePath) {
      // Fallback: use .codeatlas inside workspace
      dataDir = await window.codeatlas.joinPaths(setupData.workspacePath, '.codeatlas');
    }

    // Collect selected extensions for indexing
    const indexedExts = [];
    for (const lang of LANGUAGES) {
      if (setupData.selectedLangs.has(lang.key)) {
        indexedExts.push(...lang.extensions);
      }
    }

    const config = {
      dashboard: {
        autoOpen: true,
        port: parseInt(document.getElementById('serverPort').value) || 8090,
        dataDir: dataDir
      },
      watcher: {
        enabled: true,
        indexerPath: 'codeatlas-indexer'
      },
      indexing: {
        extensions: indexedExts,
        languages: Array.from(setupData.selectedLangs)
      }
    };

    // Write config file
    const appData = await window.codeatlas.getAppDataPath();
    const configDir = await window.codeatlas.joinPaths(appData, 'CodeAtlas');
    const configPath = await window.codeatlas.joinPaths(configDir, 'codeatlas-config.json');
    
    addLogEntry('INFO', 'CONFIG', `Writing config to ${configPath}`);
    await window.codeatlas.writeConfig(configPath, config);

    // Try to start the server
    const serverIndexPath = await window.codeatlas.joinPaths(repoRoot, 'server', 'dist', 'index.js');
    if (await window.codeatlas.fileExists(serverIndexPath)) {
      addLogEntry('INFO', 'LAUNCH', `Starting server: ${serverIndexPath} with dataDir: ${dataDir}`);
      // Server is built - try to launch
      await window.codeatlas.spawnProcess('node', [serverIndexPath, dataDir], { cwd: repoRoot });
    } else {
      addLogEntry('WARN', 'LAUNCH', `Server index.js not found at ${serverIndexPath}`);
    }

    addLogEntry('INFO', 'LAUNCH', 'CodeAtlas launched successfully');
    alert('🚀 CodeAtlas가 시작되었습니다!\n\n서버가 백그라운드에서 실행됩니다.\n대시보드는 http://localhost:' + (config.dashboard.port || 8090) + ' 에서 접속하세요.');
    
  } catch (err) {
    addLogEntry('ERROR', 'LAUNCH', `Failed to launch CodeAtlas: ${err.message}`);
    alert(`❌ CodeAtlas 시작 중 오류: ${err.message}`);
  }
}

async function openReadme() {
  // Open README in default browser/editor
  try {
    const root = await window.codeatlas.getRepoRoot();
    const readmePath = await window.codeatlas.joinPaths(root, 'README.md');
    if (await window.codeatlas.fileExists(readmePath)) {
      addLogEntry('INFO', 'README', `Opening ${readmePath}`);
      // Use default system opener
      await window.codeatlas.spawnProcess('start', [readmePath], { shell: true });
    }
  } catch (err) {
    addLogEntry('ERROR', 'README', `Failed to open README: ${err.message}`);
  }
}

// ==================== Utility Functions ====================

function clearTerminal() {
  // Clear the currently visible terminal output
  document.querySelectorAll('.terminal-output').forEach(el => el.textContent = '');
}

// Toggle logs panel visibility
function toggleLogs() {
  const panel = document.getElementById('logsPanel');
  const icon = document.getElementById('logsToggleIcon');
  
  if (panel.classList.contains('collapsed')) {
    panel.classList.remove('collapsed');
    icon.textContent = '▼';
  } else {
    panel.classList.add('collapsed');
    icon.textContent = '▶';
  }
}

// ==================== Global Event Handlers ====================

// Expose functions to window for onclick handlers in HTML
window._installTool = installTool;
window.nextStep = nextStep;
window.goToStep = goToStep;
window.selectWorkspace = selectWorkspace;
window.clearTerminal = clearTerminal;
window.launchCodeAtlas = launchCodeAtlas;
window.openReadme = openReadme;

// Expose step-specific action functions
window.buildIndexer = buildIndexer;
window.installServer = installServer;
window.runIndexing = runIndexing;

// Log management functions
window.loadLogs = loadLogs;
window.clearLogs = clearLogs;
window.addLogEntry = addLogEntry;

// ==================== Initialize ====================

document.addEventListener('DOMContentLoaded', () => {
  showStep(0);
  
  // Set up log entry listener from main process
  window.codeatlas.onLogEntry((log) => {
    addLogEntry(log.level, log.step || '', log.message);
  });
  
  // Load initial logs
  loadLogs();
});
