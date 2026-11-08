param(
    [switch]$SkipBuild,
    [string]$InnoCompiler = ""
)
$ErrorActionPreference = "Stop"
$desktopRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$manifest = Get-Content -Raw -LiteralPath (Join-Path $desktopRoot "Cargo.toml")
$versionMatch = [regex]::Match($manifest, '(?m)^version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"\s*$')
if (-not $versionMatch.Success) { throw "Cannot determine the desktop version." }
$version = $versionMatch.Groups[1].Value
if (-not $SkipBuild) {
    & cargo +stable-x86_64-pc-windows-msvc build --locked --release --manifest-path (Join-Path $desktopRoot "Cargo.toml") --bin sonar-desktop
    if ($LASTEXITCODE -ne 0) { throw "Native release build failed." }
}
$exe = Join-Path $desktopRoot "target\release\sonar-desktop.exe"
if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw "Native release executable is missing." }
$dist = Join-Path $desktopRoot "dist"
New-Item -ItemType Directory -Force -Path $dist | Out-Null
$zip = Join-Path $dist "Sonar-Desktop-Preview-$version-windows-x64.zip"
Compress-Archive -LiteralPath $exe, (Join-Path $desktopRoot "WINDOWS-PREVIEW.txt") -DestinationPath $zip -Force
$checksum = (Get-FileHash -Algorithm SHA256 -LiteralPath $zip).Hash.ToLowerInvariant()
[IO.File]::WriteAllText("$zip.sha256", "$checksum  $([IO.Path]::GetFileName($zip))`n", [Text.UTF8Encoding]::new($false))
Write-Host "Portable preview: $zip"
if ($InnoCompiler) {
    if (-not (Test-Path -LiteralPath $InnoCompiler -PathType Leaf)) { throw "Inno Setup compiler not found." }
    & $InnoCompiler "/DSonarVersion=$version" (Join-Path $PSScriptRoot "sonar-preview.iss")
    if ($LASTEXITCODE -ne 0) { throw "Installer build failed." }
    $installer = Join-Path $dist "Sonar-Desktop-Preview-$version-windows-x64-setup.exe"
    if (-not (Test-Path -LiteralPath $installer -PathType Leaf)) { throw "Installer was not created." }
    $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $installer).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText("$installer.sha256", "$hash  $([IO.Path]::GetFileName($installer))`n", [Text.UTF8Encoding]::new($false))
    Write-Host "Installer preview: $installer"
}
