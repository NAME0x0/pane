param(
    [string]$SessionName = "pane",
    [string]$Distro = "",
    [string]$OutputPath
)

$ErrorActionPreference = "Stop"

$packageRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$paneExe = Join-Path $packageRoot "pane.exe"
if (-not (Test-Path $paneExe)) {
    throw "Expected pane.exe beside $($MyInvocation.MyCommand.Name)."
}

$arguments = @("bundle", "--session-name", $SessionName)
if (-not [string]::IsNullOrWhiteSpace($Distro)) {
    $arguments += @("--distro", $Distro)
}
if ($OutputPath) {
    $arguments += @("--output", $OutputPath)
}

& $paneExe @arguments
$exitCode = $LASTEXITCODE
if ($exitCode -ne 0) {
    exit $exitCode
}

if ($OutputPath) {
    $resolved = Resolve-Path $OutputPath -ErrorAction SilentlyContinue
    if ($resolved) {
        Write-Host "Saved bundle at $($resolved.Path)"
    }
}

exit 0
