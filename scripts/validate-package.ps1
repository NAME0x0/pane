param(
    [string]$PackagePath,
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
            return @{
                PackageRoot = $ScriptRoot
                Extracted = $false
            }
        }
        if (Test-Path $repoPackageDir) {
            return @{
                PackageRoot = $repoPackageDir
                Extracted = $false
            }
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
        return @{
            PackageRoot = $item.FullName
            Extracted = $false
        }
    }

    $extractRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("pane-package-" + [guid]::NewGuid().ToString("N"))
    Expand-Archive -LiteralPath $item.FullName -DestinationPath $extractRoot -Force
    return @{
        PackageRoot = $extractRoot
        Extracted = $true
    }
}

function Invoke-PaneCapture {
    param(
        [string]$PaneExe,
        [string[]]$Arguments,
        [string]$OutputPath
    )

    $output = & $PaneExe @Arguments 2>&1
    $exitCode = $LASTEXITCODE
    $raw = ($output | Out-String).TrimEnd()
    Set-Content -Path $OutputPath -Value $raw -Encoding utf8

    if ($exitCode -ne 0) {
        $rendered = if ([string]::IsNullOrWhiteSpace($raw)) { "(no output)" } else { $raw }
        throw "pane.exe $($Arguments -join ' ') failed with exit code $exitCode.`n$rendered"
    }

    return $raw
}

function Invoke-ScriptCapture {
    param(
        [string]$ScriptPath,
        [string[]]$Arguments,
        [string]$OutputPath
    )

    $output = & powershell -NoLogo -NoProfile -ExecutionPolicy Bypass -File $ScriptPath @Arguments 2>&1
    $exitCode = $LASTEXITCODE
    $raw = ($output | Out-String).TrimEnd()
    Set-Content -Path $OutputPath -Value $raw -Encoding utf8

    if ($exitCode -ne 0) {
        $rendered = if ([string]::IsNullOrWhiteSpace($raw)) { "(no output)" } else { $raw }
        throw "$(Split-Path -Leaf $ScriptPath) $($Arguments -join ' ') failed with exit code $exitCode.`n$rendered"
    }

    return $raw
}

function Assert-PackageFile {
    param(
        [string]$PackageRoot,
        [string]$RelativePath
    )

    $path = Join-Path $PackageRoot $RelativePath
    if (-not (Test-Path $path)) {
        throw "Expected package file missing: $RelativePath"
    }
}

function Assert-ShortcutFile {
    param(
        [string]$BasePath,
        [string]$RelativePath
    )

    $path = Join-Path $BasePath $RelativePath
    if (-not (Test-Path $path)) {
        throw "Expected shortcut missing: $path"
    }
}

if (-not $SessionName) {
    $SessionName = "pane-clean-smoke-$PID"
}

$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$resolvedPackage = Resolve-PackageRoot -RequestedPath $PackagePath -ScriptRoot $scriptRoot
$packageRoot = $resolvedPackage.PackageRoot
$cleanupExtracted = $resolvedPackage.Extracted -and -not $KeepExtracted
$artifactRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("pane-validation-" + [guid]::NewGuid().ToString("N"))
$paneAppRoot = Join-Path $env:LOCALAPPDATA "Pane"
$statePath = Join-Path $paneAppRoot "state.json"
$stateBackupPath = Join-Path $artifactRoot "preexisting-state.json"
$hadExistingState = Test-Path $statePath
$paneExe = Join-Path $packageRoot "pane.exe"
$bundlePath = Join-Path $artifactRoot "pane-support.zip"
$controlCenterScript = Join-Path $packageRoot "Pane Control Center.ps1"
$launchScript = Join-Path $packageRoot "Launch Pane Arch.ps1"
$shareScript = Join-Path $packageRoot "Open Pane Shared Folder.ps1"
$terminalScript = Join-Path $packageRoot "Open Pane Arch Terminal.ps1"
$bundleScript = Join-Path $packageRoot "Collect Pane Support Bundle.ps1"
$shortcutScript = Join-Path $packageRoot "Install Pane Shortcuts.ps1"
$shortcutDesktopRoot = Join-Path $artifactRoot "desktop"
$shortcutStartMenuRoot = Join-Path $artifactRoot "start-menu\Pane"
$requiredFiles = @(
    "pane.exe",
    "README.md",
    "LICENSE",
    "mvp-arch.md",
    "phase-1-audit.md",
    "clean-machine-validation.md",
    "vision.md",
    "product-contract.md",
    "native-runtime-architecture.md",
    "validate-package.ps1",
    "certify-fresh-machine.ps1",
    "assets\pane-icon.png",
    "assets\pane-icon.ico",
    "Pane Control Center.ps1",
    "Pane Control Center.cmd",
    "Launch Pane Arch.ps1",
    "Launch Pane Arch.cmd",
    "Open Pane Arch Terminal.ps1",
    "Open Pane Arch Terminal.cmd",
    "Open Pane Shared Folder.ps1",
    "Open Pane Shared Folder.cmd",
    "Collect Pane Support Bundle.ps1",
    "Collect Pane Support Bundle.cmd",
    "Install Pane Shortcuts.ps1",
    "Install Pane Shortcuts.cmd"
)
$expectedShortcuts = @(
    "Pane.lnk",
    "Pane Arch.lnk",
    "Pane Arch Terminal.lnk",
    "Pane Shared Folder.lnk",
    "Pane Support Bundle.lnk"
)

New-Item -ItemType Directory -Force $artifactRoot | Out-Null

foreach ($relativePath in $requiredFiles) {
    Assert-PackageFile -PackageRoot $packageRoot -RelativePath $relativePath
}

if ($hadExistingState) {
    Copy-Item $statePath $stateBackupPath -Force
}

$summary = $null
$bundleEntries = @()
$shortcutFiles = @()

try {
    $status = Invoke-PaneCapture -PaneExe $paneExe -Arguments @("status", "--json") -OutputPath (Join-Path $artifactRoot "status.json") | ConvertFrom-Json
    $init = Invoke-PaneCapture -PaneExe $paneExe -Arguments @("init", "--json", "--dry-run", "--distro-name", "pane-arch") -OutputPath (Join-Path $artifactRoot "init.json") | ConvertFrom-Json
    $initLiveArgs = if ($Distro -eq "pane-arch") {
        @("init", "--json", "--distro-name", $Distro)
    }
    else {
        @("init", "--json", "--existing-distro", $Distro)
    }
    $initLive = Invoke-PaneCapture -PaneExe $paneExe -Arguments $initLiveArgs -OutputPath (Join-Path $artifactRoot "init-live.json") | ConvertFrom-Json
    $onboard = Invoke-PaneCapture -PaneExe $paneExe -Arguments @("onboard", "--json", "--dry-run", "--username", "paneuser", "--session-name", $SessionName) -OutputPath (Join-Path $artifactRoot "onboard.json") | ConvertFrom-Json
    $setupUser = Invoke-PaneCapture -PaneExe $paneExe -Arguments @("setup-user", "--json", "--dry-run", "--username", "paneuser") -OutputPath (Join-Path $artifactRoot "setup-user.json") | ConvertFrom-Json
    $resetReleaseDryRun = Invoke-PaneCapture -PaneExe $paneExe -Arguments @("reset", "--dry-run", "--release-managed-environment", "--session-name", $SessionName) -OutputPath (Join-Path $artifactRoot "reset-release-dry-run.txt")
    $repairDryRun = Invoke-PaneCapture -PaneExe $paneExe -Arguments @("repair", "--dry-run", "--de", $DesktopEnvironment, "--session-name", $SessionName) -OutputPath (Join-Path $artifactRoot "repair-dry-run.txt")
    $updateDryRun = Invoke-PaneCapture -PaneExe $paneExe -Arguments @("update", "--dry-run", "--de", $DesktopEnvironment, "--session-name", $SessionName) -OutputPath (Join-Path $artifactRoot "update-dry-run.txt")
    $doctor = Invoke-PaneCapture -PaneExe $paneExe -Arguments @("doctor", "--json", "--distro", $Distro, "--de", $DesktopEnvironment, "--session-name", $SessionName) -OutputPath (Join-Path $artifactRoot "doctor.json") | ConvertFrom-Json
    $doctorReconnect = Invoke-PaneCapture -PaneExe $paneExe -Arguments @("doctor", "--json", "--distro", $Distro, "--de", $DesktopEnvironment, "--session-name", $SessionName, "--skip-bootstrap") -OutputPath (Join-Path $artifactRoot "doctor-reconnect.json") | ConvertFrom-Json
    $appStatus = Invoke-PaneCapture -PaneExe $paneExe -Arguments @("app-status", "--json", "--session-name", $SessionName) -OutputPath (Join-Path $artifactRoot "app-status.json") | ConvertFrom-Json
    $runtime = Invoke-PaneCapture -PaneExe $paneExe -Arguments @("runtime", "--json", "--prepare", "--create-user-disk", "--session-name", $SessionName, "--capacity-gib", "8") -OutputPath (Join-Path $artifactRoot "runtime.json") | ConvertFrom-Json
    $nativePreflight = Invoke-PaneCapture -PaneExe $paneExe -Arguments @("native-preflight", "--json", "--session-name", $SessionName) -OutputPath (Join-Path $artifactRoot "native-preflight.json") | ConvertFrom-Json
    $nativeLaunchDryRun = Invoke-PaneCapture -PaneExe $paneExe -Arguments @("launch", "--runtime", "pane-owned", "--dry-run", "--session-name", $SessionName) -OutputPath (Join-Path $artifactRoot "native-launch-dry-run.txt")

    if (-not $init.managed_environment -or $init.managed_environment.distro_name -ne "pane-arch") {
        throw "Init dry-run did not resolve the expected Pane-managed provisioning target."
    }
    if ($init.managed_environment.ownership -ne "installed-online") {
        throw "Init dry-run did not exercise the Pane-owned online provisioning path."
    }
    if (-not $initLive.managed_environment -or $initLive.managed_environment.distro_name -ne $Distro) {
        throw "Init live run did not persist the expected managed environment for $Distro."
    }
    if (-not $onboard.managed_environment -or $onboard.managed_environment.distro_name -ne "pane-arch") {
        throw "pane onboard dry-run did not resolve the expected Pane-managed distro target."
    }
    if ($onboard.setup_user.username -ne "paneuser" -or -not $onboard.dry_run) {
        throw "pane onboard dry-run did not include the expected setup-user summary."
    }
    if ($null -ne $onboard.launch_readiness) {
        throw "pane onboard dry-run should skip the final readiness check."
    }
    if ($setupUser.username -ne "paneuser" -or -not $setupUser.dry_run) {
        throw "pane setup-user dry-run did not return the expected onboarding summary."
    }
    
    if (-not $doctor.selected_distro) {
        throw "pane doctor did not resolve a target distro. Review $artifactRoot\doctor.json before treating the package as ready."
    }
    if (-not $doctor.supported_for_mvp) {
        throw "pane doctor resolved '$($doctor.target_distro)' but it is outside the Arch-first MVP boundary. Review $artifactRoot\doctor.json."
    }
    if (-not $doctor.ready) {
        throw "pane doctor reported launch blockers. Review $artifactRoot\doctor.json before treating the package as ready."
    }
    if (-not $doctorReconnect.selected_distro) {
        throw "pane doctor --skip-bootstrap did not resolve a target distro. Review $artifactRoot\doctor-reconnect.json before treating the package as ready."
    }
    if ($null -eq $doctorReconnect.selected_distro.pane_session_assets_ready) {
        throw "pane doctor --skip-bootstrap did not report Pane-managed session asset readiness. Review $artifactRoot\doctor-reconnect.json."
    }
    if ($null -eq $doctorReconnect.selected_distro.user_home_ready) {
        throw "pane doctor --skip-bootstrap did not report user-home readiness. Review $artifactRoot\doctor-reconnect.json."
    }
    if (-not $appStatus.next_action -or -not $appStatus.display) {
        throw "pane app-status did not return app-facing lifecycle and display information. Review $artifactRoot\app-status.json."
    }
    if ($appStatus.display.current_mode -ne "external-mstsc-rdp" -or $appStatus.display.contained_window_available) {
        throw "pane app-status misrepresented the current display transport boundary. Review $artifactRoot\app-status.json."
    }
    if (-not $appStatus.runtime -or $appStatus.runtime.target_engine -ne "pane-owned-os-runtime") {
        throw "pane app-status did not expose the Pane-owned runtime target. Review $artifactRoot\app-status.json."
    }
    if (-not $runtime.prepared -or $runtime.current_engine -ne "wsl-xrdp-bridge" -or $runtime.target_engine -ne "pane-owned-os-runtime") {
        throw "pane runtime did not prepare or report the current/target runtime engines correctly. Review $artifactRoot\runtime.json."
    }
    if ($runtime.storage_budget.requested_capacity_gib -ne 8) {
        throw "pane runtime did not use the expected 8 GiB runtime reservation. Review $artifactRoot\runtime.json."
    }
    if (-not $runtime.native_runtime -or $runtime.native_runtime.requires_wsl -or $runtime.native_runtime.requires_mstsc -or $runtime.native_runtime.requires_xrdp) {
        throw "pane runtime did not expose a WSL/mstsc/XRDP-free native runtime contract. Review $artifactRoot\runtime.json."
    }
    if (-not $runtime.native_host -or $null -eq $runtime.native_runtime.host_ready -or $null -eq $runtime.native_runtime.ready_for_boot_spike) {
        throw "pane runtime did not expose native host preflight state. Review $artifactRoot\runtime.json."
    }
    if (-not $runtime.artifacts.user_disk_ready) {
        throw "pane runtime did not create a valid Pane-owned user disk descriptor. Review $artifactRoot\runtime.json."
    }
    if (-not $nativePreflight.host -or -not $nativePreflight.host.checks) {
        throw "pane native-preflight did not report host checks. Review $artifactRoot\native-preflight.json."
    }
    if (-not $nativePreflight.runtime -or $nativePreflight.runtime.target_engine -ne "pane-owned-os-runtime") {
        throw "pane native-preflight did not include the Pane-owned runtime target. Review $artifactRoot\native-preflight.json."
    }
    if ($nativeLaunchDryRun -notmatch "Pane-Owned Runtime Launch") {
        throw "pane launch --runtime pane-owned --dry-run did not exercise the native runtime path. Review $artifactRoot\native-launch-dry-run.txt."
    }

    $controlCenterOutput = Invoke-ScriptCapture -ScriptPath $controlCenterScript -Arguments @("-SessionName", $SessionName, "-PrintOnly") -OutputPath (Join-Path $artifactRoot "control-center.txt")
    if ($controlCenterOutput -notmatch "Onboard Arch") {
        throw "Pane Control Center did not advertise the onboarding-first app flow."
    }
    if ($controlCenterOutput -notmatch "First Run Wizard") {
        throw "Pane Control Center did not advertise the first-run app flow."
    }
    if ($controlCenterOutput -notmatch "Runtime Space") {
        throw "Pane Control Center did not advertise the dedicated runtime-space foundation."
    }
    if ($controlCenterOutput -notmatch "Native Preview") {
        throw "Pane Control Center did not advertise the native runtime preview."
    }
    if ($controlCenterOutput -notmatch "Native Preflight") {
        throw "Pane Control Center did not advertise native host preflight."
    }
    if ($controlCenterOutput -notmatch "Image Register") {
        throw "Pane Control Center did not advertise base image registration."
    }
    Invoke-ScriptCapture -ScriptPath $launchScript -Arguments @("-SessionName", $SessionName, "-DryRun", "-NoConnect") -OutputPath (Join-Path $artifactRoot "launch-dry-run.txt") | Out-Null
    Invoke-ScriptCapture -ScriptPath $terminalScript -Arguments @("-PrintOnly") -OutputPath (Join-Path $artifactRoot "terminal.txt") | Out-Null
    Invoke-ScriptCapture -ScriptPath $shareScript -Arguments @("-SessionName", $SessionName, "-PrintOnly") -OutputPath (Join-Path $artifactRoot "share.txt") | Out-Null
    Invoke-ScriptCapture -ScriptPath $shortcutScript -Arguments @("-DesktopPath", $shortcutDesktopRoot, "-StartMenuPath", $shortcutStartMenuRoot) -OutputPath (Join-Path $artifactRoot "shortcuts.txt") | Out-Null
    Invoke-ScriptCapture -ScriptPath $bundleScript -Arguments @("-SessionName", $SessionName, "-Distro", $Distro, "-OutputPath", $bundlePath) -OutputPath (Join-Path $artifactRoot "bundle.txt") | Out-Null

    foreach ($shortcutName in $expectedShortcuts) {
        Assert-ShortcutFile -BasePath $shortcutDesktopRoot -RelativePath $shortcutName
        Assert-ShortcutFile -BasePath $shortcutStartMenuRoot -RelativePath $shortcutName
        $shortcutFiles += (Join-Path $shortcutDesktopRoot $shortcutName)
        $shortcutFiles += (Join-Path $shortcutStartMenuRoot $shortcutName)
    }

    if (-not (Test-Path $bundlePath)) {
        throw "Pane support bundle launcher reported success but did not create $bundlePath"
    }

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [System.IO.Compression.ZipFile]::OpenRead($bundlePath)
    try {
        $bundleEntries = $zip.Entries | ForEach-Object { $_.FullName.Replace("\", "/") }
    }
    finally {
        $zip.Dispose()
    }

    $requiredBundleEntries = @(
        "doctor.json",
        "manifest.json",
        "state.json",
        "status.json",
        "shared-directory.txt",
        "workspace/pane-bootstrap.sh",
        "workspace/pane.rdp"
    )
    foreach ($entry in $requiredBundleEntries) {
        if ($entry -notin $bundleEntries) {
            throw "Support bundle is missing expected entry '$entry'."
        }
    }

    $summary = [ordered]@{
        validated_at = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
        package_root = $packageRoot
        artifact_root = $artifactRoot
        distro = $Distro
        desktop_environment = $DesktopEnvironment
        session_name = $SessionName
        wsl_available = $status.wsl_available
        init_managed_distro = $init.managed_environment.distro_name
        init_ownership = $init.managed_environment.ownership
        live_managed_distro = $initLive.managed_environment.distro_name
        onboard = $onboard
        doctor_ready = $doctor.ready
        doctor_supported_for_mvp = $doctor.supported_for_mvp
        app_phase = $appStatus.phase
        app_next_action = $appStatus.next_action
        runtime_prepared = $runtime.prepared
        runtime_target = $runtime.target_engine
        runtime_capacity_gib = $runtime.storage_budget.requested_capacity_gib
        runtime_user_disk_ready = $runtime.artifacts.user_disk_ready
        native_preflight_ready = $nativePreflight.ready_for_boot_spike
        native_host_ready = $runtime.native_runtime.host_ready
        native_runtime_dry_run = ($nativeLaunchDryRun -match "Pane-Owned Runtime Launch")
        reconnect_session_assets_ready = $doctorReconnect.selected_distro.pane_session_assets_ready
        reconnect_user_home_ready = $doctorReconnect.selected_distro.user_home_ready
        selected_distro = $doctor.target_distro
        setup_user = $setupUser
        bundle_path = $bundlePath
        bundle_entries = $bundleEntries
        shortcut_files = $shortcutFiles
    }
    $summary | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $artifactRoot "summary.json") -Encoding utf8

    Write-Host "Pane Package Validation"
    Write-Host "  Package Root   $packageRoot"
    Write-Host "  Artifact Root  $artifactRoot"
    Write-Host "  Distro         $Distro"
    Write-Host "  Session        $SessionName"
    Write-Host "  Bundle         $bundlePath"
    Write-Host "  Doctor Ready   $($doctor.ready)"
    Write-Host "  Supported MVP  $($doctor.supported_for_mvp)"
    Write-Host "  Shortcuts      $($shortcutFiles.Count)"
}
finally {
    try {
        & $paneExe reset --session-name $SessionName --purge-shared *> $null
    }
    catch {
    }

    if ($hadExistingState) {
        New-Item -ItemType Directory -Force (Split-Path -Parent $statePath) | Out-Null
        Copy-Item $stateBackupPath $statePath -Force
    }
    else {
        Remove-Item $statePath -Force -ErrorAction SilentlyContinue
    }

    if ($cleanupExtracted) {
        Remove-Item $packageRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}













