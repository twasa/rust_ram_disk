param(
    [string]$OutputDir = "$PSScriptRoot/../winfsp_bin"
)

$ErrorActionPreference = "Stop"

$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vcvars64 = $null

if (Test-Path $vswhere) {
    $installPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($installPath -and (Test-Path "$installPath\VC\Auxiliary\Build\vcvars64.bat")) {
        $vcvars64 = "$installPath\VC\Auxiliary\Build\vcvars64.bat"
    }
}

if (-not $vcvars64) {
    $candidates = @(
        "C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\VC\Auxiliary\Build\vcvars64.bat",
        "C:\Program Files\Microsoft Visual Studio\2022\Enterprise\VC\Auxiliary\Build\vcvars64.bat",
        "C:\Program Files\Microsoft Visual Studio\2022\Professional\VC\Auxiliary\Build\vcvars64.bat",
        "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat",
        "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
    )
    foreach ($cand in $candidates) {
        if (Test-Path $cand) {
            $vcvars64 = $cand
            break
        }
    }
}

if (-not $vcvars64) {
    throw "Visual Studio x64 C++ build tools (vcvars64.bat) not found."
}

Write-Host "Using MSVC environment: $vcvars64"

$resolvedOutputDir = [System.IO.Path]::GetFullPath($OutputDir)
if (-not (Test-Path $resolvedOutputDir)) {
    New-Item -ItemType Directory -Path $resolvedOutputDir -Force | Out-Null
}

$cmd = @"
call "$vcvars64" >nul 2>&1
cd /d "$PSScriptRoot"
cl /nologo /O2 /MD /EHsc /I. /Iwinfsp memfs.cpp memfs_main.c /Fe:memfs-x64.exe /link winfsp_x64.lib advapi32.lib kernel32.lib /SUBSYSTEM:CONSOLE
"@

$cmdFile = [System.IO.Path]::GetTempFileName() + ".bat"
Set-Content -Path $cmdFile -Value $cmd -Encoding ASCII

try {
    Write-Host "Compiling custom memfs-x64.exe..."
    $proc = Start-Process -FilePath "cmd.exe" -ArgumentList "/c `"$cmdFile`"" -NoNewWindow -Wait -PassThru
    if ($proc.ExitCode -ne 0) {
        throw "Compilation failed with exit code $($proc.ExitCode)"
    }
    
    $builtExe = Join-Path $PSScriptRoot "memfs-x64.exe"
    if (-not (Test-Path $builtExe)) {
        throw "Build output not found at $builtExe"
    }
    
    $targetExe = Join-Path $resolvedOutputDir "memfs-x64.exe"
    Copy-Item $builtExe $targetExe -Force
    Write-Host "Successfully compiled and deployed custom memfs-x64.exe to $targetExe"
} finally {
    if (Test-Path $cmdFile) { Remove-Item $cmdFile -Force -ErrorAction SilentlyContinue }
    Remove-Item (Join-Path $PSScriptRoot "*.obj") -Force -ErrorAction SilentlyContinue
}
