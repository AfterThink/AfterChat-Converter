import { copyFileSync, existsSync, mkdirSync, statSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const guiRoot = resolve(__dirname, "..");
const repoRoot = resolve(guiRoot, "..", "..");
const tauriBinDir = resolve(guiRoot, "src-tauri", "bin");
const isRelease = process.argv.includes("--release");
const profile = isRelease ? "release" : "debug";
const exeSuffix = process.platform === "win32" ? ".exe" : "";

const binaries = [
  { pkg: "google-ai-studio-json-converter", bin: "ai-studio" },
  { pkg: "cherry-studio-backup-json-converter", bin: "cherry" },
  { pkg: "qwen-json-converter", bin: "qwen" },
];

mkdirSync(tauriBinDir, { recursive: true });

const hostTriple = getHostTriple();
buildWorkspaceBinaries();

for (const { bin } of binaries) {
  const source = resolve(repoRoot, "target", profile, `${bin}${exeSuffix}`);
  const target = resolve(tauriBinDir, `${bin}-${hostTriple}${exeSuffix}`);

  if (!existsSync(source)) {
    throw new Error(`未找到 sidecar 二进制：${source}`);
  }

  // Check if target needs update (if source is newer or target missing)
  let needsCopy = true;
  if (existsSync(target)) {
    const srcStat = statSync(source);
    const tgtStat = statSync(target);
    // If source mtime is older or equal to target mtime, we assume it's up to date
    if (srcStat.mtimeMs <= tgtStat.mtimeMs) {
      needsCopy = false;
    }
  }

  if (needsCopy) {
    copyFileSync(source, target);
    console.log(`copied ${bin} -> ${target}`);
  } else {
    console.log(`skipped ${bin} (up to date)`);
  }
}

function buildWorkspaceBinaries() {
  const args = ["build"];
  if (isRelease) {
    args.push("--release");
  }

  for (const { pkg } of binaries) {
    args.push("-p", pkg);
  }

  execFileSync("cargo", args, {
    cwd: repoRoot,
    stdio: "inherit",
  });
}

function getHostTriple() {
  const versionText = execFileSync("rustc", ["-vV"], {
    cwd: repoRoot,
    encoding: "utf8",
  });

  const match = versionText.match(/^host:\s+(.+)$/m);
  if (!match) {
    throw new Error("无法从 rustc -vV 中解析 host triple。");
  }

  return match[1].trim();
}
