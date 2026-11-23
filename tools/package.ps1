# Builds everything and lays out a ready-to-use folder: dist\gta-vi-scripthook\
#   shk-loader.exe, scripthook.dll, scripthook.toml, mod.toml (optional, for runtimes with mod folders)
#   plugins\menu\     shk_menu.dll, mod.toml, menu.toml
#   plugins\trainer\  shk_trainer.dll, mod.toml, trainer.toml, cheats.toml, features\*.toml
#   plugins\hello-c\, hello-cpp\, hello-rust\   the examples (-NoExamples to leave them out)
# Then add your natives.tsv (docs/natives-database.md) and start the game with
#   shk-loader.exe --launch <the program that runs the game> [-- its arguments]
# Nothing is downloaded. Single-player / offline only.
param([switch]$NoExamples, [switch]$NoCExamples)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$dist = Join-Path $root 'dist\gta-vi-scripthook'
if (-not $env:CARGO_BUILD_JOBS) { $env:CARGO_BUILD_JOBS = '6' }
Push-Location $root
try {
    cargo build --release -p shk-core -p shk-loader -p shk-menu -p shk-trainer -p hello-rust -p shk-natives
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
    if (-not $NoExamples -and -not $NoCExamples) {
        & cmd /c "`"$root\tools\build-examples.cmd`""
        if ($LASTEXITCODE -ne 0) { throw 'C / C++ examples failed (MSVC needed; -NoCExamples skips them)' }
    }
    if (Test-Path $dist) { Remove-Item -Recurse -Force $dist }
    $t = Join-Path $root 'target\release'
    New-Item -ItemType Directory -Force $dist | Out-Null
    Copy-Item "$t\scripthook.dll", "$t\shk-loader.exe", "$t\shk-natives.exe" $dist
    Copy-Item (Join-Path $root 'core\scripthook.toml'), (Join-Path $root 'core\mod.toml') $dist
    $layout = @{
        'menu'    = @(@("$t\shk_menu.dll"), @('menu\mod.toml', 'menu\menu.toml'))
        'trainer' = @(@("$t\shk_trainer.dll"), @('trainer\mod.toml', 'trainer\trainer.toml', 'trainer\cheats.toml'))
    }
    if (-not $NoExamples) {
        $layout['hello-rust'] = @(@("$t\hello_rust.dll"), @('examples\hello-rust\mod.toml'))
        if (-not $NoCExamples) {
            $layout['hello-c'] = @(@("$root\build\examples\hello_c.dll"), @('examples\hello-c\mod.toml'))
            $layout['hello-cpp'] = @(@("$root\build\examples\hello_cpp.dll"), @('examples\hello-cpp\mod.toml'))
        }
    }
    foreach ($k in $layout.Keys) {
        $d = Join-Path $dist "plugins\$k"
        New-Item -ItemType Directory -Force $d | Out-Null
        foreach ($f in $layout[$k][0]) { Copy-Item $f $d }
        foreach ($f in $layout[$k][1]) { Copy-Item (Join-Path $root $f) $d }
    }
    $fd = Join-Path $dist 'plugins\trainer\features'
    New-Item -ItemType Directory -Force $fd | Out-Null
    Copy-Item (Join-Path $root 'trainer\features\*.toml') $fd
    Write-Host "package: $dist"
    Get-ChildItem -Recurse $dist | Where-Object { -not $_.PSIsContainer } | ForEach-Object { Write-Host ('  ' + $_.FullName.Substring($dist.Length + 1)) }
} finally {
    Pop-Location
}
