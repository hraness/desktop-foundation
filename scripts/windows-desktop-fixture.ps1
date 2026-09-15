# CI desktop infrastructure, analogous to Linux's explicit Xvfb/D-Bus fixture.
# This is not evidence of a visible menu or a clean-machine installation.
# Never invoke against a user's desktop or change sessions, login, or OS policy.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not $IsWindows -or $env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') {
    throw 'windows-desktop-fixture-requires-github-hosted-windows'
}

$fixtureWatch = [System.Diagnostics.Stopwatch]::StartNew()
$fixtureSession = [System.Diagnostics.Process]::GetCurrentProcess().SessionId
$fixtureInteractive = [Environment]::UserInteractive
$explorerStarted = $false

Add-Type -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Runtime.InteropServices;

public static class CompanionDesktopFixture {
    [DllImport("user32.dll", CharSet = CharSet.Unicode, ExactSpelling = true)]
    private static extern IntPtr FindWindowW(string className, string windowName);
    [DllImport("user32.dll", ExactSpelling = true)]
    private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);

    public static int TrayProcessId(int sessionId) {
        IntPtr window = FindWindowW("Shell_TrayWnd", null);
        if (window == IntPtr.Zero) return 0;
        uint processId;
        if (GetWindowThreadProcessId(window, out processId) == 0 || processId == 0) return 0;
        try {
            using (Process process = Process.GetProcessById((int)processId)) {
                return process.SessionId == sessionId ? process.Id : 0;
            }
        } catch (ArgumentException) {
            // The shell may exit between the window and process queries.
            return 0;
        }
    }
}
'@

function Write-FixtureEvidence([string]$stage, [int]$trayProcessId) {
    [ordered]@{
        fixture = 'windows-desktop'
        stage = $stage
        sessionId = $fixtureSession
        userInteractive = $fixtureInteractive
        trayProcessId = $trayProcessId
        explorerStarted = $explorerStarted
        elapsedMs = $fixtureWatch.ElapsedMilliseconds
        visualQualification = $false
    } | ConvertTo-Json -Compress | Write-Output
}

$trayProcessId = [CompanionDesktopFixture]::TrayProcessId($fixtureSession)
Write-FixtureEvidence 'initial' $trayProcessId
if ($trayProcessId -eq 0 -and $fixtureWatch.ElapsedMilliseconds -lt 15000) {
    # UseShellExecute=false uses this job's token and session. No elevation,
    # alternate-user credentials, session switching, autologin, or policy edits.
    # Windows connects child processes to the inherited window station:
    # https://learn.microsoft.com/en-us/windows/win32/winstation/process-connection-to-a-window-station
    $explorerPath = Join-Path ([Environment]::GetFolderPath('Windows')) 'explorer.exe'
    if (-not (Test-Path -LiteralPath $explorerPath -PathType Leaf)) {
        throw 'windows-desktop-fixture-explorer-unavailable'
    }
    $startInfo = [System.Diagnostics.ProcessStartInfo]::new($explorerPath)
    $startInfo.UseShellExecute = $false
    $explorerProcess = [System.Diagnostics.Process]::Start($startInfo)
    if ($null -eq $explorerProcess) { throw 'windows-desktop-fixture-explorer-start-failed' }
    $explorerStarted = $true
    $explorerProcess.Dispose()
    Write-FixtureEvidence 'explorer-started' 0
    do {
        $trayProcessId = [CompanionDesktopFixture]::TrayProcessId($fixtureSession)
        if ($trayProcessId -ne 0) { break }
        Start-Sleep -Milliseconds 100
    } while ($fixtureWatch.ElapsedMilliseconds -lt 15000)
}

if ($trayProcessId -eq 0 -or $fixtureWatch.ElapsedMilliseconds -ge 15000) {
    Write-FixtureEvidence 'unavailable' $trayProcessId
    throw "windows-desktop-fixture-unavailable: no same-session Shell_TrayWnd within 15s (session=$fixtureSession, interactive=$fixtureInteractive)"
}
Write-FixtureEvidence 'ready' $trayProcessId
# Leave the job-owned shell available to both native smoke steps. The disposable
# hosted VM and GitHub runner process cleanup own its teardown; never kill an
# existing Explorer process or change a real user's shell.
