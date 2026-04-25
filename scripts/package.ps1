param(
    [string]$Profile = "release",
    [switch]$RunSmoke,
    [string]$Distro = "pane-arch",
    [string]$DesktopEnvironment = "xfce",
    [switch]$Offline
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$distRoot = Join-Path $repoRoot "dist"
$packageDir = Join-Path $distRoot "pane-windows-x86_64"
$archivePath = Join-Path $distRoot "pane-windows-x86_64.zip"
$exeAssetPath = Join-Path $distRoot "pane-windows-x86_64.exe"
$validationScript = Join-Path $repoRoot "scripts\validate-package.ps1"
$certificationScript = Join-Path $repoRoot "scripts\certify-fresh-machine.ps1"
$assetRoot = Join-Path $repoRoot "scripts\package-assets"
$iconAssetRoot = Join-Path $repoRoot "assets"

Push-Location $repoRoot
try {
    $cargoArgs = @("build", "--profile", $Profile)
    if ($Offline) {
        $cargoArgs += "--offline"
    }

    cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build failed with exit code $LASTEXITCODE"
    }

    $binaryPath = Join-Path $repoRoot "target\$Profile\pane.exe"
    if (-not (Test-Path $binaryPath)) {
        throw "Expected built binary at $binaryPath"
    }
    if (-not (Test-Path $assetRoot)) {
        throw "Expected package asset directory at $assetRoot"
    }
    if (-not (Test-Path $iconAssetRoot)) {
        throw "Expected icon asset directory at $iconAssetRoot"
    }

    New-Item -ItemType Directory -Force $distRoot | Out-Null

    if (Test-Path $exeAssetPath) {
        Remove-Item $exeAssetPath -Force
    }
    Remove-Item $packageDir -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force $packageDir | Out-Null

    Copy-Item $binaryPath $exeAssetPath
    Copy-Item $binaryPath (Join-Path $packageDir "pane.exe")
    Copy-Item (Join-Path $repoRoot "README.md") $packageDir
    Copy-Item (Join-Path $repoRoot "LICENSE") $packageDir
    Copy-Item (Join-Path $repoRoot "docs\mvp-arch.md") $packageDir
    Copy-Item (Join-Path $repoRoot "docs\phase-1-audit.md") $packageDir
    Copy-Item (Join-Path $repoRoot "docs\clean-machine-validation.md") (Join-Path $packageDir "clean-machine-validation.md")
    Copy-Item (Join-Path $repoRoot "docs\vision.md") (Join-Path $packageDir "vision.md")
    Copy-Item (Join-Path $repoRoot "docs\product-contract.md") (Join-Path $packageDir "product-contract.md")
    Copy-Item (Join-Path $repoRoot "docs\native-runtime-architecture.md") (Join-Path $packageDir "native-runtime-architecture.md")
    Copy-Item $validationScript (Join-Path $packageDir "validate-package.ps1")
    Copy-Item $certificationScript (Join-Path $packageDir "certify-fresh-machine.ps1")
    Copy-Item (Join-Path $assetRoot "*") $packageDir
    Copy-Item $iconAssetRoot (Join-Path $packageDir "assets") -Recurse

    if (Test-Path $archivePath) {
        Remove-Item $archivePath -Force
    }
    Compress-Archive -Path (Join-Path $packageDir "*") -DestinationPath $archivePath

    Write-Host "Standalone exe: $exeAssetPath"
    Write-Host "Package directory: $packageDir"
    Write-Host "Archive: $archivePath"

    if ($RunSmoke) {
        & $certificationScript -PackagePath $packageDir -Mode PackageOnly -Distro $Distro -DesktopEnvironment $DesktopEnvironment
        & $validationScript -PackagePath $packageDir -Distro $Distro -DesktopEnvironment $DesktopEnvironment
    }
}
finally {
    Pop-Location
}
