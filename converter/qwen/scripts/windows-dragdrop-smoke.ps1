# Windows 拖拽式调用冒烟测试（CI: windows-latest 上跑）
#
# 覆盖两条主路径：
#   1. 单体导出（顶层数组）        -> 源文件同目录产出 "<title>.md"
#   2. 全部导出（包装 data 数组）  -> 源文件同目录产出 "chat-export-qwen-all-*.zip"（含 2 个条目）
$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$workDir = Join-Path $repoRoot "target\dragdrop-smoke"
if (Test-Path $workDir) {
    Remove-Item -Recurse -Force $workDir
}
New-Item -ItemType Directory -Path $workDir | Out-Null

function Write-JsonFile {
    param([string]$Path, [string]$Content)
    # UTF-8 无 BOM：Set-Content 在部分 PowerShell 版本会写 BOM
    [System.IO.File]::WriteAllText($Path, $Content, [System.Text.UTF8Encoding]::new($false))
}

# ── 1. 单体导出 ──────────────────────────────────────────────
$single = @'
[
  {
    "id": "drag-smoke",
    "title": "Drag Smoke",
    "created_at": 1700000000,
    "chat": {
      "messages": [
        { "id": "u1", "role": "user", "content": "drag question" },
        {
          "id": "a1",
          "role": "assistant",
          "content": "",
          "reasoning_content": null,
          "modelName": "Qwen3.5-Plus",
          "content_list": [
            { "phase": "think", "content": "drag thinking" },
            { "phase": "answer", "content": "drag answer" }
          ]
        }
      ]
    }
  }
]
'@
$singleJson = Join-Path $workDir "single.json"
Write-JsonFile -Path $singleJson -Content $single

# 拖拽等价：不带任何子命令/参数，输出落在源文件同目录
cargo run --quiet -- $singleJson

# sanitizeFilename 保留空格，所以文件名是 "Drag Smoke.md"（不是 "Drag_Smoke.md"）
$mdPath = Join-Path $workDir "Drag Smoke.md"
if (!(Test-Path $mdPath)) {
    $found = Get-ChildItem $workDir -Filter *.md | Select-Object -ExpandProperty Name
    throw "Single-export smoke failed: expected '$mdPath'. Found: $found"
}

$body = [System.IO.File]::ReadAllText($mdPath)
foreach ($marker in @(
    '## Metadata',
    '- **Model:** `qwen3.5-plus`',
    'https://chat.qwen.ai/c/drag-smoke',
    '## Conversation',
    '### 🧑‍💻 User',
    '### 🤖 Assistant',
    '#### 🤔 Thought Process',
    '#### 💡 Response',
    'drag thinking',
    'drag answer'
)) {
    if ($body -notmatch [regex]::Escape($marker)) {
        throw "Single-export smoke failed: markdown is missing '$marker'"
    }
}
if ($body -match 'Run Settings') {
    throw "Single-export smoke failed: legacy 'Run Settings' section should be gone"
}

# ── 2. 全部导出 ──────────────────────────────────────────────
$all = @'
{
  "success": true,
  "request_id": "drag-smoke-req",
  "data": [
    {
      "id": "s-1",
      "title": "Second Chat",
      "created_at": 1700000100,
      "updated_at": 1700000100,
      "chat": {
        "messages": [
          { "role": "user", "content": "second question" },
          { "role": "assistant", "content": "second answer" }
        ]
      }
    },
    {
      "id": "s-2",
      "title": "First Chat",
      "created_at": 1700000000,
      "updated_at": 1700000000,
      "chat": {
        "messages": [
          { "role": "user", "content": "first question" },
          { "role": "assistant", "content": "first answer" }
        ]
      }
    }
  ]
}
'@
$allJson = Join-Path $workDir "all.json"
Write-JsonFile -Path $allJson -Content $all

cargo run --quiet -- $allJson

$zip = Get-ChildItem $workDir -Filter "chat-export-qwen-all-*.zip" | Select-Object -First 1
if ($null -eq $zip) {
    $found = Get-ChildItem $workDir | Select-Object -ExpandProperty Name
    throw "All-export smoke failed: no 'chat-export-qwen-all-*.zip' produced. Found: $found"
}

Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [System.IO.Compression.ZipFile]::OpenRead($zip.FullName)
try {
    $entries = @($archive.Entries | ForEach-Object { $_.FullName })
    if ($entries.Count -ne 2) {
        throw "All-export smoke failed: expected 2 zip entries, got $($entries.Count): $entries"
    }
    if ($entries -contains "export-failures.md") {
        throw "All-export smoke failed: unexpected 'export-failures.md'"
    }
    # 时间降序：较新的 "Second Chat" 在前
    if ($entries[0] -notmatch '-Second Chat\.md$') {
        throw "All-export smoke failed: expected newest first, got: $entries"
    }
    if ($entries[1] -notmatch '-First Chat\.md$') {
        throw "All-export smoke failed: expected oldest second, got: $entries"
    }
} finally {
    $archive.Dispose()
}

Write-Host "Drag-drop smoke test passed."
Write-Host "  single -> $mdPath"
Write-Host "  all    -> $($zip.FullName) ($($entries.Count) entries)"
