# Develops the app: builds it, starts it with --dev, and on every change to the Rust code, the
# templates, the queries, the assets or the extension builds and restarts it, after which the page
# in the browser reloads itself. Ctrl+C stops it.
#
#   .\dev.ps1                                  (from app\, or app\dev.ps1 from anywhere)
#   .\dev.ps1 --database $env:TEMP\dev.sqlite  (more recruiter options go on to the app)
#
# It needs watchexec once: cargo install watchexec-cli --locked

param(
    # Where the app listens and the browser is pointed; passed on as --port.
    [int] $Port = 8765,
    # Any other recruiter options, such as --database or --laya.
    [Parameter(ValueFromRemainingArguments)] [string[]] $RecruiterArgs = @()
)

$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

if (-not (Get-Command watchexec -ErrorAction SilentlyContinue)) {
    Write-Error 'dev.ps1 needs watchexec. Install it once with: cargo install watchexec-cli --locked'
}

$address = "http://127.0.0.1:$Port/match"

# Opens the page once the app first answers; after that the page reloads itself on each restart.
# Seconds to wait for the first build before giving up on opening the page.
$firstBuildSeconds = 900
Start-Job -ArgumentList $address, $firstBuildSeconds {
    param($address, $firstBuildSeconds)
    $deadline = (Get-Date).AddSeconds($firstBuildSeconds)
    while ((Get-Date) -lt $deadline) {
        try {
            Invoke-WebRequest $address -UseBasicParsing -TimeoutSec 2 | Out-Null
            Start-Process $address
            return
        } catch {
            Start-Sleep -Seconds 1
        }
    }
} | Out-Null

Write-Host "Building and starting Recruiter; $address opens when it is up."
watchexec --restart --shell=none `
    --watch src --watch templates --watch queries --watch migrations --watch .sqlx `
    --watch Cargo.toml --watch ../assets --watch ../extension --watch ../laya-server/src `
    -- cargo run -- --dev --no-browser --port $Port @RecruiterArgs
