# Lastbook Europe for Windows: an installer (Inno Setup) and a .zip to unpack anywhere, for x64. Both hold lastbook.exe (the
# window and the engine), lastbook-llm.exe (the local AI runner), pdfium.dll (PDFium, which draws PDF pages) and the program's files.
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
Remove-Item -Recurse -Force "$Stage\content\library" -ErrorAction SilentlyContinue # sources lists for lastbook-tools, not read by the app
Copy-Item "LICENSE" "$Stage\LICENSE.txt"

# PDFium beside lastbook.exe, which draws PDF pages: the build pinned in pdfium.txt, used only if its SHA-256 is the
# pinned one; the archive is kept in derived\pdfium\ for the next time. Its licence texts go in licenses\pdfium.
$ProgressPreference = "SilentlyContinue" # Windows PowerShell downloads slowly while it draws progress
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
$Asset = "pdfium-win-x64.tgz"
$Pin = @{}
foreach ($Line in Get-Content "$Root\packaging\rust\pdfium.txt") {
  if ($Line -match '^\s*(#|$)') { continue }
  $Key, $Value = $Line -split '\s+', 2
  if ($Value) { $Pin[$Key] = $Value.Trim() }
}
$Release = $Pin["release"]
$Sum = $Pin[$Asset]
if (-not $Release -or -not $Sum) { throw "PDFium: $Asset is not pinned in packaging\rust\pdfium.txt" }
$Cache = Join-Path $Root ("derived\pdfium\" + ($Release -replace '/', '-'))
New-Item -ItemType Directory -Force -Path $Cache | Out-Null
$Archive = Join-Path $Cache $Asset
if (-not (Test-Path $Archive) -or (Get-FileHash -Algorithm SHA256 $Archive).Hash.ToLower() -ne $Sum) {
  Remove-Item -Force -ErrorAction SilentlyContinue $Archive, "$Archive.part"
  $Url = "https://github.com/bblanchon/pdfium-binaries/releases/download/" + ($Release -replace '/', '%2F') + "/$Asset"
  Invoke-WebRequest -UseBasicParsing -Uri $Url -OutFile "$Archive.part"
  $Got = (Get-FileHash -Algorithm SHA256 "$Archive.part").Hash.ToLower()
  if ($Got -ne $Sum) { Remove-Item "$Archive.part"; throw "PDFium: $Asset from $Release has SHA-256 $Got, not the pinned ${Sum}: not used" }
  Move-Item -Force "$Archive.part" $Archive
}
$Unpacked = Join-Path $Cache "unpacked"
if (Test-Path $Unpacked) { Remove-Item -Recurse -Force $Unpacked }
New-Item -ItemType Directory -Force -Path $Unpacked | Out-Null
& "$env:SystemRoot\System32\tar.exe" -xzf $Archive -C $Unpacked # Windows' own tar: Git's would read C: as a host name
if ($LASTEXITCODE -ne 0) { throw "PDFium: $Asset could not be unpacked" }
Copy-Item "$Unpacked\bin\pdfium.dll" $Stage
New-Item -ItemType Directory -Force -Path "$Stage\licenses\pdfium" | Out-Null
Copy-Item "$Unpacked\LICENSE" "$Stage\licenses\pdfium\pdfium-binaries-LICENSE.txt"
Copy-Item "$Unpacked\licenses\*" "$Stage\licenses\pdfium\"
Remove-Item -Recurse -Force $Unpacked
Write-Host "PDFium $Release ($Asset, SHA-256 checked): $Stage\pdfium.dll"
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
