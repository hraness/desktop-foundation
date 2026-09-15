# CI desktop infrastructure, analogous to Linux's explicit Xvfb/D-Bus fixture.
# This is not evidence of a visible menu or a clean-machine installation.
# Never invoke against a user's desktop or change sessions, login, or OS policy.
param([switch]$ProbeOnly)

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
    [DllImport("user32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)]
    private static extern IntPtr CreateWindowExW(uint extendedStyle, string className, string name,
        uint style, int x, int y, int width, int height, IntPtr parent, IntPtr menu, IntPtr instance, IntPtr parameter);
    [DllImport("user32.dll", ExactSpelling = true, SetLastError = true)]
    private static extern int DestroyWindow(IntPtr window);
    [DllImport("user32.dll", ExactSpelling = true, SetLastError = true)]
    private static extern IntPtr LoadIconW(IntPtr instance, IntPtr resourceId);
    [DllImport("shell32.dll", ExactSpelling = true, SetLastError = true)]
    private static extern int Shell_NotifyIconW(uint message, ref NotifyIconData data);

    // Windows SDK shellapi.h uses pack(1) only for !_WIN64. Natural sequential
    // layout matches both x64 and ARM64; strings are fixed UTF-16 arrays.
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct NotifyIconData {
        public uint Size;
        public IntPtr Window;
        public uint Id, Flags, CallbackMessage;
        public IntPtr Icon;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)] public string Tip;
        public uint State, StateMask;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 256)] public string Info;
        public uint TimeoutOrVersion;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 64)] public string InfoTitle;
        public uint InfoFlags;
        public Guid Guid;
        public IntPtr BalloonIcon;
    }

    private static NotifyIconData Data(IntPtr window, uint id) {
        return new NotifyIconData {
            Size = (uint)Marshal.SizeOf<NotifyIconData>(), Window = window, Id = id,
            Tip = "", Info = "", InfoTitle = ""
        };
    }

    private static void Emit(string stage, int variant, int result, int error) {
        // Shell_NotifyIcon documents BOOL only, not GetLastError semantics.
        // Retain the numeric error as diagnostic context, never a success test.
        Console.WriteLine("{\"fixture\":\"windows-notifyicon-probe\",\"stage\":\"" + stage +
            "\",\"withIcon\":" + variant + ",\"result\":" + result + ",\"lastError\":" + error +
            ",\"lastErrorDiagnosticOnly\":true,\"size\":" + Marshal.SizeOf<NotifyIconData>() +
            ",\"windowOffset\":" + Marshal.OffsetOf<NotifyIconData>("Window").ToInt64() +
            ",\"iconOffset\":" + Marshal.OffsetOf<NotifyIconData>("Icon").ToInt64() +
            ",\"tipOffset\":" + Marshal.OffsetOf<NotifyIconData>("Tip").ToInt64() + "}");
    }

    public static void Probe() {
        // An owned hidden top-level window; never show, activate, or interact.
        IntPtr window = CreateWindowExW(0x80, "STATIC", "Companion CI probe", 0,
            0, 0, 1, 1, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero);
        int error = Marshal.GetLastWin32Error();
        Emit("create-window", -1, window == IntPtr.Zero ? 0 : 1, error);
        if (window == IntPtr.Zero) return;
        try {
            // IDI_APPLICATION is a shared borrowed system icon: do not destroy.
            IntPtr icon = LoadIconW(IntPtr.Zero, new IntPtr(32512));
            error = Marshal.GetLastWin32Error();
            Emit("load-system-icon", -1, icon == IntPtr.Zero ? 0 : 1, error);
            for (int variant = 0; variant < 2; variant++) {
                if (variant == 1 && icon == IntPtr.Zero) continue;
                NotifyIconData data = Data(window, (uint)(1000 + variant));
                try {
                    data.Flags = variant == 0 ? 1u : 3u; // NIF_MESSAGE, optionally NIF_ICON
                    data.CallbackMessage = 0x8001;
                    data.Icon = variant == 0 ? IntPtr.Zero : icon;
                    int result = Shell_NotifyIconW(0, ref data); // NIM_ADD
                    error = Marshal.GetLastWin32Error();
                    Emit("add", variant, result, error);
                    data.Flags = 4; // NIF_TIP: clear tooltip, matching the initial product model.
                    result = Shell_NotifyIconW(1, ref data); // NIM_MODIFY
                    error = Marshal.GetLastWin32Error();
                    Emit("modify-tooltip", variant, result, error);
                } finally {
                    // The HWND and IDs were created by this probe. Never touch
                    // any existing taskbar icon, even after an add failure.
                    data.Flags = 0;
                    int result = Shell_NotifyIconW(2, ref data); // NIM_DELETE
                    error = Marshal.GetLastWin32Error();
                    Emit("delete", variant, result, error);
                }
            }
        } finally {
            int result = DestroyWindow(window);
            error = Marshal.GetLastWin32Error();
            Emit("destroy-window", -1, result, error);
        }
    }

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

if ($ProbeOnly) {
    if ([CompanionDesktopFixture]::TrayProcessId($fixtureSession) -eq 0) {
        throw 'windows-notifyicon-probe-requires-same-session-tray'
    }
    [CompanionDesktopFixture]::Probe()
    return
}

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

# A separate same-session process bounds even a stalled Shell_NotifyIcon call.
# Probe results diagnose the real native gate; they never replace or skip it.
$probeStart = [System.Diagnostics.ProcessStartInfo]::new([Environment]::ProcessPath)
$probeStart.UseShellExecute = $false
$probeStart.RedirectStandardOutput = $true
$probeStart.RedirectStandardError = $true
foreach ($argument in @('-NoProfile', '-NonInteractive', '-File', $PSCommandPath, '-ProbeOnly')) {
    $probeStart.ArgumentList.Add($argument)
}
$probeProcess = [System.Diagnostics.Process]::Start($probeStart)
if ($null -eq $probeProcess) { throw 'windows-notifyicon-probe-start-failed' }
try {
    $probeOutput = $probeProcess.StandardOutput.ReadToEndAsync()
    $probeErrors = $probeProcess.StandardError.ReadToEndAsync()
    if (-not $probeProcess.WaitForExit(10000)) {
        # Only terminate this exact owned diagnostic process. Normal probe
        # cleanup uses finally; on timeout the OS destroys its owned window.
        $probeProcess.Kill()
        if (-not $probeProcess.WaitForExit(1000)) { throw 'windows-notifyicon-probe-cleanup-timeout' }
        Write-Output '{"fixture":"windows-notifyicon-probe","stage":"timeout","diagnosticOnly":true}'
    }
    if ($probeOutput.Wait(1000)) { Write-Output $probeOutput.Result.TrimEnd() }
    if ($probeErrors.Wait(1000) -and -not [string]::IsNullOrWhiteSpace($probeErrors.Result)) {
        $probeErrorText = $probeErrors.Result
        [ordered]@{
            fixture = 'windows-notifyicon-probe'
            stage = 'stderr'
            diagnostic = $probeErrorText.Substring(0, [Math]::Min(2000, $probeErrorText.Length))
            diagnosticOnly = $true
        } | ConvertTo-Json -Compress | Write-Output
    }
    [ordered]@{
        fixture = 'windows-notifyicon-probe'
        stage = 'complete'
        exitCode = $probeProcess.ExitCode
        diagnosticOnly = $true
    } | ConvertTo-Json -Compress | Write-Output
} finally {
    if (-not $probeProcess.HasExited) { $probeProcess.Kill() }
    $probeProcess.Dispose()
}
# Leave the job-owned shell available to both native smoke steps. The disposable
# hosted VM and GitHub runner process cleanup own its teardown; never kill an
# existing Explorer process or change a real user's shell.
