param(
    [Parameter(Mandatory = $true)]
    [scriptblock] $Body
)

$ErrorActionPreference = 'Stop'
$postgresBin = if ($env:BC_POSTGRES_BIN) { $env:BC_POSTGRES_BIN } else { 'F:\postgres\bin' }
$initdb = Join-Path $postgresBin 'initdb.exe'
$pgCtl = Join-Path $postgresBin 'pg_ctl.exe'
if (-not (Test-Path -LiteralPath $initdb) -or -not (Test-Path -LiteralPath $pgCtl)) {
    throw 'Set BC_POSTGRES_BIN to a PostgreSQL bin directory containing initdb.exe and pg_ctl.exe.'
}

$testRoot = Join-Path ([IO.Path]::GetTempPath()) ("bc-m0-04-" + [guid]::NewGuid().ToString('N'))
$data = Join-Path $testRoot 'data'
New-Item -ItemType Directory -Path $testRoot | Out-Null
$listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
$listener.Start()
$port = $listener.LocalEndpoint.Port
$listener.Stop()

$started = $false
$previousUrl = $env:BC_TEST_ADMIN_DATABASE_URL
try {
    & $initdb -D $data -U postgres --auth=trust --no-instructions | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'initdb failed' }
    & $pgCtl -D $data -l (Join-Path $testRoot 'postgres.log') -o "-h 127.0.0.1 -p $port" -w start
    if ($LASTEXITCODE -ne 0) { throw 'pg_ctl start failed' }
    $started = $true
    $env:BC_TEST_ADMIN_DATABASE_URL = "postgres://postgres@127.0.0.1:$port/postgres"
    & $Body
    if ($LASTEXITCODE -ne 0) { throw "test command failed with exit code $LASTEXITCODE" }
}
finally {
    $env:BC_TEST_ADMIN_DATABASE_URL = $previousUrl
    if ($started) { & $pgCtl -D $data -m immediate -w stop }
    $resolvedRoot = [IO.Path]::GetFullPath($testRoot)
    $resolvedTemp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    if (-not $resolvedRoot.StartsWith($resolvedTemp, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Refusing to remove a test directory outside the temporary directory.'
    }
    Remove-Item -LiteralPath $resolvedRoot -Recurse -Force
}
