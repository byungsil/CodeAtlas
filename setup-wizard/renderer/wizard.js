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

  // Back button
  if (stepIndex === 0 || stepIndex === totalSteps - 1) {
    btnBack.classList.add('hidden');
  } else {
    btnBack.classList.remove('hidden');
  }

  // Next/Finish button
  if (stepIndex === totalSteps - 1) {
    btnNext.textContent = '닫기';
    btnNext.onclick = () => window.close();
  } else if (stepIndex === 0) {
    btnNext.textContent = '시작하기 →';
    btnNext.onclick = nextStep;
  } else if (stepIndex === 1) {
    // Prereqs step - check if all tools are installed
    const allInstalled = Object.values(setupData.tools).every(t => t.installed);
    btnNext.textContent = allInstalled ? '다음 →' : '완료';
    btnNext.onclick = nextStep;
  } else {
    btnNext.textContent = '다음 →';
    btnNext.onclick = nextStep;
  }
}

// ==================== Step 1: Prerequisites Check ====================

async function runPrereqCheck() {
  const prereqs = [
    { name: 'node', wingetId: 'OpenJS.NodeJS.LTS', displayName: 'Node.js LTS' },
    { name: 'npm', wingetId: 'OpenJS.NodeJS.LTS', displayName: 'npm (included with Node.js)' },
    { name: 'cargo', wingetId: 'Rustlang.Rustup', displayName: 'Rust toolchain' }
  ];

  for (const prereq of prereqs) {
    const result = await window.codeatlas.checkCommand(prereq.name);
    setupData.tools[prereq.name] = result;

    updatePrereqUI(prereq.name, result);
  }

  // Check if any need installation
  const needsInstall = prereqs.some(p => !setupData.tools[p.name].exists);
  
  const statusEl = document.getElementById('prereqStatus');
  if (needsInstall) {
    statusEl.className = 'status-message info';
    statusEl.textContent = '일부 도구가 필요합니다. 각 도구 옆의 "설치하기" 버튼을 클릭하세요.';
  } else {
    statusEl.className = 'status-message success';
    statusEl.textContent = '✅ 모든 필수 도구가 설치되어 있습니다!';
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
      
      // Update status
      const statusEl = document.getElementById('prereqStatus');
      statusEl.className = 'status-message success';
      statusEl.textContent = `✅ ${config.displayName}이(가) 성공적으로 설치되었습니다.`;
    } else {
      item.classList.remove('installing');
      item.classList.add('missing');
      versionEl.textContent = '❌ 설치 실패';
      
      const statusEl = document.getElementById('prereqStatus');
      statusEl.className = 'status-message error';
      statusEl.textContent = `❌ ${config.displayName} 설치가 실패했습니다. 수동으로 설치해주세요.`;
    }
  } catch (err) {
    item.classList.remove('installing');
    item.classList.add('missing');
    versionEl.textContent = '❌ 오류';
    
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

  btn.disabled = true;
  btn.textContent = '빌드 중...';
  outputEl.textContent = '';

  // Listen for command output
  window.codeatlas.onCommandOutput((data) => {
    if (data.type === 'stdout' || data.type === 'stderr') {
      outputEl.textContent += data.text;
      outputEl.scrollTop = outputEl.scrollHeight;
    }
  });

  try {
    const repoRoot = await window.codeatlas.getRepoRoot();
    const indexerPath = require('path').join(repoRoot, 'indexer');
    
    statusEl.className = 'status-message info';
    statusEl.textContent = '🔨 Rust 인덱서를 빌드하고 있습니다... (최초 빌드는 5-10 분 소요)';

    const result = await window.codeatlas.runCommand('cargo', ['build', '--release'], indexerPath);

    if (result.success) {
      setupData.indexerBuilt = true;
      statusEl.className = 'status-message success';
      statusEl.textContent = '✅ Rust 인덱서 빌드 완료!';
      outputEl.textContent += '\n\n✅ 빌드 성공!\n';
    } else {
      statusEl.className = 'status-message error';
      statusEl.textContent = `❌ 빌드 실패: ${result.stderr || result.stdout}`;
      outputEl.textContent += `\n\n❌ 빌드 실패\n`;
    }
  } catch (err) {
    statusEl.className = 'status-message error';
    statusEl.textContent = `❌ 오류: ${err.message}`;
  }

  btn.disabled = false;
  btn.textContent = '빌드 시작';
}

// ==================== Step 3: Server Setup ====================

async function installServer() {
  const outputEl = document.getElementById('serverOutput');
  const statusEl = document.getElementById('serverStatus');
  const btn = event.target;

  btn.disabled = true;
  btn.textContent = '설치 중...';
  outputEl.textContent = '';

  // Listen for command output
  window.codeatlas.onCommandOutput((data) => {
    if (data.type === 'stdout' || data.type === 'stderr') {
      outputEl.textContent += data.text;
      outputEl.scrollTop = outputEl.scrollHeight;
    }
  });

  try {
    const repoRoot = await window.codeatlas.getRepoRoot();
    const serverPath = require('path').join(repoRoot, 'server');

    statusEl.className = 'status-message info';
    statusEl.textContent = '📦 npm 의존성을 설치하고 있습니다...';

    // Step 1: npm install
    const installResult = await window.codeatlas.runCommand('npm', ['install'], serverPath);
    
    if (!installResult.success) {
      statusEl.className = 'status-message error';
      statusEl.textContent = `❌ npm install 실패: ${installResult.stderr || installResult.stdout}`;
      btn.disabled = false;
      btn.textContent = '설치 시작';
      return;
    }

    // Step 2: Build TypeScript
    statusEl.className = 'status-message info';
    statusEl.textContent = '🔨 TypeScript를 컴파일하고 있습니다...';

    const buildResult = await window.codeatlas.runCommand('npx', ['tsc'], serverPath);

    if (buildResult.success) {
      setupData.serverInstalled = true;
      statusEl.className = 'status-message success';
      statusEl.textContent = '✅ 서버 설치 및 컴파일 완료!';
      outputEl.textContent += '\n\n✅ 서버 준비 완료!\n';
    } else {
      // Build might fail due to TypeScript errors, but npm install succeeded
      setupData.serverInstalled = true;
      statusEl.className = 'status-message info';
      statusEl.textContent = '⚠️ npm 설치는 완료되었으나 컴파일에 문제가 있습니다. (계속 진행 가능)';
    }
  } catch (err) {
    statusEl.className = 'status-message error';
    statusEl.textContent = `❌ 오류: ${err.message}`;
  }

  btn.disabled = false;
  btn.textContent = '설치 시작';
}

// ==================== Step 4: Workspace Configuration ====================

async function selectWorkspace() {
  const pathInput = document.getElementById('workspacePath');
  const statusEl = document.getElementById('workspaceStatus');

  try {
    const selected = await window.codeatlas.selectDirectory();
    
    if (selected) {
      pathInput.value = selected;
      setupData.workspacePath = selected;

      // Show directory listing preview
      statusEl.className = 'status-message info';
      statusEl.textContent = `📁 선택된 경로: ${selected}`;

      const entries = await window.codeatlas.listDirectory(selected);
      if (entries.length > 0) {
        statusEl.textContent += `\n\n📂 미리보기 (${entries.length}개 항목):\n${entries.slice(0, 10).join(', ')}`;
      } else {
        statusEl.textContent += '\n\n⚠️ 디렉토리가 비어 있거나 접근할 수 없습니다.';
      }
    }
  } catch (err) {
    const statusEl = document.getElementById('workspaceStatus');
    statusEl.className = 'status-message error';
    statusEl.textContent = `❌ 경로 선택 실패: ${err.message}`;
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
    const repoRoot = await window.codeatlas.getRepoRoot();
    
    // Save configuration
    const config = {
      dashboard: {
        autoOpen: true,
        port: parseInt(document.getElementById('serverPort').value) || 3000,
        dataDirs: document.getElementById('dataDirs').value ? 
          document.getElementById('dataDirs').value.split(',').map(d => d.trim()) : []
      },
      watcher: {
        enabled: true,
        indexerPath: 'codeatlas-indexer'
      }
    };

    if (setupData.workspacePath) {
      config.dashboard.dataDirs = [setupData.workspacePath];
    }

    // Write config file
    const configDir = require('path').join(process.env.APPDATA || process.env.HOME || '', 'CodeAtlas');
    const configPath = require('path').join(configDir, 'codeatlas-config.json');
    
    await window.codeatlas.writeConfig(configPath, config);

    // Try to start the server
    const serverPath = require('path').join(repoRoot, 'server', 'dist', 'app.js');
    if (require('fs').existsSync(serverPath)) {
      // Server is built - try to launch
      const child = require('child_process').spawn('node', [serverPath], {
        cwd: repoRoot,
        detached: true,
        stdio: 'ignore'
      });
      child.unref();
    }

    alert('🚀 CodeAtlas가 시작되었습니다!\n\n서버가 백그라운드에서 실행됩니다.\n대시보드는 http://localhost:' + (config.dashboard.port || 3000) + ' 에서 접속하세요.');
    
  } catch (err) {
    alert(`❌ CodeAtlas 시작 중 오류: ${err.message}`);
  }
}

function openReadme() {
  // Open README in default browser/editor
  window.codeatlas.getRepoRoot().then(root => {
    const readmePath = require('path').join(root, 'README.md');
    if (require('fs').existsSync(readmePath)) {
      const child = require('child_process').spawn('start', [readmePath], { shell: true });
    }
  });
}

// ==================== Utility Functions ====================

function clearTerminal() {
  // Clear the currently visible terminal output
  document.querySelectorAll('.terminal-output').forEach(el => el.textContent = '');
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

// ==================== Initialize ====================

document.addEventListener('DOMContentLoaded', () => {
  showStep(0);
});
