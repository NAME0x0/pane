param(
    [string]$DesktopPath = [Environment]::GetFolderPath("Desktop"),
    [string]$StartMenuPath = (Join-Path ([Environment]::GetFolderPath("Programs")) "Pane"),
    [switch]$StartMenuOnly,
    [switch]$PrintOnly
)

$ErrorActionPreference = "Stop"

$packageRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$paneExe = Join-Path $packageRoot "pane.exe"
if (-not (Test-Path $paneExe)) {
    throw "Expected pane.exe inside the Pane package directory."
}

$shortcutSpecs = @(
    @{
        Name = "Pane.lnk"
        Target = $paneExe
        Description = "Open the Pane control center."
    },
    @{
        Name = "Pane Arch.lnk"
        Target = Join-Path $packageRoot "Launch Pane Arch.cmd"
        Description = "Launch the Pane-managed Arch desktop."
    },
    @{
        Name = "Pane Arch Terminal.lnk"
        Target = Join-Path $packageRoot "Open Pane Arch Terminal.cmd"
        Description = "Open a terminal in the Pane-managed Arch environment."
    },
    @{
        Name = "Pane Shared Folder.lnk"
        Target = Join-Path $packageRoot "Open Pane Shared Folder.cmd"
        Description = "Open PaneShared storage."
    },
    @{
        Name = "Pane Support Bundle.lnk"
        Target = Join-Path $packageRoot "Collect Pane Support Bundle.cmd"
        Description = "Collect a Pane support bundle."
    }
)

foreach ($shortcut in $shortcutSpecs) {
    if (-not (Test-Path $shortcut.Target)) {
        throw "Expected package entrypoint missing: $($shortcut.Target)"
    }
}

$destinations = @()
if (-not $StartMenuOnly) {
    $destinations += @{ Label = "Desktop"; Path = $DesktopPath }
}
$destinations += @{ Label = "Start Menu"; Path = $StartMenuPath }

if ($PrintOnly) {
    Write-Host "Pane Shortcut Targets"
    foreach ($destination in $destinations) {
        foreach ($shortcut in $shortcutSpecs) {
            Write-Host "  [$($destination.Label)] $(Join-Path $destination.Path $shortcut.Name)"
        }
    }
    exit 0
}

$wshShell = New-Object -ComObject WScript.Shell
foreach ($destination in $destinations) {
    New-Item -ItemType Directory -Force $destination.Path | Out-Null
    foreach ($shortcut in $shortcutSpecs) {
        $shortcutPath = Join-Path $destination.Path $shortcut.Name
        $link = $wshShell.CreateShortcut($shortcutPath)
        $link.TargetPath = $shortcut.Target
        $link.WorkingDirectory = $packageRoot
        $link.Description = $shortcut.Description
        $link.IconLocation = "$paneExe,0"
        $link.Save()
        Write-Host "Created $shortcutPath"
    }
}

Write-Host "Pane shortcuts installed."
