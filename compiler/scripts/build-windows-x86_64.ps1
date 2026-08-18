param(
    [string]$CacheDirectory = "$PSScriptRoot/../.cache/windows-x86_64"
)

$ErrorActionPreference = "Stop"
$llvmVersion = "21.1.8"
$llvmArchiveName = "clang+llvm-$llvmVersion-x86_64-pc-windows-msvc.tar.xz"
$llvmDirectoryName = "clang+llvm-$llvmVersion-x86_64-pc-windows-msvc"
$llvmUrl = "https://github.com/llvm/llvm-project/releases/download/llvmorg-$llvmVersion/clang%2Bllvm-$llvmVersion-x86_64-pc-windows-msvc.tar.xz"
$llvmSha256 = "749d22f565fcd5718dbed06512572d0e5353b502c03fe1f7f17ee8b8aca21a47"
$vcpkgCommit = "62739e5f7b27f2e04abd051a61c4848940204d93"

$compiler = (Resolve-Path "$PSScriptRoot/..").Path
$root = (Resolve-Path "$compiler/..").Path
$cache = [IO.Path]::GetFullPath($CacheDirectory)
$archive = Join-Path $cache $llvmArchiveName
$llvm = Join-Path $cache $llvmDirectoryName
$vcpkg = Join-Path $cache "vcpkg"

New-Item -ItemType Directory -Path $cache -Force | Out-Null
if (-not (Test-Path $archive)) {
    & curl.exe -L --fail --retry 3 -o $archive $llvmUrl
    if ($LASTEXITCODE -ne 0) { throw "LLVM download failed" }
}
if ((Get-FileHash $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $llvmSha256) {
    throw "LLVM archive checksum mismatch"
}
if (-not (Test-Path "$llvm/bin/llvm-config.exe")) {
    & "$env:SystemRoot/System32/tar.exe" -xf $archive -C $cache
    if ($LASTEXITCODE -ne 0) { throw "LLVM extraction failed" }
}

if (-not (Test-Path "$vcpkg/.git")) {
    & git clone --no-checkout https://github.com/microsoft/vcpkg.git $vcpkg
    & git -C $vcpkg fetch --depth 1 origin $vcpkgCommit
    & git -C $vcpkg checkout --detach $vcpkgCommit
    & "$vcpkg/bootstrap-vcpkg.bat" -disableMetrics
}
& "$vcpkg/vcpkg.exe" install libxml2:x64-windows-static
if ($LASTEXITCODE -ne 0) { throw "libxml2 build failed" }

$env:LLVM_CONFIG = "$llvm/bin/llvm-config.exe"
$env:LLVM_DEPENDENCY_DIR = "$vcpkg/installed/x64-windows-static/lib"
$env:RUSTFLAGS = "-C target-feature=+crt-static"
& cargo build --manifest-path "$root/Cargo.toml" --locked --release -p land-compiler --features embedded-backend
if ($LASTEXITCODE -ne 0) { throw "landc build failed" }

New-Item -ItemType Directory -Path "$compiler/dist" -Force | Out-Null
$artifact = "$compiler/dist/landc-windows-x86_64.exe"
Copy-Item "$root/target/release/landc.exe" $artifact -Force
$hash = (Get-FileHash $artifact -Algorithm SHA256).Hash.ToLowerInvariant()
Set-Content "$artifact.sha256" "$hash  landc-windows-x86_64.exe" -NoNewline
Write-Output $artifact
