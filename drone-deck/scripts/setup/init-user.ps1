param([Parameter(Mandatory=$true)][string]$Distro)
# Opened in its own console by Setup's "Initialize Linux user". A distribution installed with
# --no-launch has only root, because Windows never ran its first-run account prompt. This makes
# the account here: the password is typed into Linux's own prompt in this window, so Drone Deck
# never sees it, and it is not written to any log or file.
$ErrorActionPreference = 'Stop'
$wsl = Join-Path $env:WINDIR 'System32\wsl.exe'
function Finish([string]$Message) {
    Write-Host ''
    Write-Host $Message
    Write-Host ''
    [void](Read-Host 'Press Enter to close this window, then return to Drone Deck')
}
try {
    Write-Host ('Linux user for ' + $Distro)
    Write-Host ''
    $users = @()
    foreach ($line in @(& $wsl -d $Distro -u root --exec getent passwd)) {
        if ($line -match '^([^:]+):[^:]*:(\d+):') {
            $uid = [int]$Matches[2]
            if ($uid -ge 1000 -and $uid -lt 60000) { $users += $Matches[1] }
        }
    }
    if ($users.Count -gt 0) {
        $name = $users[0]
        Write-Host ('This distribution already has the user "' + $name + '". Making it the default user.')
    } else {
        Write-Host 'Choose a username for the Linux account (lowercase letters, digits, - and _).'
        do {
            $name = (Read-Host 'Username').Trim()
            $ok = $name -match '^[a-z_][a-z0-9_-]{0,30}$' -and $name -ne 'root'
            if (-not $ok) { Write-Host 'Use lowercase letters, digits, - and _, starting with a letter. "root" is not allowed.' }
        } until ($ok)
        Write-Host ''
        Write-Host 'Next, Linux asks for a password for this account. It stays invisible as you type.'
        & $wsl -d $Distro -u root --exec /usr/sbin/adduser --gecos $name $name
        if ($LASTEXITCODE -ne 0) { throw ('adduser exited ' + $LASTEXITCODE + '; the account was not created.') }
    }
    & $wsl -d $Distro -u root --exec /usr/sbin/usermod -aG sudo $name
    if ($LASTEXITCODE -ne 0) { throw ('usermod exited ' + $LASTEXITCODE + '; the account could not be added to the sudo group.') }
    & $wsl --manage $Distro --set-default-user $name
    if ($LASTEXITCODE -ne 0) { throw ('wsl --manage --set-default-user exited ' + $LASTEXITCODE + '.') }
    & $wsl --terminate $Distro
    $now = (& $wsl -d $Distro --exec id -un | Select-Object -First 1)
    if ("$now".Trim() -ne $name) { throw ('The default user is "' + "$now".Trim() + '", not "' + $name + '".') }
    Finish ('Done. "' + $name + '" is now the default Linux user. Drone Deck will notice on its own.')
} catch {
    Finish ('Could not finish: ' + $_.Exception.Message)
}
