param([switch]$Test)
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
if (Test-Path "$PSScriptRoot/.tools/cargo/bin/cargo.exe") {
    $env:CARGO_HOME = "$PSScriptRoot/.tools/cargo"
    $env:RUSTUP_HOME = "$PSScriptRoot/.tools/rustup"
    $cargoExe = "$PSScriptRoot/.tools/cargo/bin/cargo.exe"
} else {
    $cargoExe = 'cargo'
}
if ($Test) {
    & $cargoExe test --lib
    if ($LASTEXITCODE -ne 0) { throw 'Tests failed' }
}
& $cargoExe build --release --bins
if ($LASTEXITCODE -ne 0) { throw 'Build failed' }
New-Item -ItemType Directory -Force "$PSScriptRoot/dist" | Out-Null
Copy-Item "$PSScriptRoot/target/release/battery-status.exe" "$PSScriptRoot/dist/BatteryStatus.exe"
Copy-Item "$PSScriptRoot/target/release/battery-diagnostics.exe" "$PSScriptRoot/dist/BatteryDiagnostics.exe"
Copy-Item "$PSScriptRoot/LICENSE", "$PSScriptRoot/THIRD-PARTY-NOTICES.md", "$PSScriptRoot/README-BATTERY-STATUS.md" "$PSScriptRoot/dist"
Write-Host 'Built dist/BatteryStatus.exe and dist/BatteryDiagnostics.exe'
