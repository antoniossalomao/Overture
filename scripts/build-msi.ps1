param(
    [switch]$SkipRedistDownload
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

$redistDir = Join-Path $repoRoot "redist"
New-Item -ItemType Directory -Force -Path $redistDir | Out-Null

# 1. Stage wgpu HLSL/DX12 runtime helper (d3dcompiler_47.dll)
$d3dDest = Join-Path $redistDir "d3dcompiler_47.dll"
if (-not (Test-Path $d3dDest)) {
    $sysD3d = Join-Path $env:WINDIR "System32\d3dcompiler_47.dll"
    if (Test-Path $sysD3d) {
        Copy-Item $sysD3d -Destination $d3dDest -Force
        Write-Host "Staged d3dcompiler_47.dll from System32"
    }
}

# 2. Stage libmpv-2.dll runtime
$mpvDest = Join-Path $redistDir "libmpv-2.dll"
if (-not (Test-Path $mpvDest)) {
    $cachedMpv = Join-Path $env:USERPROFILE ".overture-build-tools\mpv-dev\libmpv-2.dll"
    if (Test-Path $cachedMpv) {
        Copy-Item $cachedMpv -Destination $mpvDest -Force
        Write-Host "Staged libmpv-2.dll from local cache"
    } elseif (-not $SkipRedistDownload) {
        Write-Host "Downloading official Windows x86_64 libmpv-2.dll runtime..."
        $toolsDir = Join-Path $env:USERPROFILE ".overture-build-tools\mpv-dev"
        New-Item -ItemType Directory -Force -Path $toolsDir | Out-Null
        $rels = Invoke-RestMethod -Uri "https://api.github.com/repos/shinchiro/mpv-winbuild-cmake/releases?per_page=2" -Headers @{"User-Agent"="overture-build"}
        $asset = $rels[0].assets | Where-Object { $_.name -match "^mpv-dev-x86_64-20.*\.7z$" } | Select-Object -First 1
        $archive = Join-Path $toolsDir "mpv-dev.7z"
        Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $archive
        & "$env:WINDIR\System32\tar.exe" -xf $archive -C $toolsDir
        Copy-Item (Join-Path $toolsDir "libmpv-2.dll") -Destination $mpvDest -Force
    }
}

# 3. Ensure WiX Toolset 3.14 is on PATH and WIX\bin layout matches cargo-wix expectation
$portableWixRoot = Join-Path $env:USERPROFILE ".overture-build-tools\wix314-root"
$portableWixBin = Join-Path $portableWixRoot "bin"
if (-not (Get-Command candle.exe -ErrorAction SilentlyContinue)) {
    if (-not (Test-Path (Join-Path $portableWixBin "candle.exe"))) {
        New-Item -ItemType Directory -Force -Path $portableWixBin | Out-Null
        $legacyFlat = Join-Path $env:USERPROFILE ".overture-build-tools\wix314"
        if (Test-Path (Join-Path $legacyFlat "candle.exe")) {
            Copy-Item (Join-Path $legacyFlat "*") -Destination $portableWixBin -Recurse -Force
        } else {
            Write-Host "Downloading portable WiX Toolset 3.14.1..."
            $wixZip = Join-Path $env:USERPROFILE ".overture-build-tools\wix314-binaries.zip"
            if (-not (Test-Path $wixZip)) {
                Invoke-WebRequest -Uri "https://github.com/wixtoolset/wix3/releases/download/wix3141rtm/wix314-binaries.zip" -OutFile $wixZip
            }
            Expand-Archive -Path $wixZip -DestinationPath $portableWixBin -Force
        }
    }
    $env:PATH = "$portableWixBin;$env:PATH"
    $env:WIX = $portableWixRoot
}

# 4. Build MSI via cargo-wix
Write-Host "Building Overture Windows MSI installer via cargo-wix..."
cargo wix -p overture --nocapture
if ($LASTEXITCODE -ne 0) { throw "cargo wix failed with exit code $LASTEXITCODE" }

$msi = Get-ChildItem -Path (Join-Path $repoRoot "target\wix") -Filter "*.msi" | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if ($null -eq $msi) { throw "No .msi file found in target\wix" }

Write-Host "Built MSI: $($msi.FullName) ($([math]::Round($msi.Length / 1MB, 2)) MB)"
& (Join-Path $PSScriptRoot "verify-msi.ps1") -MsiPath $msi.FullName
