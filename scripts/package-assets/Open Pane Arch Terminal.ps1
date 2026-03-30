param(
    [string]$Distro = "",
    [string]$User = "",
    [switch]$PrintOnly
)

$ErrorActionPreference = "Stop"

$packageRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$paneExe = Join-Path $packageRoot "pane.exe"
if (-not (Test-Path $paneExe)) {
    throw "Expected pane.exe beside $($MyInvocation.MyCommand.Name)."
}

$arguments = @("terminal")
if (-not [string]::IsNullOrWhiteSpace($Distro)) {
    $arguments += @("--distro", $Distro)
}
if (-not [string]::IsNullOrWhiteSpace($User)) {
    $arguments += @("--user", $User)
}
if ($PrintOnly) {
    $arguments += "--print-only"
}

& $paneExe @arguments
exit $LASTEXITCODE