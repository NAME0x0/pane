param(
    [string]$SessionName = "pane",
    [switch]$PrintOnly
)

$ErrorActionPreference = "Stop"

$packageRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$paneExe = Join-Path $packageRoot "pane.exe"
if (-not (Test-Path $paneExe)) {
    throw "Expected pane.exe beside $($MyInvocation.MyCommand.Name)."
}

$arguments = @("share", "--session-name", $SessionName)
if ($PrintOnly) {
    $arguments += "--print-only"
}

& $paneExe @arguments
exit $LASTEXITCODE
