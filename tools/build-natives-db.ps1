# Builds YOUR local natives database from files YOU provide, checks it against YOUR copy of the
# game's executable, and installs it where the host and the trainer read it. Nothing is
# downloaded; nothing from the game goes into this repository (natives\local\ is git-ignored).
#
#   tools\build-natives-db.ps1 -Exe "A:\Games\...\game_scarlett_master.exe" `
#       [-DocJson my-copy-of-public-native-docs.json] [-Ids my-identifiers.tsv] `
#       [-Directives my-directives.tsv] [-Install "<trainer data folder>"]
#
# -DocJson     the public documentation's JSON form (names, argument types, parameter names);
#              its hashes belong to the build it documents and are NOT imported
# -Ids         identifiers you found for this executable: rows "name<TAB>identifier"
# -Directives  @native_table / @native_context / @script_tick rows (docs/natives-database.md)
# -Install     copy the result to this folder as natives.tsv (the folder of scripthook.dll)
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [string]$DocJson,
    [string]$Ids,
    [string]$Directives,
    [string]$Install
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$local = Join-Path $root 'natives\local'
New-Item -ItemType Directory -Force $local | Out-Null
if (-not $env:CARGO_BUILD_JOBS) { $env:CARGO_BUILD_JOBS = '6' }
Push-Location $root
try {
    cargo build --release -p shk-natives
    if ($LASTEXITCODE -ne 0) { throw 'build of shk-natives failed' }
    $tool = Join-Path $root 'target\release\shk-natives.exe'
    $db = Join-Path $local 'natives.tsv'
    if ($DocJson) {
        & $tool import-json $DocJson --out $db
    } elseif (-not (Test-Path $db)) {
        Set-Content -Encoding utf8 $db "identifier`targs`treturns`tname`tparams`tdoc"
    }
    if ($LASTEXITCODE -ne 0) { throw 'import failed' }
    if ($Ids) {
        & $tool merge $db $Ids --out $db
        if ($LASTEXITCODE -ne 0) { throw 'merge failed' }
    }
    if ($Directives) {
        # directives go first; earlier directive rows of the same name are replaced
        $dir = Get-Content $Directives | Where-Object { $_ -match '^@' }
        $names = $dir | ForEach-Object { ($_ -split "`t")[0] }
        $rest = Get-Content $db | Where-Object { -not ($_ -match '^@' -and $names -contains (($_ -split "`t")[0])) }
        $header = $rest | Where-Object { $_ -notmatch '^#' } | Select-Object -First 1
        $body = $rest | Where-Object { $_ -ne $header }
        Set-Content -Encoding utf8 $db (@($header) + $dir + $body)
    }
    & $tool check $db
    if ($LASTEXITCODE -ne 0) { throw 'the database has errors' }
    & $tool verify $db --exe $Exe
    & $tool coverage $db (Join-Path $root 'trainer\natives-needed.txt')
    & $tool gen $db --out (Join-Path $local 'generated')
    if ($Install) {
        New-Item -ItemType Directory -Force $Install | Out-Null
        Copy-Item $db (Join-Path $Install 'natives.tsv')
        Write-Host "installed $db -> $Install\natives.tsv"
    }
} finally {
    Pop-Location
}
