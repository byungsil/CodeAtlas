/**
 * CodeAtlas Setup Wizard - Frontend Controller
 * Manages step navigation, tool installation, and build processes.
 */

// ==================== State ====================
let currentStep = 0;
const totalSteps = 6; // 0-5 (welcome, prereqs, indexer, server, workspace, complete)
const setupData = {
  tools: {},
  indexerBuilt: false,
  serverInstalled: false,
  workspacePath: '',
  config: {}
};

// Build/install state tracking
let isBuildingIndexer = false;
let isInstallingServer = false;

// Log state
let logEntries = [];
const MAX_LOG_DISPLAY = 200;

// ==================== Step Navigation ====================

function showStep(stepIndex) {
  // Hide all steps
  document.querySelectorAll('.wizard-step').forEach(el => el.classList.remove('active'));
  
  // Show target step
  const stepMap = ['welcome', 'prereqs', 'indexer', 'server', 'workspace', 'complete'];
  const targetId = `step-${stepMap[stepIndex]}`;
  document.getElementById(targetId).classList.add('active');

  // Update progress indicators
  updateProgress(stepIndex);

  // Update footer buttons
  updateFooter(stepIndex);

  currentStep = stepIndex;

  // Auto-run step-specific logic
  if (stepIndex === 1) runPrereqCheck();
}

function nextStep() {
  // Prevent navigation during build/install
  if (isBuildingIndexer || isInstallingServer) {
    addLogEntry('WARN', 'NAV', 'Cannot navigate while build/install is in progress');
    return;
  }
  
  if (currentStep < totalSteps - 1) {
    showStep(currentStep + 1);
  }
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
  if (stepIndex === 0 || stepIndex === totalSteps - 1) {
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
    statusEl.textContent = '🔨 Rust 인덱서를 빌드하고 있습니다... (최초 빌드는 5-10 분 소요)';

    addLogEntry('INFO', 'BUILD', `Building in: ${indexerPath}`);

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
    statusEl.textContent = '📦 npm 의존성을 설치하고 있습니다...';

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
    btn.disabled = false;
    btn.textContent = '설치 시작';
    isInstallingServer = false;
    updateFooter(currentStep); // Re-evaluate next button state
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
      dataDirsInput.value = `${selected}\.codeatlas`;

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

  summaryEl.innerHTML = html;
}

async function launchCodeAtlas() {
  try {
    addLogEntry('INFO', 'LAUNCH', 'Starting CodeAtlas...');
    
    const repoRoot = await window.codeatlas.getRepoRoot();
    
    // Save configuration
    const dataDirsValue = document.getElementById('dataDirs').value.trim();
    let dataDirsList = [];
    
    if (dataDirsValue) {
      // Parse comma-separated paths, default to .codeatlas in workspace if only one path
      dataDirsList = dataDirsValue.split(',').map(d => d.trim()).filter(d => d.length > 0);
    } else if (setupData.workspacePath) {
      // Fallback: use .codeatlas inside workspace
      dataDirsList = [await window.codeatlas.joinPaths(setupData.workspacePath, '.codeatlas')];
    }

    const config = {
      dashboard: {
        autoOpen: true,
        port: parseInt(document.getElementById('serverPort').value) || 3000,
        dataDirs: dataDirsList
      },
      watcher: {
        enabled: true,
        indexerPath: 'codeatlas-indexer'
      }
    };

    // Write config file
    const appData = process.env.APPDATA || process.env.HOME || '';
    const configDir = await window.codeatlas.joinPaths(appData, 'CodeAtlas');
    const configPath = await window.codeatlas.joinPaths(configDir, 'codeatlas-config.json');
    
    addLogEntry('INFO', 'CONFIG', `Writing config to ${configPath}`);
    await window.codeatlas.writeConfig(configPath, config);

    // Try to start the server
    const serverPath = await window.codeatlas.joinPaths(repoRoot, 'server', 'dist', 'app.js');
    if (await window.codeatlas.fileExists(serverPath)) {
      addLogEntry('INFO', 'LAUNCH', `Starting server: ${serverPath}`);
      // Server is built - try to launch
      await window.codeatlas.spawnProcess('node', [serverPath], { cwd: repoRoot });
    }

    addLogEntry('INFO', 'LAUNCH', 'CodeAtlas launched successfully');
    alert('🚀 CodeAtlas가 시작되었습니다!\n\n서버가 백그라운드에서 실행됩니다.\n대시보드는 http://localhost:' + (config.dashboard.port || 3000) + ' 에서 접속하세요.');
    
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
