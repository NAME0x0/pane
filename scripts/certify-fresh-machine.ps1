param(
    [string]$PackagePath,
    [ValidateSet("PackageOnly", "FreshMachinePreflight", "LiveArchSession")]
    [string]$Mode = "PackageOnly",
    [string]$Distro = "pane-arch",
    [string]$DesktopEnvironment = "xfce",
    [string]$SessionName,
    [switch]$KeepExtracted
)

$ErrorActionPreference = "Stop"

function Resolve-PackageRoot {
    param(
        [string]$RequestedPath,
        [string]$ScriptRoot
    )

    $repoRoot = Split-Path -Parent $ScriptRoot
    $repoPackageDir = Join-Path $repoRoot "dist\pane-windows-x86_64"
    $repoArchive = Join-Path $repoRoot "dist\pane-windows-x86_64.zip"

    if (-not $RequestedPath) {
        if (Test-Path (Join-Path $ScriptRoot "pane.exe")) {
            return @{ PackageRoot = $ScriptRoot; Extracted = $false }
        }
        if (Test-Path $repoPackageDir) {
            return @{ PackageRoot = $repoPackageDir; Extracted = $false }
        }
        if (Test-Path $repoArchive) {
            $RequestedPath = $repoArchive
        }
        else {
            throw "Could not find a packaged Pane build. Pass -PackagePath with either the package directory or pane-windows-x86_64.zip."
        }
    }

    $resolved = Resolve-Path $RequestedPath -ErrorAction Stop
    $item = Get-Item $resolved
    if ($item.PSIsContainer) {
        return @{ PackageRoot = $item.FullName; Extracted = $false }
    }

    $extractRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("pane-cert-package-" + [guid]::NewGuid().ToString("N"))
    Expand-Archive -LiteralPath $item.FullName -DestinationPath $extractRoot -Force
    return @{ PackageRoot = $extractRoot; Extracted = $true }
}

function Invoke-Capture {
    param(
        [string]$OutputPath,
        [scriptblock]$Body
    )

    $output = & $Body 2>&1
    $exitCode = $LASTEXITCODE
    $raw = (($output | Out-String) -replace "`0", "").TrimEnd()
    Set-Content -Path $OutputPath -Value $raw -Encoding utf8
    return @{ ExitCode = $exitCode; Output = $raw }
}

function Assert-Success {
    param(
        [hashtable]$Result,
        [string]$Label
    )

    if ($Result.ExitCode -ne 0) {
        $rendered = if ([string]::IsNullOrWhiteSpace($Result.Output)) { "(no output)" } else { $Result.Output }
        throw "$Label failed with exit code $($Result.ExitCode).`n$rendered"
    }
}

function Assert-File {
    param(
        [string]$BasePath,
        [string]$RelativePath
    )

    $path = Join-Path $BasePath $RelativePath
    if (-not (Test-Path $path)) {
        throw "Expected package file missing: $RelativePath"
    }
}

function Get-CommandPathOrNull {
    param([string]$Name)

    $command = Get-Command $Name -ErrorAction SilentlyContinue
    if ($command) {
        return $command.Source
    }
    return $null
}

function Write-CertificationSummary {
    param(
        [string]$Path,
        [string]$Mode,
        [string]$PackageRoot,
        [string]$ArtifactRoot,
        [string]$SessionName,
        [string]$Distro,
        [string]$DesktopEnvironment,
        [hashtable]$Checks
    )

    $summary = [ordered]@{
        certified_at = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
        mode = $Mode
        package_root = $PackageRoot
        artifact_root = $ArtifactRoot
        session_name = $SessionName
        distro = $Distro
        desktop_environment = $DesktopEnvironment
        checks = $Checks
    }
    $summary | ConvertTo-Json -Depth 6 | Set-Content $Path -Encoding utf8
}

if (-not $SessionName) {
    $SessionName = "pane-cert-$PID"
}

$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$resolvedPackage = Resolve-PackageRoot -RequestedPath $PackagePath -ScriptRoot $scriptRoot
$packageRoot = $resolvedPackage.PackageRoot
$cleanupExtracted = $resolvedPackage.Extracted -and -not $KeepExtracted
$artifactRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("pane-certification-" + [guid]::NewGuid().ToString("N"))
$sandboxLocalAppData = Join-Path $artifactRoot "localappdata"
$paneExe = Join-Path $packageRoot "pane.exe"
$controlCenterScript = Join-Path $packageRoot "Pane Control Center.ps1"
$launchScript = Join-Path $packageRoot "Launch Pane Arch.ps1"
$shareScript = Join-Path $packageRoot "Open Pane Shared Folder.ps1"
$terminalScript = Join-Path $packageRoot "Open Pane Arch Terminal.ps1"
$shortcutScript = Join-Path $packageRoot "Install Pane Shortcuts.ps1"
$validateScript = Join-Path $packageRoot "validate-package.ps1"
$certifyScript = Join-Path $packageRoot "certify-fresh-machine.ps1"
$standaloneRoot = Join-Path $artifactRoot "standalone"
$shortcutDesktopRoot = Join-Path $artifactRoot "desktop"
$shortcutStartMenuRoot = Join-Path $artifactRoot "start-menu\Pane"

New-Item -ItemType Directory -Force $artifactRoot | Out-Null
New-Item -ItemType Directory -Force $sandboxLocalAppData | Out-Null

$requiredFiles = @(
    "pane.exe",
    "README.md",
    "LICENSE",
    "mvp-arch.md",
    "clean-machine-validation.md",
    "vision.md",
    "product-contract.md",
    "native-runtime-architecture.md",
    "validate-package.ps1",
    "certify-fresh-machine.ps1",
    "Pane Control Center.ps1",
    "Launch Pane Arch.ps1",
    "Open Pane Shared Folder.ps1",
    "Open Pane Arch Terminal.ps1",
    "Install Pane Shortcuts.ps1",
    "assets\pane-icon.png",
    "assets\pane-icon.ico"
)

$checks = [ordered]@{}
$oldLocalAppData = $env:LOCALAPPDATA

try {
    foreach ($relative in $requiredFiles) {
        Assert-File -BasePath $packageRoot -RelativePath $relative
    }
    $checks.package_contents = "pass"

    $env:LOCALAPPDATA = $sandboxLocalAppData

    $help = Invoke-Capture -OutputPath (Join-Path $artifactRoot "pane-help.txt") -Body { & $paneExe --help }
    Assert-Success -Result $help -Label "pane.exe --help"
    $checks.pane_help = "pass"

    $environments = Invoke-Capture -OutputPath (Join-Path $artifactRoot "environments.txt") -Body { & $paneExe environments }
    Assert-Success -Result $environments -Label "pane.exe environments"
    $checks.environments = "pass"

    $appStatus = Invoke-Capture -OutputPath (Join-Path $artifactRoot "app-status.json") -Body {
        & $paneExe app-status --json --session-name $SessionName
    }
    Assert-Success -Result $appStatus -Label "pane app-status"
    $appStatusReport = $appStatus.Output | ConvertFrom-Json
    if (-not $appStatusReport.display -or $appStatusReport.display.current_mode -ne "external-mstsc-rdp") {
        throw "pane app-status did not report the current external RDP display bridge."
    }
    if ($appStatusReport.display.contained_window_available) {
        throw "pane app-status must not claim the contained display window is available yet."
    }
    $checks.app_status = "pass"

    $runtime = Invoke-Capture -OutputPath (Join-Path $artifactRoot "runtime.json") -Body {
        & $paneExe runtime --json --prepare --create-user-disk --create-serial-boot-image --session-name $SessionName --capacity-gib 8
    }
    Assert-Success -Result $runtime -Label "pane runtime --prepare"
    $runtimeReport = $runtime.Output | ConvertFrom-Json
    if (-not $runtimeReport.prepared) {
        throw "pane runtime --prepare did not report prepared runtime storage."
    }
    if ($runtimeReport.storage_budget.requested_capacity_gib -ne 8) {
        throw "pane runtime did not use the expected 8 GiB runtime reservation."
    }
    if ($runtimeReport.current_engine -ne "wsl-xrdp-bridge" -or $runtimeReport.target_engine -ne "pane-owned-os-runtime") {
        throw "pane runtime did not report the current bridge and target Pane-owned runtime engines."
    }
    if (-not $runtimeReport.native_runtime -or $runtimeReport.native_runtime.requires_wsl -or $runtimeReport.native_runtime.requires_mstsc -or $runtimeReport.native_runtime.requires_xrdp) {
        throw "pane runtime did not expose a WSL/mstsc/XRDP-free native runtime contract."
    }
    if (-not $runtimeReport.native_host -or $null -eq $runtimeReport.native_runtime.host_ready -or $null -eq $runtimeReport.native_runtime.ready_for_boot_spike) {
        throw "pane runtime did not expose native host preflight state."
    }
    if (-not (Test-Path $runtimeReport.directories.manifest)) {
        throw "pane runtime --prepare did not write the runtime manifest."
    }
    if (-not $runtimeReport.artifacts.user_disk_ready) {
        throw "pane runtime --create-user-disk did not create a valid Pane-owned user disk descriptor."
    }
    if (-not $runtimeReport.artifacts.serial_boot_image_ready) {
        throw "pane runtime --create-serial-boot-image did not create a valid Pane-owned serial boot image."
    }
    $checks.runtime_prepare = "pass"

    $nativePreflight = Invoke-Capture -OutputPath (Join-Path $artifactRoot "native-preflight.json") -Body {
        & $paneExe native-preflight --json --session-name $SessionName
    }
    Assert-Success -Result $nativePreflight -Label "pane native-preflight"
    $nativePreflightReport = $nativePreflight.Output | ConvertFrom-Json
    if (-not $nativePreflightReport.host -or -not $nativePreflightReport.host.checks) {
        throw "pane native-preflight did not report host checks."
    }
    if (-not $nativePreflightReport.runtime -or $nativePreflightReport.runtime.target_engine -ne "pane-owned-os-runtime") {
        throw "pane native-preflight did not include the Pane-owned runtime target."
    }
    $checks.native_preflight = "pass"

    $nativeBootSpike = Invoke-Capture -OutputPath (Join-Path $artifactRoot "native-boot-spike.json") -Body {
        & $paneExe native-boot-spike --json --session-name $SessionName
    }
    Assert-Success -Result $nativeBootSpike -Label "pane native-boot-spike"
    $nativeBootSpikeReport = $nativeBootSpike.Output | ConvertFrom-Json
    if (-not $nativeBootSpikeReport.partition_smoke -or $nativeBootSpikeReport.partition_smoke.status -ne "planned") {
        throw "pane native-boot-spike did not report a safe planned partition smoke by default."
    }
    $checks.native_boot_spike_plan = "pass"

    $nativeLaunch = Invoke-Capture -OutputPath (Join-Path $artifactRoot "native-launch-dry-run.txt") -Body {
        & $paneExe launch --runtime pane-owned --dry-run --session-name $SessionName
    }
    Assert-Success -Result $nativeLaunch -Label "pane launch --runtime pane-owned --dry-run"
    if ($nativeLaunch.Output -notmatch "Pane-Owned Runtime Launch") {
        throw "pane launch --runtime pane-owned --dry-run did not exercise the native runtime path."
    }
    $checks.native_runtime_launch_dry_run = "pass"

    $doctorNoWrite = Invoke-Capture -OutputPath (Join-Path $artifactRoot "doctor-no-write.json") -Body {
        & $paneExe doctor --json --distro $Distro --session-name $SessionName --de $DesktopEnvironment --no-connect --no-write
    }
    Assert-Success -Result $doctorNoWrite -Label "pane doctor --no-write"
    $doctorNoWriteReport = $doctorNoWrite.Output | ConvertFrom-Json
    if ($doctorNoWriteReport.write_probes_enabled) {
        throw "pane doctor --no-write reported write probes as enabled."
    }
    if (Test-Path (Join-Path $sandboxLocalAppData "Pane\sessions\$SessionName")) {
        throw "pane doctor --no-write created a session workspace."
    }
    $checks.doctor_no_write = "pass"

    $initDryRun = Invoke-Capture -OutputPath (Join-Path $artifactRoot "init-dry-run.json") -Body {
        & $paneExe init --json --dry-run --distro-name $Distro
    }
    Assert-Success -Result $initDryRun -Label "pane init --dry-run"
    $checks.init_dry_run = "pass"

    $onboardDryRun = Invoke-Capture -OutputPath (Join-Path $artifactRoot "onboard-dry-run.json") -Body {
        & $paneExe onboard --json --dry-run --username paneuser --session-name $SessionName --distro-name $Distro
    }
    Assert-Success -Result $onboardDryRun -Label "pane onboard --dry-run"
    $checks.onboard_dry_run = "pass"

    $controlCenter = Invoke-Capture -OutputPath (Join-Path $artifactRoot "control-center.txt") -Body {
        & powershell -NoLogo -NoProfile -ExecutionPolicy Bypass -File $controlCenterScript -SessionName $SessionName -PrintOnly
    }
    Assert-Success -Result $controlCenter -Label "Pane Control Center -PrintOnly"
    if ($controlCenter.Output -notmatch "Shared Storage") {
        throw "Pane Control Center did not report the shared-storage policy."
    }
    if ($controlCenter.Output -notmatch "Native Preview") {
        throw "Pane Control Center did not advertise the native runtime preview."
    }
    if ($controlCenter.Output -notmatch "Native Preflight") {
        throw "Pane Control Center did not advertise native host preflight."
    }
    if ($controlCenter.Output -notmatch "Boot Spike") {
        throw "Pane Control Center did not advertise the native boot-spike smoke test."
    }
    if ($controlCenter.Output -notmatch "Image Register") {
        throw "Pane Control Center did not advertise base image registration."
    }
    $checks.control_center = "pass"

    $launch = Invoke-Capture -OutputPath (Join-Path $artifactRoot "launch-dry-run.txt") -Body {
        & powershell -NoLogo -NoProfile -ExecutionPolicy Bypass -File $launchScript -SessionName $SessionName -Distro $Distro -DesktopEnvironment $DesktopEnvironment -SharedStorage scratch -DryRun -NoConnect
    }
    Assert-Success -Result $launch -Label "Launch Pane Arch -DryRun"
    $rdpProfile = Join-Path $sandboxLocalAppData "Pane\sessions\$SessionName\pane.rdp"
    if (-not (Test-Path $rdpProfile)) {
        throw "Launch dry-run did not create the expected RDP profile at $rdpProfile"
    }
    $rdpText = Get-Content -LiteralPath $rdpProfile -Raw
    foreach ($line in @("compression:i:1", "networkautodetect:i:0", "bitmapcachepersistenable:i:1", "use multimon:i:0")) {
        if ($rdpText -notmatch [regex]::Escape($line)) {
            throw "RDP profile is missing expected latency setting: $line"
        }
    }
    $checks.launch_dry_run = "pass"

    $share = Invoke-Capture -OutputPath (Join-Path $artifactRoot "share.txt") -Body {
        & powershell -NoLogo -NoProfile -ExecutionPolicy Bypass -File $shareScript -SessionName $SessionName -SharedStorage scratch -PrintOnly
    }
    Assert-Success -Result $share -Label "Open Pane Shared Folder -PrintOnly"
    $checks.share = "pass"

    $terminal = Invoke-Capture -OutputPath (Join-Path $artifactRoot "terminal.txt") -Body {
        & powershell -NoLogo -NoProfile -ExecutionPolicy Bypass -File $terminalScript -PrintOnly
    }
    Assert-Success -Result $terminal -Label "Open Pane Arch Terminal -PrintOnly"
    $checks.terminal = "pass"

    $shortcuts = Invoke-Capture -OutputPath (Join-Path $artifactRoot "shortcuts.txt") -Body {
        & powershell -NoLogo -NoProfile -ExecutionPolicy Bypass -File $shortcutScript -DesktopPath $shortcutDesktopRoot -StartMenuPath $shortcutStartMenuRoot
    }
    Assert-Success -Result $shortcuts -Label "Install Pane Shortcuts"
    foreach ($shortcut in @("Pane.lnk", "Pane Arch.lnk", "Pane Shared Folder.lnk")) {
        Assert-File -BasePath $shortcutDesktopRoot -RelativePath $shortcut
        Assert-File -BasePath $shortcutStartMenuRoot -RelativePath $shortcut
    }
    $checks.shortcuts = "pass"

    New-Item -ItemType Directory -Force $standaloneRoot | Out-Null
    Copy-Item $paneExe (Join-Path $standaloneRoot "pane.exe") -Force
    $standaloneExe = Join-Path $standaloneRoot "pane.exe"
    $env:PANE_APP_HYDRATE_ONLY = "1"
    $standalone = Invoke-Capture -OutputPath (Join-Path $artifactRoot "standalone-hydrate.txt") -Body {
        & $standaloneExe
    }
    Assert-Success -Result $standalone -Label "standalone pane.exe hydration"
    if (-not (Test-Path (Join-Path $sandboxLocalAppData "Pane\app\Pane Control Center.ps1"))) {
        throw "Standalone pane.exe did not hydrate the Control Center into LOCALAPPDATA."
    }
    $checks.standalone_hydration = "pass"

    if ($Mode -ne "PackageOnly") {
        $wslPath = Get-CommandPathOrNull "wsl.exe"
        $mstscPath = Get-CommandPathOrNull "mstsc.exe"
        $checks.wsl_exe = if ($wslPath) { "pass" } else { "fail" }
        $checks.mstsc = if ($mstscPath) { "pass" } else { "fail" }

        $wslHelp = if ($wslPath) {
            Invoke-Capture -OutputPath (Join-Path $artifactRoot "wsl-help.txt") -Body { & wsl.exe --help }
        }
        else {
            @{ ExitCode = 1; Output = "" }
        }
        $requiredWslFlags = @("--name", "--location", "--no-launch", "--web-download")
        $missingFlags = @()
        foreach ($flag in $requiredWslFlags) {
            if ($wslHelp.Output -notmatch [regex]::Escape($flag)) {
                $missingFlags += $flag
            }
        }
        $checks.wsl_online_install_flags = if ($missingFlags.Count -eq 0) { "pass" } else { "fail: $($missingFlags -join ', ')" }

        $wslStatus = if ($wslPath) {
            Invoke-Capture -OutputPath (Join-Path $artifactRoot "wsl-status.txt") -Body { & wsl.exe --status }
        }
        else {
            @{ ExitCode = 1; Output = "" }
        }
        $checks.wsl_status_exit_code = $wslStatus.ExitCode

        if (-not $wslPath -or -not $mstscPath -or $missingFlags.Count -gt 0) {
            Write-CertificationSummary `
                -Path (Join-Path $artifactRoot "summary.json") `
                -Mode $Mode `
                -PackageRoot $packageRoot `
                -ArtifactRoot $artifactRoot `
                -SessionName $SessionName `
                -Distro $Distro `
                -DesktopEnvironment $DesktopEnvironment `
                -Checks $checks
            throw "Fresh-machine preflight failed. Review $artifactRoot\summary.json."
        }
    }

    if ($Mode -eq "LiveArchSession") {
        if (-not (Test-Path $validateScript)) {
            throw "LiveArchSession mode requires validate-package.ps1 in the package root."
        }
        $env:LOCALAPPDATA = $oldLocalAppData
        $live = Invoke-Capture -OutputPath (Join-Path $artifactRoot "live-arch-session.txt") -Body {
            & powershell -NoLogo -NoProfile -ExecutionPolicy Bypass -File $validateScript -PackagePath $packageRoot -Distro $Distro -DesktopEnvironment $DesktopEnvironment -SessionName $SessionName
        }
        Assert-Success -Result $live -Label "validate-package.ps1 LiveArchSession"
        $checks.live_arch_session = "pass"
    }

    Write-CertificationSummary `
        -Path (Join-Path $artifactRoot "summary.json") `
        -Mode $Mode `
        -PackageRoot $packageRoot `
        -ArtifactRoot $artifactRoot `
        -SessionName $SessionName `
        -Distro $Distro `
        -DesktopEnvironment $DesktopEnvironment `
        -Checks $checks

    Write-Host "Pane Fresh-Machine Certification"
    Write-Host "  Mode          $Mode"
    Write-Host "  Package Root  $packageRoot"
    Write-Host "  Artifact Root $artifactRoot"
    Write-Host "  Session       $SessionName"
    foreach ($entry in $checks.GetEnumerator()) {
        Write-Host ("  {0,-24} {1}" -f $entry.Key, $entry.Value)
    }
}
finally {
    Remove-Item Env:\PANE_APP_HYDRATE_ONLY -ErrorAction SilentlyContinue
    $env:LOCALAPPDATA = $oldLocalAppData

    if ($cleanupExtracted) {
        Remove-Item $packageRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}
