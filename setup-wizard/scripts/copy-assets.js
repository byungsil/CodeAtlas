/**
 * Copy static assets (HTML, CSS) from src/renderer/ to root renderer/
 * This ensures Electron can find them at runtime.
 */

const fs = require('fs');
const path = require('path');

const srcDir = path.join(__dirname, '..', 'src', 'renderer');
const destDir = path.join(__dirname, '..', 'renderer');

// Create destination directory if it doesn't exist
if (!fs.existsSync(destDir)) {
  fs.mkdirSync(destDir, { recursive: true });
}

// Copy files
const filesToCopy = ['index.html', 'styles.css', 'wizard.js'];

for (const file of filesToCopy) {
  const srcPath = path.join(srcDir, file);
  const destPath = path.join(destDir, file);
  
  if (fs.existsSync(srcPath)) {
    fs.copyFileSync(srcPath, destPath);
    console.log(`Copied: ${file}`);
  } else {
    console.warn(`Warning: ${file} not found at ${srcPath}`);
  }
}

console.log('Assets copied successfully.');
