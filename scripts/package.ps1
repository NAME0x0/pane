param(
    [string]$Profile = "release",
    [switch]$RunSmoke,
    [string]$Distro = "pane-arch",
    [string]$DesktopEnvironment = "xfce",
    [switch]$Offline,
    [ValidateSet("Required", "Auto", "Disabled")]
    [string]$BundleQemu = "Required",
    [string]$QemuRoot,
    [string]$BaseImagePath
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$distRoot = Join-Path $repoRoot "dist"
$packageDir = Join-Path $distRoot "pane-windows-x86_64"
$archivePath = Join-Path $distRoot "pane-windows-x86_64.zip"
$exeAssetPath = Join-Path $distRoot "pane-windows-x86_64.exe"
$validationScript = Join-Path $repoRoot "scripts\validate-package.ps1"
$certificationScript = Join-Path $repoRoot "scripts\certify-fresh-machine.ps1"
$nativeBootSetManifestScript = Join-Path $repoRoot "scripts\write-native-boot-set-manifest.ps1"
$assetRoot = Join-Path $repoRoot "scripts\package-assets"
$iconAssetRoot = Join-Path $repoRoot "assets"

function Resolve-QemuRoot {
    param([string]$RequestedRoot)

    if ($RequestedRoot) {
        $resolved = Resolve-Path $RequestedRoot -ErrorAction Stop
        $root = (Get-Item $resolved).FullName
        if (-not (Test-Path (Join-Path $root "qemu-system-x86_64.exe"))) {
            throw "QEMU root does not contain qemu-system-x86_64.exe: $root"
        }
        if (-not (Test-Path (Join-Path $root "qemu-img.exe"))) {
            throw "QEMU root does not contain qemu-img.exe: $root"
        }
        return $root
    }

    $candidates = @(
        "C:\Program Files\qemu",
        "C:\Program Files\QEMU"
    )
    foreach ($candidate in $candidates) {
        if ((Test-Path (Join-Path $candidate "qemu-system-x86_64.exe")) -and
            (Test-Path (Join-Path $candidate "qemu-img.exe"))) {
            return $candidate
        }
    }

    $command = Get-Command "qemu-system-x86_64.exe" -ErrorAction SilentlyContinue
    if ($command) {
        $root = Split-Path -Parent $command.Source
        if (Test-Path (Join-Path $root "qemu-img.exe")) {
            return $root
        }
    }

    return $null
}

function Copy-QemuEngine {
    param(
        [string]$SourceRoot,
        [string]$PackageRoot
    )

    $engineDir = Join-Path $PackageRoot "engine"
    New-Item -ItemType Directory -Force $engineDir | Out-Null
    Copy-Item -Path (Join-Path $SourceRoot "*") -Destination $engineDir -Recurse -Force
    Copy-Item (Join-Path $engineDir "qemu-system-x86_64.exe") (Join-Path $engineDir "pane-engine.exe") -Force

    if (-not (Test-Path (Join-Path $engineDir "pane-engine.exe"))) {
        throw "Failed to create bundled pane-engine.exe."
    }
    if (-not (Test-Path (Join-Path $engineDir "qemu-img.exe"))) {
        throw "Failed to bundle qemu-img.exe."
    }
    Write-Host "Bundled QEMU engine: $engineDir"
}

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
    New-Item -ItemType Directory -Force (Join-Path $packageDir "docs") | Out-Null
    Copy-Item (Join-Path $repoRoot "docs\cli-reference.md") (Join-Path $packageDir "docs\cli-reference.md")
    Copy-Item (Join-Path $repoRoot "docs\mvp-arch.md") (Join-Path $packageDir "docs\mvp-arch.md")
    Copy-Item (Join-Path $repoRoot "docs\vision.md") (Join-Path $packageDir "docs\vision.md")
    Copy-Item (Join-Path $repoRoot "docs\product-contract.md") (Join-Path $packageDir "docs\product-contract.md")
    Copy-Item (Join-Path $repoRoot "docs\native-runtime-architecture.md") (Join-Path $packageDir "docs\native-runtime-architecture.md")
    Copy-Item (Join-Path $repoRoot "docs\vmm-foundation.md") (Join-Path $packageDir "docs\vmm-foundation.md")
    Copy-Item (Join-Path $repoRoot "docs\clean-machine-validation.md") (Join-Path $packageDir "docs\clean-machine-validation.md")
    Copy-Item $validationScript (Join-Path $packageDir "validate-package.ps1")
    Copy-Item $certificationScript (Join-Path $packageDir "certify-fresh-machine.ps1")
    Copy-Item $nativeBootSetManifestScript (Join-Path $packageDir "write-native-boot-set-manifest.ps1")
    Copy-Item (Join-Path $assetRoot "*") $packageDir
    Copy-Item $iconAssetRoot (Join-Path $packageDir "assets") -Recurse

    if ($BundleQemu -ne "Disabled") {
        $resolvedQemuRoot = Resolve-QemuRoot -RequestedRoot $QemuRoot
        if ($resolvedQemuRoot) {
            Copy-QemuEngine -SourceRoot $resolvedQemuRoot -PackageRoot $packageDir
        }
        elseif ($BundleQemu -eq "Required") {
            throw "QEMU was not found. Install QEMU or pass -QemuRoot. Use -BundleQemu Disabled only for developer-only packages."
        }
        else {
            Write-Warning "QEMU was not found; package will rely on PATH, Program Files, or first-run winget installation."
        }
    }

    if ($BaseImagePath) {
        $resolvedBaseImage = Resolve-Path $BaseImagePath -ErrorAction Stop
        $imageDir = Join-Path $packageDir "images"
        $packagedBaseImage = Join-Path $imageDir "arch-base.paneimg"
        New-Item -ItemType Directory -Force $imageDir | Out-Null
        Copy-Item $resolvedBaseImage $packagedBaseImage -Force
        Write-Host "Bundled base OS image: $packagedBaseImage"
    }

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
