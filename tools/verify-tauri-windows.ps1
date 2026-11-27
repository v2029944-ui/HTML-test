#Requires -Version 5.1
[CmdletBinding()]
param(
  [switch]$BuildInstaller
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Invoke-Checked([string]$Program, [string[]]$Arguments) {
  Write-Host ('> ' + $Program + ' ' + ($Arguments -join ' '))
  & $Program @Arguments
  if ($LASTEXITCODE -ne 0) {
    throw "$Program exited with code $LASTEXITCODE"
  }
}

$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Push-Location $root
try {
  foreach ($program in @('node.exe', 'npm.cmd', 'cargo.exe')) {
    if (-not (Get-Command $program -ErrorAction SilentlyContinue)) {
      throw "Missing $program. Install Node.js 24 and stable Rust before running verification."
    }
  }
  $nodeMajor = & node.exe -p "process.versions.node.split('.')[0]"
  if ($LASTEXITCODE -ne 0 -or [int]$nodeMajor -lt 24) {
    throw 'Node.js 24 or newer is required.'
  }

  $sourceHtmlHash = (Get-FileHash 'index.html' -Algorithm SHA256).Hash
  Invoke-Checked 'npm.cmd' @('ci', '--prefix', 'tauri', '--no-audit', '--no-fund', '--include=optional')

  $nodeTests = @(Get-ChildItem -Path 'tauri/tests/*.test.cjs', 'tests/test_*.cjs', '.github/scripts/pr-safety.test.cjs' -File |
    Sort-Object -Property FullName | ForEach-Object { $_.FullName })
  if ($nodeTests.Count -eq 0) { throw 'No Node.js test files were found.' }
  Invoke-Checked 'node.exe' (@('--test') + $nodeTests)

  Invoke-Checked 'npm.cmd' @('--prefix', 'tauri', 'run', 'prepare:frontend')
  Invoke-Checked 'npm.cmd' @('--prefix', 'tauri', 'run', 'icons')
  if ($sourceHtmlHash -ne (Get-FileHash 'index.html' -Algorithm SHA256).Hash) {
    throw 'Preparing Tauri unexpectedly modified the standalone index.html.'
  }
  $preparedHtml = Get-Content 'tauri/web/index.html' -Raw -Encoding UTF8
  if (-not ($preparedHtml.Contains('tauri-unified-app.js') -and
             $preparedHtml.Contains('tauri-reference-desktop.js') -and
             $preparedHtml.Contains('tauri-bridge.js') -and
             $preparedHtml.Contains('tauri-updates.js'))) {
    throw 'Prepared Tauri frontend is missing a required script.'
  }

  Invoke-Checked 'cargo.exe' @('test', '--locked', '--manifest-path', 'tauri/src-tauri/Cargo.toml')
  Invoke-Checked 'cargo.exe' @('test', '--locked', '--manifest-path', 'tauri/import-core/Cargo.toml')
  Invoke-Checked 'cargo.exe' @('test', '--locked', '--manifest-path', 'tauri/storage-core/Cargo.toml')

  if ($BuildInstaller) {
    Invoke-Checked 'npm.cmd' @('--prefix', 'tauri', 'run', 'build:windows', '--', '--', '--locked')
    $nsisFolder = 'tauri/src-tauri/target/release/bundle/nsis'
    $installers = @(Get-ChildItem -Path $nsisFolder -Filter '*.exe' -File -ErrorAction SilentlyContinue)
    if ($installers.Count -ne 1) {
      throw "Expected one local NSIS installer in $nsisFolder; found $($installers.Count)."
    }
    $setup = $installers[0]
    Write-Host ('Installer: ' + $setup.FullName)
    Write-Host ('SHA256: ' + (Get-FileHash $setup.FullName -Algorithm SHA256).Hash)
  }

  Write-Host 'PASS: Node.js tests, Tauri frontend preparation, unchanged standalone HTML, Rust app/import/storage tests.'
  if ($BuildInstaller) {
    Write-Host 'PASS: unsigned local installer built. Windows GUI and user data are NOT tested.'
  } else {
    Write-Host 'Installer build omitted (use -BuildInstaller to include it).'
  }
}
finally {
  Pop-Location
}
