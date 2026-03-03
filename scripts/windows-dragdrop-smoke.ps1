$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$workDir = Join-Path $repoRoot "target\dragdrop-smoke"
if (Test-Path $workDir) {
    Remove-Item -Recurse -Force $workDir
}
New-Item -ItemType Directory -Path $workDir | Out-Null

$chatformat = @"
# Format

## Conversation
"@
$chatformat | Set-Content (Join-Path $workDir "chatformat.txt")

$json = @"
[
  {
    "id": "drag-smoke",
    "title": "Drag Smoke",
    "chat": {
      "history": {
        "messages": {
          "u1": {
            "id": "u1",
            "role": "user",
            "content": "drag question",
            "childrenIds": ["a1"],
            "timestamp": 1700000000
          },
          "a1": {
            "id": "a1",
            "role": "assistant",
            "content": "drag answer",
            "parentId": "u1",
            "timestamp": 1700000001
          }
        }
      }
    }
  }
]
"@
$inputJson = Join-Path $workDir "drag.json"
$json | Set-Content $inputJson

cargo run --quiet -- $inputJson

$mdPath = Join-Path $workDir "drag.md"
if (!(Test-Path $mdPath)) {
    throw "Drag-drop smoke test failed: expected output markdown not found at $mdPath"
}

Write-Host "Drag-drop smoke test passed: $mdPath"
