import { copyFileSync, existsSync, mkdirSync } from "node:fs";
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
  "google-ai-studio-json-converter",
  "cherry-studio-backup-json-converter",
  "qwen-json-converter",
];

mkdirSync(tauriBinDir, { recursive: true });

const hostTriple = getHostTriple();
buildWorkspaceBinaries();

for (const name of binaries) {
  const source = resolve(repoRoot, "target", profile, `${name}${exeSuffix}`);
  const target = resolve(tauriBinDir, `${name}-${hostTriple}${exeSuffix}`);

  if (!existsSync(source)) {
    throw new Error(`未找到 sidecar 二进制：${source}`);
  }

  copyFileSync(source, target);
  console.log(`copied ${name} -> ${target}`);
}

function buildWorkspaceBinaries() {
  const args = ["build"];
  if (isRelease) {
    args.push("--release");
  }

  for (const name of binaries) {
    args.push("-p", name);
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
