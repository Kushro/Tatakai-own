#Requires -Version 7.0
<#
.SYNOPSIS
  Downloads the third-party runtime pieces of Tatakai Next and verifies every file against a SHA-256 lock.

.DESCRIPTION
  Run by the USER, never by the build (cargo, dx). Everything lands under packaging/deps (git-ignored):

    deps/libmpv/libmpv-2.dll (+ mpv.lib when MSVC tools are on PATH)   libmpv.lock.json
    deps/ffmpeg/bin/ffmpeg.exe, ffprobe.exe                            ffmpeg.lock.json   (Tatakai Link sessions)
    deps/shaders/*.glsl                                                anime4k.lock.json, fsr.lock.json
    deps/fonts/*.woff2 + OFL texts                                     fonts.lock.json
    deps/env.ps1                                                       sets RUSTFLAGS for linking libmpv

  The script never deletes anything. A file whose hash does not match is refused (the partial download is
  left in the cache folder with a .partial suffix). Lock entries with "sha256": null are refused unless
  -Pin is given; -Pin downloads them, prints the hash and writes nothing into the lock: paste it yourself.

.PARAMETER Only
  Subset to fetch: libmpv, ffmpeg, anime4k, fsr, fonts. Default: all.
.PARAMETER Pin
  Allow unpinned (null hash) entries and print their hashes.
.PARAMETER Anime4kSource
  Folder that already contains the Anime4K .glsl files (e.g. Enigmata app/src/main/assets/shaders); they
  are copied after hash verification instead of being downloaded.
.PARAMETER Destination
  Output folder (default: packaging/deps).
.PARAMETER CacheDirectory
  Download cache (default: packaging/deps/.cache).
#>
[CmdletBinding()]
param(
    [ValidateSet('libmpv', 'ffmpeg', 'anime4k', 'fsr', 'fonts')]
    [string[]] $Only = @('libmpv', 'ffmpeg', 'anime4k', 'fsr', 'fonts'),
    [switch] $Pin,
    [string] $Anime4kSource,
    [string] $Destination = (Join-Path $PSScriptRoot 'deps'),
    [string] $CacheDirectory
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$Destination = [IO.Path]::GetFullPath($Destination)
if (-not $CacheDirectory) { $CacheDirectory = Join-Path $Destination '.cache' }
$CacheDirectory = [IO.Path]::GetFullPath($CacheDirectory)
$repoFonts = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../crates/tk-ui/assets/fonts'))

function Read-Lock([string] $name) {
    Get-Content (Join-Path $PSScriptRoot $name) -Raw | ConvertFrom-Json
}

function Get-Sha256([string] $path) {
    (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
}

# Downloads $url into the cache (name = hash or url hash) and returns the verified path.
function Get-VerifiedDownload([string] $url, $sha256, [string] $label) {
    if ($url -notmatch '^https://') { throw "Refusing non-HTTPS URL for ${label}: $url" }
    $pinned = -not [string]::IsNullOrWhiteSpace($sha256)
    if (-not $pinned -and -not $Pin) {
        throw "$label has no pinned SHA-256 in its lock file. Re-run with -Pin, review the printed hash and paste it into the lock."
    }
    New-Item -ItemType Directory -Force -Path $CacheDirectory | Out-Null
    $key = if ($pinned) { $sha256.ToLowerInvariant() } else { '' }
    $name = if ($pinned) { $key } else { [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($url))).ToLowerInvariant() }
    $cached = Join-Path $CacheDirectory $name
    if ($pinned -and (Test-Path -LiteralPath $cached) -and (Get-Sha256 $cached) -eq $key) { return $cached }
    $partial = "$cached.partial"
    Write-Host "  downloading $label"
    Invoke-WebRequest -Uri $url -OutFile $partial -MaximumRedirection 5
    $actual = Get-Sha256 $partial
    if ($pinned -and $actual -ne $key) {
        throw "SHA-256 mismatch for $label`n  expected $key`n  actual   $actual`n  (left at $partial)"
    }
    if (-not $pinned) { Write-Host "  PIN $label sha256 = $actual" -ForegroundColor Yellow }
    Move-Item -LiteralPath $partial -Destination $cached -Force
    return $cached
}

function New-Dir([string] $path) { New-Item -ItemType Directory -Force -Path $path | Out-Null; $path }

# Extract an archive into a fresh, uniquely named folder inside the cache.
function Expand-Verified([string] $archive, [string] $type) {
    $dir = Join-Path $CacheDirectory ("x-" + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $dir | Out-Null
    if ($type -eq 'zip') {
        Expand-Archive -LiteralPath $archive -DestinationPath $dir
    } else {
        # Windows' bsdtar reads 7z; fall back to 7z.exe when it does not.
        & tar.exe -xf $archive -C $dir
        if ($LASTEXITCODE -ne 0) {
            $sevenZip = Get-Command 7z -ErrorAction SilentlyContinue
            if (-not $sevenZip) { throw "Could not extract $archive (tar failed and 7z is not on PATH)." }
            & $sevenZip.Source x $archive "-o$dir" -y | Out-Null
            if ($LASTEXITCODE -ne 0) { throw "7z failed on $archive" }
        }
    }
    return $dir
}

function Install-LibMpv {
    Write-Host '== libmpv'
    $lock = (Read-Lock 'libmpv.lock.json').packages[0]
    $archive = Get-VerifiedDownload $lock.url $lock.sha256 'libmpv'
    $x = Expand-Verified $archive $lock.archiveType
    $out = New-Dir (Join-Path $Destination 'libmpv')
    foreach ($f in $lock.requiredFiles) {
        $src = Join-Path $x $f
        if (-not (Test-Path -LiteralPath $src)) { throw "libmpv archive layout changed: $f not found." }
        Copy-Item -LiteralPath $src -Destination $out -Force
    }
    # An MSVC import library is needed to link; make one from the DLL's export table when possible.
    $dumpbin = Get-Command dumpbin -ErrorAction SilentlyContinue
    $lib = Get-Command lib -ErrorAction SilentlyContinue
    if ($dumpbin -and $lib) {
        $names = & $dumpbin.Source /exports (Join-Path $out 'libmpv-2.dll') |
            ForEach-Object { if ($_ -match '^\s+\d+\s+[0-9A-F]+\s+[0-9A-F]{8}\s+(\S+)') { $Matches[1] } }
        if (@($names).Count -lt 50) { throw "Parsed only $(@($names).Count) exports from libmpv-2.dll." }
        $def = Join-Path $out 'mpv.def'
        (@('EXPORTS') + ($names | ForEach-Object { "    $_" })) | Set-Content -Path $def -Encoding ascii
        & $lib.Source "/def:$def" '/name:libmpv-2.dll' "/out:$(Join-Path $out 'mpv.lib')" '/machine:x64' | Out-Null
        if (-not (Test-Path (Join-Path $out 'mpv.lib'))) { throw 'lib.exe did not produce mpv.lib' }
        Write-Host '  mpv.lib generated'
    } else {
        Write-Warning 'dumpbin/lib not on PATH: open a "x64 Native Tools" prompt and re-run with -Only libmpv to create mpv.lib (needed by cargo build).'
    }
    $env1 = Join-Path $Destination 'env.ps1'
    $libDir = $out.Replace('\', '/')
    Set-Content -Path $env1 -Encoding utf8 -Value @(
        '# Dot-source before `cargo build`: . packaging/deps/env.ps1',
        "`$env:RUSTFLAGS = `"-L native=$libDir`""
    )
    Write-Host "  wrote $env1"
}

function Install-Ffmpeg {
    Write-Host '== ffmpeg (Tatakai Link sessions)'
    $pkg = (Read-Lock 'ffmpeg.lock.json').packages[0]
    $archive = Get-VerifiedDownload $pkg.url $pkg.sha256 'ffmpeg'
    $x = Expand-Verified $archive 'zip'
    $root = Join-Path $x $pkg.archiveRoot
    foreach ($e in $pkg.executables) {
        if (-not (Test-Path -LiteralPath (Join-Path $root $e))) { throw "ffmpeg archive layout changed: $e not found." }
    }
    $out = New-Dir (Join-Path $Destination 'ffmpeg')
    Copy-Item -Path (Join-Path $root '*') -Destination $out -Recurse -Force
    Write-Host '  note: ffmpeg is LGPL-3.0-or-later; keep LICENSE.txt next to the binaries.'
}

function Install-Shaders([string] $lockName, [string] $label, [string] $localSource) {
    Write-Host "== $label"
    $lock = Read-Lock $lockName
    $out = New-Dir (Join-Path $Destination 'shaders')
    foreach ($f in $lock.files) {
        $target = Join-Path $out $f.path
        if ($localSource -and (Test-Path -LiteralPath (Join-Path $localSource $f.path))) {
            $src = Join-Path $localSource $f.path
            $actual = Get-Sha256 $src
            if ($f.sha256 -and $actual -ne $f.sha256.ToLowerInvariant()) { throw "SHA-256 mismatch for local $($f.path): $actual" }
            Copy-Item -LiteralPath $src -Destination $target -Force
            Write-Host "  copied $($f.path) (local, verified)"
            continue
        }
        $src = Get-VerifiedDownload $f.url $f.sha256 $f.path
        Copy-Item -LiteralPath $src -Destination $target -Force
    }
}

function Install-Fonts {
    Write-Host '== fonts'
    $lock = Read-Lock 'fonts.lock.json'
    $out = New-Dir (Join-Path $Destination 'fonts')
    foreach ($f in $lock.files) {
        $src = Get-VerifiedDownload $f.url $f.sha256 $f.path
        Copy-Item -LiteralPath $src -Destination (Join-Path $out $f.path) -Force
    }
    # JetBrains Mono ships in the repo; copy it next to the downloaded ones so one folder is complete.
    foreach ($n in 'JetBrainsMono.woff2', 'OFL-JetBrainsMono.txt') {
        Copy-Item -LiteralPath (Join-Path $repoFonts $n) -Destination $out -Force
    }
}

New-Dir $Destination | Out-Null
foreach ($what in $Only) {
    switch ($what) {
        'libmpv'  { Install-LibMpv }
        'ffmpeg'  { Install-Ffmpeg }
        'anime4k' { Install-Shaders 'anime4k.lock.json' 'Anime4K shaders' $Anime4kSource }
        'fsr'     { Install-Shaders 'fsr.lock.json' 'FSR shader' $null }
        'fonts'   { Install-Fonts }
    }
}
Write-Host "Done. Output: $Destination"
