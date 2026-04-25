param(
    [string]$SessionName = "pane",
    [string]$Distro = "",
    [string]$DesktopEnvironment = "xfce",
    [ValidateSet("durable", "scratch")]
    [string]$SharedStorage = "durable",
    [switch]$DryRun,
    [switch]$NoConnect,
    [switch]$ForceLaunch
)

$ErrorActionPreference = "Stop"

$packageRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$paneExe = Join-Path $packageRoot "pane.exe"
if (-not (Test-Path $paneExe)) {
    throw "Expected pane.exe beside $($MyInvocation.MyCommand.Name)."
}

function Add-DistroArgument {
    param(
        [string[]]$Arguments,
        [string]$Value
    )

    if ([string]::IsNullOrWhiteSpace($Value)) {
        return $Arguments
    }

    return $Arguments + @("--distro", $Value)
}

$shouldReconnect = -not $ForceLaunch -and -not $DryRun
if ($shouldReconnect) {
    try {
        $doctorArguments = @("doctor", "--json", "--de", $DesktopEnvironment, "--session-name", $SessionName, "--skip-bootstrap")
        $doctorArguments = Add-DistroArgument -Arguments $doctorArguments -Value $Distro
        $doctorOutput = & $paneExe @doctorArguments 2>$null
        if ($LASTEXITCODE -eq 0 -and -not [string]::IsNullOrWhiteSpace(($doctorOutput | Out-String))) {
            $doctorReport = $doctorOutput | ConvertFrom-Json
            if ($doctorReport.ready) {
                Write-Host "Reconnecting to the saved Pane session..."
                & $paneExe "connect" "--session-name" $SessionName
                if ($LASTEXITCODE -eq 0) {
                    exit 0
                }
                Write-Warning "Reconnect failed. Falling back to a fresh Arch launch."
            }
        }
    }
    catch {
        Write-Warning "Reconnect readiness probe failed. Falling back to a fresh Arch launch."
    }
}

Write-Host "Launching Pane Arch session..."
$arguments = @("launch", "--de", $DesktopEnvironment, "--session-name", $SessionName, "--shared-storage", $SharedStorage)
$arguments = Add-DistroArgument -Arguments $arguments -Value $Distro
if ($DryRun) {
    $arguments += "--dry-run"
}
if ($NoConnect) {
    $arguments += "--no-connect"
}

& $paneExe @arguments
exit $LASTEXITCODE
