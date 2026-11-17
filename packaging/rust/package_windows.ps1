# Lastbook Europe for Windows: an installer (Inno Setup) and a .zip to unpack anywhere, for x64.
#   powershell -File packaging\rust\package_windows.ps1 [-Out DIR]          (default dist\)
# Not signed with Authenticode yet (the maintainer's decision): SmartScreen may ask the person to confirm.
param([string]$Out = "")
$ErrorActionPreference = "Stop"
$Root = (Resolve-Path "$PSScriptRoot\..\..").Path
if (-not $Out) { $Out = Join-Path $Root "dist" }
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Out = (Resolve-Path $Out).Path
$Ver = (Select-String -Path "$Root\Cargo.toml" -Pattern '^version = "(.*)"' | Select-Object -First 1).Matches[0].Groups[1].Value
Set-Location $Root
cargo build --release --locked -p lastbook-app -p lastbook-llm
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
$Stage = Join-Path $env:RUNNER_TEMP "lastbook-stage"
if (-not $env:RUNNER_TEMP) { $Stage = Join-Path $env:TEMP "lastbook-stage" }
if (Test-Path $Stage) { Remove-Item -Recurse -Force $Stage }
New-Item -ItemType Directory -Force -Path $Stage | Out-Null
Copy-Item "target\release\lastbook.exe", "target\release\lastbook-llm.exe" $Stage
# llama.cpp's libraries and its backends (Vulkan, and the CPU variants), which the runner loads from beside itself
Get-ChildItem "target\release" -Filter "*.dll" | Where-Object { $_.Name -match '^(llama|ggml)' } | Copy-Item -Destination $Stage
$Backends = Get-ChildItem "target\release\build" -Directory -Recurse -Filter "backends" | Where-Object { $_.FullName -match 'llama-cpp-sys-2-.*\\out\\backends$' } | Select-Object -First 1
if ($Backends) { Get-ChildItem $Backends.FullName -Filter "*.dll" | Copy-Item -Destination $Stage }
Copy-Item -Recurse "web" "$Stage\web"
Copy-Item -Recurse "content" "$Stage\content"
Copy-Item "LICENSE" "$Stage\LICENSE.txt"
$Iscc = @("${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:ProgramFiles\Inno Setup 6\ISCC.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $Iscc) {
  choco install innosetup -y --no-progress | Out-Null
  $Iscc = "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe"
}
& $Iscc /Q "/DVersion=$Ver" "/DStage=$Stage" "/DOut=$Out" "$Root\packaging\rust\lastbook.iss"
if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed" }
$Zip = Join-Path $Out "LastbookEurope-$Ver-windows-x64.zip"
if (Test-Path $Zip) { Remove-Item $Zip }
Compress-Archive -Path "$Stage\*" -DestinationPath $Zip
Get-ChildItem $Out
