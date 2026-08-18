param(
    [string]$Version = "0.0.1",
    [string]$InstallDirectory = "$env:LOCALAPPDATA/Land/bin",
    [string]$BaseUrl = "https://github.com/coder-ka/land/releases/download",
    [switch]$NoPathUpdate
)

$ErrorActionPreference = "Stop"
if ($env:OS -ne "Windows_NT") {
    throw "This installer currently supports Windows only"
}
if ([Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne "X64") {
    throw "This installer currently supports x86_64 only"
}

$asset = "landc-windows-x86_64.exe"
$release = "$($BaseUrl.TrimEnd('/'))/v$Version"
$temporary = Join-Path ([IO.Path]::GetTempPath()) "land-install-$PID"
$download = Join-Path $temporary $asset
$checksum = "$download.sha256"

try {
    New-Item -ItemType Directory -Path $temporary -Force | Out-Null
    Invoke-WebRequest "$release/$asset" -OutFile $download -UseBasicParsing
    Invoke-WebRequest "$release/$asset.sha256" -OutFile $checksum -UseBasicParsing

    $expected = (Get-Content $checksum -Raw).Trim().Split()[0].ToLowerInvariant()
    if ($expected -notmatch "^[0-9a-f]{64}$") {
        throw "The published checksum is invalid"
    }
    $actual = (Get-FileHash $download -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $expected) {
        throw "landc checksum mismatch"
    }

    $destination = [IO.Path]::GetFullPath($InstallDirectory)
    New-Item -ItemType Directory -Path $destination -Force | Out-Null
    Copy-Item $download (Join-Path $destination "landc.exe") -Force

    if (-not $NoPathUpdate) {
        $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
        $entries = @($userPath -split ";" | Where-Object { $_ })
        if (-not ($entries | Where-Object { $_.TrimEnd("\") -ieq $destination.TrimEnd("\") })) {
            $updatedPath = (@($entries) + $destination) -join ";"
            [Environment]::SetEnvironmentVariable("Path", $updatedPath, "User")
        }
        if (-not (($env:Path -split ";") -contains $destination)) {
            $env:Path = "$destination;$env:Path"
        }
    }

    $reportedVersion = & (Join-Path $destination "landc.exe") --version
    if ($reportedVersion -ne "landc $Version") {
        throw "Expected landc $Version, got $reportedVersion"
    }
    Write-Output $reportedVersion
    Write-Output "Installed landc to $destination"
} finally {
    Remove-Item $temporary -Recurse -Force -ErrorAction SilentlyContinue
}
