/**
 * pre-commit hook: 每次本地提交自动递增 patch 版本号
 * 同步更新 package.json / Cargo.toml / tauri.conf.json
 */
const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

const ROOT = path.resolve(__dirname, '..');

try {
  // 1. 读取 package.json
  const pkgPath = path.join(ROOT, 'package.json');
  const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));

  // 2. 解析并递增 patch
  const parts = pkg.version.split('.').map(Number);
  if (parts.length !== 3 || parts.some(isNaN)) {
    console.error(`❌ 无法解析版本号: ${pkg.version}`);
    process.exit(1);
  }
  const [maj, min, patch] = parts;
  const newVersion = `${maj}.${min}.${patch + 1}`;

  // 3. 更新 package.json
  const oldVersion = pkg.version;
  pkg.version = newVersion;
  fs.writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + '\n', 'utf8');

  // 4. 更新 src-tauri/Cargo.toml
  const cargoPath = path.join(ROOT, 'src-tauri', 'Cargo.toml');
  let cargo = fs.readFileSync(cargoPath, 'utf8');
  cargo = cargo.replace(/^version = ".*"/m, `version = "${newVersion}"`);
  fs.writeFileSync(cargoPath, cargo, 'utf8');

  // 5. 更新 src-tauri/tauri.conf.json
  const tauriPath = path.join(ROOT, 'src-tauri', 'tauri.conf.json');
  const tauri = JSON.parse(fs.readFileSync(tauriPath, 'utf8'));
  tauri.version = newVersion;
  fs.writeFileSync(tauriPath, JSON.stringify(tauri, null, 2) + '\n', 'utf8');

  // 6. Stage 这三个文件纳入本次提交
  execSync('git add package.json src-tauri/Cargo.toml src-tauri/tauri.conf.json', {
    cwd: ROOT,
    stdio: 'ignore',
  });

  console.log(`🔖 版本自动递增: ${oldVersion} → ${newVersion}`);
} catch (err) {
  console.error('❌ 版本递增失败:', err.message);
  // 不阻止提交，仅打印警告
  process.exit(0);
}
