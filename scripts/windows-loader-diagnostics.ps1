# CI-only loader diagnostics. Reads synthetic build artifacts and system DLLs;
# never changes the OS loader configuration, installed DLLs, or test executable.
param([string] $OutputDirectory = 'target/windows-loader-diagnostics')
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (-not $IsWindows) { throw 'This diagnostic requires Windows.' }
New-Item -ItemType Directory -Force $OutputDirectory | Out-Null
$outputRoot = (Resolve-Path $OutputDirectory).Path

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class CompanionLoaderProbe {
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern IntPtr LoadLibraryExW(string name, IntPtr file, uint flags);
    [DllImport("kernel32.dll", CharSet = CharSet.Ansi, ExactSpelling = true, SetLastError = true)]
    public static extern IntPtr GetProcAddress(IntPtr module, string name);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    public static extern uint GetModuleFileNameW(IntPtr module, StringBuilder path, int size);
    [DllImport("kernel32.dll")]
    public static extern bool FreeLibrary(IntPtr module);
    [DllImport("kernel32.dll", ExactSpelling = true)]
    private static extern IntPtr FindResourceW(IntPtr module, IntPtr name, IntPtr type);
    [DllImport("kernel32.dll")]
    private static extern uint SizeofResource(IntPtr module, IntPtr resource);
    [DllImport("kernel32.dll")]
    private static extern IntPtr LoadResource(IntPtr module, IntPtr resource);
    [DllImport("kernel32.dll")]
    private static extern IntPtr LockResource(IntPtr resource);
    public static string Manifest(string executable) {
        // DATAFILE | IMAGE_RESOURCE: inspect resources without executing code.
        var module = LoadLibraryExW(executable, IntPtr.Zero, 0x22);
        if (module == IntPtr.Zero) throw new System.ComponentModel.Win32Exception();
        try {
            var resource = FindResourceW(module, new IntPtr(1), new IntPtr(24));
            if (resource == IntPtr.Zero) return null;
            var length = SizeofResource(module, resource);
            if (length > 65536) throw new Exception("Manifest exceeds diagnostic limit");
            var data = LockResource(LoadResource(module, resource));
            if (data == IntPtr.Zero) throw new Exception("Cannot read executable manifest");
            var bytes = new byte[length];
            Marshal.Copy(data, bytes, 0, bytes.Length);
            return Encoding.UTF8.GetString(bytes).TrimEnd('\0');
        } finally { FreeLibrary(module); }
    }
}
'@

$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$installation = & $vswhere -latest -products '*' -property installationPath
if ($LASTEXITCODE -ne 0 -or -not $installation) { throw 'Visual Studio installation not found.' }
$machineArchitecture = [System.Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString().ToLowerInvariant()
$hostDirectory = if ($machineArchitecture -eq 'arm64') { 'Hostarm64' } else { 'Hostx64' }
$dumpbinCandidates = @(Get-ChildItem "$installation/VC/Tools/MSVC/*/bin/$hostDirectory/$machineArchitecture/dumpbin.exe")
if ($dumpbinCandidates.Count -eq 0) {
    # The ARM runner can also inspect ARM PE files with the installed x64 tool.
    $dumpbinCandidates = @(Get-ChildItem "$installation/VC/Tools/MSVC/*/bin/Hostx64/x64/dumpbin.exe")
}
if ($dumpbinCandidates.Count -eq 0) { throw 'dumpbin.exe not found.' }
$dumpbin = ($dumpbinCandidates | Sort-Object FullName -Descending | Select-Object -First 1).FullName

function Read-Dump([string] $Option, [string] $Path) {
    $lines = @(& $dumpbin /nologo $Option $Path)
    if ($LASTEXITCODE -ne 0) { throw "dumpbin failed for $Path" }
    if ($lines.Count -gt 20000) { throw 'Import/export output exceeds diagnostic limit.' }
    return $lines
}

$executables = @(Get-ChildItem target/debug/deps -File | Where-Object {
    $_.Name -match '^(desktop_foundation|hraness_companion)-[a-f0-9]+\.exe$'
} | Sort-Object LastWriteTimeUtc -Descending | Select-Object -First 4)
if ($executables.Count -eq 0) { throw 'No native test executables found.' }
$reports = @()
foreach ($executable in $executables) {
    if ($executable.Length -gt 268435456) { throw 'Test executable exceeds diagnostic limit.' }
    $stem = $executable.BaseName
    $imports = @(Read-Dump /imports $executable.FullName)
    $imports | Set-Content -Encoding utf8 (Join-Path $outputRoot "$stem.imports.txt")
    $manifest = [CompanionLoaderProbe]::Manifest($executable.FullName)
    if ($manifest) { $manifest | Set-Content -Encoding utf8 (Join-Path $outputRoot "$stem.manifest.xml") }
    $byLibrary = @{}
    $currentLibrary = $null
    foreach ($line in $imports) {
        if ($line -match '^\s*Summary\s*$') { break }
        if ($line -match '^\s+([A-Za-z0-9._-]+\.dll)\s*$') {
            $currentLibrary = $Matches[1]
            if (-not $byLibrary.ContainsKey($currentLibrary)) { $byLibrary[$currentLibrary] = @() }
        } elseif ($currentLibrary -and $line -match '^\s+[0-9A-Fa-f]+\s+([A-Za-z_?@$][^\s]*)\s*$') {
            $byLibrary[$currentLibrary] += $Matches[1]
        }
    }
    $libraries = @()
    foreach ($name in ($byLibrary.Keys | Sort-Object)) {
        # Let Windows resolve API-set DLLs and forwarded exports. These results
        # describe this probe process; the target manifest is recorded separately.
        $module = [CompanionLoaderProbe]::LoadLibraryExW($name, [IntPtr]::Zero, 0x800)
        $loadError = if ($module -eq [IntPtr]::Zero) { [Runtime.InteropServices.Marshal]::GetLastWin32Error() } else { 0 }
        $resolvedPath = $null
        $missing = @()
        if ($module -ne [IntPtr]::Zero) {
            try {
                $buffer = [Text.StringBuilder]::new(32768)
                [void][CompanionLoaderProbe]::GetModuleFileNameW($module, $buffer, $buffer.Capacity)
                $resolvedPath = $buffer.ToString()
                foreach ($symbol in $byLibrary[$name]) {
                    if ([CompanionLoaderProbe]::GetProcAddress($module, $symbol) -eq [IntPtr]::Zero) { $missing += $symbol }
                }
            } finally { [void][CompanionLoaderProbe]::FreeLibrary($module) }
        }
        $legacyMissing = @()
        if ($name -ieq 'comctl32.dll') {
            # PowerShell may itself select Common Controls v6. Compare the actual
            # legacy System32 file without loading it, so it cannot hide the bug.
            $legacyPath = Join-Path $env:SystemRoot 'System32/comctl32.dll'
            $exports = @(Read-Dump /exports $legacyPath)
            $exports | Set-Content -Encoding utf8 (Join-Path $outputRoot "$stem.comctl32-system32-exports.txt")
            $exportNames = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
            foreach ($line in $exports) {
                if ($line -match '^\s+\d+\s+[0-9A-Fa-f]+\s+[0-9A-Fa-f]+\s+(\S+)') { [void]$exportNames.Add($Matches[1]) }
            }
            if ($exportNames.Count -eq 0) { throw 'No legacy Common Controls exports parsed.' }
            $legacyMissing = @($byLibrary[$name] | Where-Object { -not $exportNames.Contains($_) })
        }
        $libraries += [ordered]@{ name = $name; resolvedPath = $resolvedPath; loadError = $loadError; importedNames = $byLibrary[$name]; missingNamesInProbeContext = $missing; absentFromLegacyCommonControls = $legacyMissing }
    }

    # --list runs only Rust's test harness, not test bodies or the native UI.
    $start = [Diagnostics.ProcessStartInfo]::new($executable.FullName)
    $start.ArgumentList.Add('--list')
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $child = [Diagnostics.Process]::Start($start)
    try {
        $stdout = $child.StandardOutput.ReadToEndAsync()
        $stderr = $child.StandardError.ReadToEndAsync()
        $timedOut = -not $child.WaitForExit(10000)
        if ($timedOut) { $child.Kill($true); $child.WaitForExit() }
        $exitHex = '0x{0:X8}' -f ($child.ExitCode -band 0xFFFFFFFFL)
        $stdout.GetAwaiter().GetResult() | Set-Content -Encoding utf8 (Join-Path $outputRoot "$stem.list.stdout.txt")
        $stderr.GetAwaiter().GetResult() | Set-Content -Encoding utf8 (Join-Path $outputRoot "$stem.list.stderr.txt")
        $hasV6 = [bool]($manifest -and $manifest -match 'Microsoft\.Windows\.Common-Controls' -and $manifest -match 'version\s*=\s*["'']6\.0\.0\.0["'']')
        $reports += [ordered]@{ executable = $executable.Name; sha256 = (Get-FileHash -Algorithm SHA256 $executable.FullName).Hash; manifestPresent = [bool]$manifest; commonControlsV6 = $hasV6; listExitCode = $exitHex; listTimedOut = $timedOut; libraries = $libraries }
        Write-Host "$($executable.Name): --list=$exitHex; embedded Common Controls v6=$hasV6"
        foreach ($library in $libraries) {
            if ($library.missingNamesInProbeContext.Count -gt 0 -or $library.absentFromLegacyCommonControls.Count -gt 0 -or $library.loadError -ne 0) {
                Write-Host ($library | ConvertTo-Json -Depth 5 -Compress)
            }
        }
    } finally { $child.Dispose() }
}
[ordered]@{ schemaVersion = 1; operatingSystem = [Environment]::OSVersion.VersionString; processArchitecture = $machineArchitecture; resolverContext = 'PowerShell process; target manifest and legacy Common Controls exports recorded independently'; namedImportsOnly = $true; executables = $reports } |
    ConvertTo-Json -Depth 10 | Set-Content -Encoding utf8 (Join-Path $outputRoot 'report.json')
