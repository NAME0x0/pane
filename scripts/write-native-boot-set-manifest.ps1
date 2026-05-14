param(
    [Parameter(Mandatory = $true)]
    [string]$BaseImage,
    [Parameter(Mandatory = $true)]
    [string]$Kernel,
    [Parameter(Mandatory = $true)]
    [string]$Initramfs,
    [string]$OutputPath = "pane-native-boot-set.json",
    [string]$KernelCmdline = "console=ttyS0 panic=-1",
    [ValidateSet("arch")]
    [string]$DistroFamily = "arch",
    [switch]$Force,
    [switch]$Register,
    [string]$PaneExe
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($PaneExe)) {
    $PaneExe = Join-Path $PSScriptRoot "pane.exe"
}

function Resolve-RequiredFile {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [Parameter(Mandatory = $true)]
        [string]$Label
    )

    $resolved = Resolve-Path -LiteralPath $Path -ErrorAction Stop
    $item = Get-Item -LiteralPath $resolved.ProviderPath
    if ($item.PSIsContainer) {
        throw "$Label must be a file, got directory: $($item.FullName)"
    }
    return $item.FullName
}

function Get-Sha256Hex {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    return (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
}

function ConvertTo-ManifestPath {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ArtifactPath,
        [Parameter(Mandatory = $true)]
        [string]$ManifestDirectory
    )

    $artifactFullPath = [System.IO.Path]::GetFullPath($ArtifactPath)
    $manifestFullDirectory = [System.IO.Path]::GetFullPath($ManifestDirectory)
    $manifestRoot = $manifestFullDirectory.TrimEnd([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
    $manifestRootUri = [System.Uri]::new($manifestRoot)
    $artifactUri = [System.Uri]::new($artifactFullPath)

    if ($manifestRootUri.IsBaseOf($artifactUri)) {
        return [System.Uri]::UnescapeDataString($manifestRootUri.MakeRelativeUri($artifactUri).ToString())
    }

    return $artifactFullPath
}

$outputFullPath = [System.IO.Path]::GetFullPath($OutputPath)
$outputDirectory = [System.IO.Path]::GetDirectoryName($outputFullPath)
if ([string]::IsNullOrWhiteSpace($outputDirectory)) {
    $outputDirectory = (Get-Location).Path
}

if ((Test-Path -LiteralPath $outputFullPath) -and -not $Force) {
    throw "Manifest already exists at $outputFullPath. Pass -Force to replace it."
}

New-Item -ItemType Directory -Force $outputDirectory | Out-Null

$baseImagePath = Resolve-RequiredFile -Path $BaseImage -Label "BaseImage"
$kernelPath = Resolve-RequiredFile -Path $Kernel -Label "Kernel"
$initramfsPath = Resolve-RequiredFile -Path $Initramfs -Label "Initramfs"

$manifest = [ordered]@{
    schema_version = 1
    distro_family = $DistroFamily
    base_image = ConvertTo-ManifestPath -ArtifactPath $baseImagePath -ManifestDirectory $outputDirectory
    base_image_sha256 = Get-Sha256Hex -Path $baseImagePath
    kernel = ConvertTo-ManifestPath -ArtifactPath $kernelPath -ManifestDirectory $outputDirectory
    kernel_sha256 = Get-Sha256Hex -Path $kernelPath
    initramfs = ConvertTo-ManifestPath -ArtifactPath $initramfsPath -ManifestDirectory $outputDirectory
    initramfs_sha256 = Get-Sha256Hex -Path $initramfsPath
    kernel_cmdline = $KernelCmdline
}

$manifestJson = $manifest | ConvertTo-Json -Depth 4
$utf8NoBom = [System.Text.UTF8Encoding]::new($false)
[System.IO.File]::WriteAllText($outputFullPath, $manifestJson + [Environment]::NewLine, $utf8NoBom)
Write-Host "Native boot-set manifest: $outputFullPath"

if ($Register) {
    $paneFullPath = Resolve-RequiredFile -Path $PaneExe -Label "PaneExe"
    $paneArgs = @("runtime", "--register-native-boot-set-manifest", $outputFullPath)
    if ($Force) {
        $paneArgs += "--force"
    }

    & $paneFullPath @paneArgs
    if ($LASTEXITCODE -ne 0) {
        throw "pane.exe failed to register the native boot-set manifest with exit code $LASTEXITCODE."
    }
}
