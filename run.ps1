# Builds Space Race, starts the server in the background and runs the client against it, then stops
# the server when the client closes. Anything this script does not recognise goes to the client,
# so every client option works here (`--help` lists them).
#
#   ./run.ps1                                              build, then play
#   ./run.ps1 -SkipBuild                                   play what is already built
#   ./run.ps1 --nickname Primo --lobby Test --min-players 1   straight into a race, alone
#   ./run.ps1 -Clients 2 --nickname Primo --lobby Test     two players on one machine
#   ./run.ps1 --autopilot-drift --track esplanade          let it drive the wide circuit
#
# The server log goes to target/server.log, and anything it writes to standard error to
# target/server.err.log. A server already listening is left alone and reused.

# No param() block on purpose: PowerShell would try to bind `--nickname` as one of its own
# parameters and fail. Everything arrives in $args, and the script's own switches are picked out.
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

$skipBuild = $false
$clients = 1
$clientArgs = @()
for ($i = 0; $i -lt $args.Count; $i++) {
    switch -Exact ($args[$i]) {
        '-SkipBuild' { $skipBuild = $true }
        '-Clients'   { $i++; $clients = [int]$args[$i] }
        default      { $clientArgs += $args[$i] }
    }
}

$port = 8080
$log = 'target/server.log'
$errLog = 'target/server.err.log'
$serverExe = 'target/debug/space-race-server.exe'
$clientExe = 'target/debug/space-race-client.exe'

function Test-Listening {
    return [bool](Get-NetTCPConnection -State Listen -LocalPort $port -ErrorAction SilentlyContinue)
}

# Debug: the dev profile optimizes every dependency, so Bevy runs at full speed, and only our own
# code is lightly optimized, which keeps it quick to rebuild. The whole workspace at once, since
# naming a single crate changes the unified features and recompiles Bevy (see CLAUDE.md).
if (-not $skipBuild) {
    Write-Host '==> building the workspace' -ForegroundColor Cyan
    cargo build --workspace
    if ($LASTEXITCODE -ne 0) { throw 'the build failed' }
}
foreach ($exe in $serverExe, $clientExe) {
    if (-not (Test-Path $exe)) { throw "$exe is missing: run without -SkipBuild" }
}

$server = $null
$extraClients = @()
try {
    if (Test-Listening) {
        Write-Host "==> a server is already listening on $port, using it" -ForegroundColor Yellow
    }
    else {
        Write-Host "==> starting the server, logging to $log" -ForegroundColor Cyan
        $server = Start-Process -FilePath $serverExe -NoNewWindow -PassThru `
            -RedirectStandardOutput $log -RedirectStandardError $errLog
        # Wait for it to listen: a client started too early just shows a failed connection.
        $deadline = (Get-Date).AddSeconds(20)
        while (-not (Test-Listening)) {
            if ($server.HasExited) { throw "the server stopped at once, see $log" }
            if ((Get-Date) -gt $deadline) { throw "the server never listened on $port, see $log" }
            Start-Sleep -Milliseconds 150
        }
    }

    # Every client past the first gets an identity of its own, or the server refuses the second
    # connection: a player is their key (see docs/protocol.md).
    for ($n = 2; $n -le $clients; $n++) {
        Write-Host "==> starting client $n" -ForegroundColor Cyan
        $extraClients += Start-Process -FilePath $clientExe -PassThru `
            -ArgumentList (@('--identity', "target/identity-$n") + $clientArgs)
    }

    Write-Host '==> running the client' -ForegroundColor Cyan
    & $clientExe @clientArgs
}
finally {
    foreach ($c in $extraClients) {
        if ($c -and -not $c.HasExited) { Stop-Process -Id $c.Id -Force -ErrorAction SilentlyContinue }
    }
    if ($server -and -not $server.HasExited) {
        Write-Host '==> stopping the server' -ForegroundColor Cyan
        Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue
    }
    if ((Test-Path $errLog) -and (Get-Item $errLog).Length -gt 0) {
        Write-Warning "the server wrote to standard error, see $errLog"
    }
}
