<#
.SYNOPSIS
    跨專案 Windows 代碼簽名 (Code Signing) 自動配置與簽署工具

.DESCRIPTION
    為 Rust RAM Disk、Rust File Manager 以及各類 Windows 桌面應用程式提供完整的代碼簽署支援：
    1. 產生開發者專屬自簽代碼簽章憑證 (.pfx 與 .cer，有效期限 5 年)
    2. 自動匯入公鑰至本機受信任的根憑證授權單位 (信任後徹底免除 SmartScreen 攔截)
    3. 自動探測 Windows 10/11 SDK 內建之 signtool.exe，支援 .exe 與 .msi 數位簽署與 RFC 3161 時間戳
    4. 匯出 GitHub Actions CI/CD 所需之 Base64 密鑰格式，支援雲端自動發布簽署

.PARAMETER Action
    執行動作：
    - Create:       建立自簽代碼簽章憑證 (.pfx / .cer)
    - InstallTrust: 將公開金鑰匯入本機受信任根憑證 (解除本機 SmartScreen 警告)
    - Sign:         簽署指定的二進位檔或目錄下的所有 .msi / .exe
    - ExportSecret: 匯出適用於 GitHub Secrets 的 Base64 字串與密碼
    - Status:       檢測系統工具鏈、憑證存在狀態與信任狀態 (預設)

.PARAMETER Path
    當 Action 為 Sign 時，指定要簽署的檔案路徑 (例如 release-artifacts\*.msi)

.PARAMETER CertDir
    存放憑證的目錄 (預設為 $HOME\.code-signing)

.PARAMETER CommonName
    憑證發行者名稱 (預設為 "William Wang (DevOps Code Signing)")

.PARAMETER Password
    PFX 私鑰密碼 (若未指定，將自憑證目錄 credentials.json 讀取或自動產生)

.EXAMPLE
    # 查看當前系統 SDK、憑證與信任狀態
    ./scripts/manage-code-signing.ps1 -Action Status

    # 1. 建立自簽憑證
    ./scripts/manage-code-signing.ps1 -Action Create

    # 2. 匯入信任 (此後本機雙擊簽名後的安裝檔將不再出現 SmartScreen 藍色視窗)
    ./scripts/manage-code-signing.ps1 -Action InstallTrust

    # 3. 簽署 MSI 安裝檔
    ./scripts/manage-code-signing.ps1 -Action Sign -Path .\release-artifacts\RustRAMDisk-v0.2.0-x64.msi

    # 4. 匯出 GitHub Actions Secret
    ./scripts/manage-code-signing.ps1 -Action ExportSecret
#>

[CmdletBinding()]
param(
    [ValidateSet("Create", "InstallTrust", "Sign", "ExportSecret", "Status")]
    [string]$Action = "Status",

    [string]$Path = "",
    [string]$CertDir = (Join-Path $HOME ".code-signing"),
    [string]$CommonName = "William Wang (DevOps Code Signing)",
    [string]$Password = "",
    [switch]$Force
)

$ErrorActionPreference = "Stop"

function Write-Info([string]$msg) { Write-Host "[INFO]  $msg" -ForegroundColor Cyan }
function Write-Ok([string]$msg)   { Write-Host "[OK]    $msg" -ForegroundColor Green }
function Write-Warn([string]$msg) { Write-Host "[WARN]  $msg" -ForegroundColor Yellow }
function Write-Err([string]$msg)  { Write-Host "[ERROR] $msg" -ForegroundColor Red }

$PfxPath = Join-Path $CertDir "code-signing.pfx"
$CerPath = Join-Path $CertDir "code-signing.cer"
$CredFile = Join-Path $CertDir "credentials.json"

# ============================================================================
# 工具函式：自動搜尋本機 Windows SDK signtool.exe
# ============================================================================
function Find-Signtool {
    $cmd = Get-Command "signtool.exe" -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -ErrorAction SilentlyContinue
    if ($cmd -and (Test-Path $cmd)) { return $cmd }

    $candidates = @()
    $kitsRoot = "${env:ProgramFiles(x86)}\Windows Kits\10\bin"
    if (Test-Path $kitsRoot) {
        $candidates += Get-ChildItem -Path $kitsRoot -Directory -Filter "10.0.*" -ErrorAction SilentlyContinue |
            Sort-Object Name -Descending |
            ForEach-Object { Join-Path $_.FullName "x64\signtool.exe" }
    }
    $appCert = "${env:ProgramFiles(x86)}\Windows Kits\10\App Certification Kit\signtool.exe"
    if (Test-Path $appCert) { $candidates += $appCert }

    foreach ($c in $candidates) {
        if (Test-Path $c) { return $c }
    }
    return $null
}

# 取得或讀取憑證密碼
function Get-CertPassword {
    if (-not [string]::IsNullOrWhiteSpace($Password)) {
        return $Password
    }
    if ($env:CODE_SIGNING_PASSWORD) {
        return $env:CODE_SIGNING_PASSWORD
    }
    if (Test-Path $CredFile) {
        try {
            $cred = Get-Content $CredFile -Raw | ConvertFrom-Json
            if ($cred.Password) { return $cred.Password }
        } catch {}
    }
    return "DevOpsCodeSign2026!"
}

# ============================================================================
# Action: Status
# ============================================================================
if ($Action -eq "Status") {
    Write-Info "=========================================================="
    Write-Info "Windows 代碼簽名 (Code Signing) 環境狀態診斷"
    Write-Info "=========================================================="

    $signtool = Find-Signtool
    if ($signtool) {
        Write-Ok "微軟簽署工具就緒 (signtool.exe): $signtool"
    } else {
        Write-Warn "未找到 signtool.exe！請確認 Windows 10/11 SDK 是否正確安裝。"
    }

    Write-Info "`n憑證檔案狀態 ($CertDir):"
    $hasPfx = Test-Path $PfxPath
    $hasCer = Test-Path $CerPath

    if ($hasPfx) {
        Write-Ok "  [已存在] 私鑰憑證: $PfxPath"
    } else {
        Write-Warn "  [未建立] 私鑰憑證不存在，可透過 -Action Create 自動建立"
    }

    if ($hasCer) {
        Write-Ok "  [已存在] 公開金鑰: $CerPath"
        $certObj = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2($CerPath)
        Write-Info "           主體: $($certObj.Subject)"
        Write-Info "           指紋: $($certObj.Thumbprint)"
        Write-Info "           有效期限至: $($certObj.NotAfter)"
    } else {
        Write-Warn "  [未建立] 公開金鑰不存在"
    }

    Write-Info "`n本機信任狀態 (Trusted Root Certification Authorities):"
    $trusted = $false
    if ($hasCer) {
        $certObj = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2($CerPath)
        $installed = Get-ChildItem Cert:\CurrentUser\Root -ErrorAction SilentlyContinue | Where-Object { $_.Thumbprint -eq $certObj.Thumbprint }
        if (-not $installed) {
            $installed = Get-ChildItem Cert:\LocalMachine\Root -ErrorAction SilentlyContinue | Where-Object { $_.Thumbprint -eq $certObj.Thumbprint }
        }
        if ($installed) {
            $trusted = $true
            Write-Ok "  [已信任] 該憑證已安裝於本機受信任根憑證存放區！(雙擊安裝免除 SmartScreen 警告)"
        } else {
            Write-Warn "  [尚未信任] 該憑證尚未安裝於受信任根憑證，可執行 -Action InstallTrust 進行一鍵信任"
        }
    } else {
        Write-Warn "  需先建立憑證方能檢測信任狀態"
    }

    Write-Info "`n使用提示："
    Write-Info "  1. 建立憑證:          ./scripts/manage-code-signing.ps1 -Action Create"
    Write-Info "  2. 信任此憑證:        ./scripts/manage-code-signing.ps1 -Action InstallTrust"
    Write-Info "  3. 簽署安裝包:        ./scripts/manage-code-signing.ps1 -Action Sign -Path <file.msi>"
    Write-Info "  4. 取得 CI/CD 密鑰:   ./scripts/manage-code-signing.ps1 -Action ExportSecret"
    exit 0
}

# ============================================================================
# Action: Create (建立專屬自簽憑證)
# ============================================================================
if ($Action -eq "Create") {
    if (-not (Test-Path $CertDir)) {
        New-Item -ItemType Directory -Path $CertDir -Force | Out-Null
    }

    if ((Test-Path $PfxPath) -and (-not $Force)) {
        Write-Warn "憑證已存在於 $PfxPath。若欲強制重新建立，請加上 -Force 參數。"
        exit 0
    }

    Write-Info "正在建立有效期 5 年之 Code Signing 代碼簽章憑證..."
    Write-Info "主體名稱: CN=$CommonName"

    $cert = New-SelfSignedCertificate `
        -Type CodeSigningCert `
        -Subject "CN=$CommonName" `
        -CertStoreLocation "Cert:\CurrentUser\My" `
        -NotAfter (Get-Date).AddYears(5) `
        -HashAlgorithm "SHA256" `
        -KeyLength 2048 `
        -KeyUsage DigitalSignature `
        -FriendlyName "$CommonName"

    $pass = Get-CertPassword
    $securePass = ConvertTo-SecureString $pass -AsPlainText -Force

    # 匯出公開金鑰 .cer (安全可公開)
    Export-Certificate -Cert $cert -FilePath $CerPath -Force | Out-Null
    Write-Ok "公開金鑰已匯出至: $CerPath"

    # 匯出私鑰 .pfx (需密碼保護)
    Export-PfxCertificate -Cert $cert -FilePath $PfxPath -Password $securePass -Force | Out-Null
    Write-Ok "私鑰憑證已匯出至: $PfxPath"

    # 儲存認證資訊
    @{
        CommonName = $CommonName
        Thumbprint = $cert.Thumbprint
        Created = (Get-Date).ToString("yyyy-MM-dd HH:mm:ss")
        Expires = $cert.NotAfter.ToString("yyyy-MM-dd HH:mm:ss")
        Password = $pass
        PfxPath = $PfxPath
        CerPath = $CerPath
    } | ConvertTo-Json -Depth 3 | Out-File $CredFile -Encoding utf8

    Write-Ok "認證設定檔已更新至: $CredFile"
    Write-Ok "`n🎉 憑證建立完成！接下來請執行以下指令將其加入本機信任："
    Write-Host "   ./scripts/manage-code-signing.ps1 -Action InstallTrust`n" -ForegroundColor Green
    exit 0
}

# ============================================================================
# Action: InstallTrust (將公鑰匯入本機受信任根憑證，消除 SmartScreen)
# ============================================================================
if ($Action -eq "InstallTrust") {
    if (-not (Test-Path $CerPath)) {
        throw "找不到公開金鑰 $CerPath，請先執行 -Action Create 建立憑證！"
    }

    Write-Info "正在將公開金鑰 ($CerPath) 匯入本機使用者的受信任的根憑證授權單位..."
    $certObj = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2($CerPath)

    # 檢查是否已存在
    $existing = Get-ChildItem Cert:\CurrentUser\Root -ErrorAction SilentlyContinue | Where-Object { $_.Thumbprint -eq $certObj.Thumbprint }
    if ($existing) {
        Write-Ok "該憑證早已存在於 Cert:\CurrentUser\Root，無需重複匯入！"
        exit 0
    }

    try {
        # 使用 Import-Certificate 匯入至 CurrentUser\Root (無需管理員 UAC 提權即可信任)
        Import-Certificate -FilePath $CerPath -CertStoreLocation "Cert:\CurrentUser\Root" | Out-Null
        Write-Ok "🎉 憑證已成功匯入 Cert:\CurrentUser\Root！"
        Write-Ok "從現在起，只要由該憑證簽署的任何 MSI 或 EXE，本機執行時一律獲得完整信任，永不跳出 SmartScreen 攔截畫面！"
    } catch {
        Write-Warn "透過 PowerShell 匯入失敗，嘗試調用 certutil.exe 工具..."
        & certutil.exe -addstore -user Root $CerPath
        if ($LASTEXITCODE -eq 0) {
            Write-Ok "🎉 透過 certutil 匯入受信任根憑證成功！"
        } else {
            throw "匯入受信任根憑證失敗，代碼: $LASTEXITCODE"
        }
    }
    exit 0
}

# ============================================================================
# Action: Sign (數位簽署目標檔案)
# ============================================================================
if ($Action -eq "Sign") {
    if (-not (Test-Path $PfxPath)) {
        throw "找不到私鑰憑證 $PfxPath，請先執行 -Action Create 產生憑證！"
    }

    $signtool = Find-Signtool
    if (-not $signtool) {
        throw "未找到 signtool.exe！請安裝 Windows 10/11 SDK。"
    }

    if ([string]::IsNullOrWhiteSpace($Path)) {
        throw "請透過 -Path 參數指定要簽署的檔案或目錄！"
    }

    $targets = @()
    if (Test-Path $Path) {
        if ((Get-Item $Path) -is [System.IO.DirectoryInfo]) {
            $targets += Get-ChildItem -Path $Path -Include "*.msi", "*.exe" -Recurse | Select-Object -ExpandProperty FullName
        } else {
            $targets += (Get-Item $Path).FullName
        }
    } else {
        # 可能是 glob pattern (例如 release-artifacts\*.msi)
        $targets += Get-ChildItem -Path $Path -ErrorAction SilentlyContinue | Select-Object -ExpandProperty FullName
    }

    if ($targets.Count -eq 0) {
        throw "在路徑 '$Path' 找不到任何符合之 .msi 或 .exe 檔案！"
    }

    $pass = Get-CertPassword
    $timestampServers = @(
        "http://timestamp.digicert.com",
        "http://timestamp.sectigo.com",
        "http://tsa.starfieldtech.com"
    )

    foreach ($file in $targets) {
        Write-Info "正在簽署: $file ..."
        $signed = $false

        foreach ($ts in $timestampServers) {
            try {
                & $signtool sign /f $PfxPath /p $pass /fd sha256 /tr $ts /td sha256 $file 2>$null
                if ($LASTEXITCODE -eq 0) {
                    $signed = $true
                    Write-Ok "已成功簽署 (時間戳: $ts): $file"
                    break
                }
            } catch {}
        }

        if (-not $signed) {
            Write-Warn "時間戳伺服器連線超時，嘗試無時間戳本機離線簽署..."
            & $signtool sign /f $PfxPath /p $pass /fd sha256 $file
            if ($LASTEXITCODE -eq 0) {
                $signed = $true
                Write-Ok "已完成無時間戳本機簽署: $file"
            }
        }

        if (-not $signed) {
            throw "簽署失敗: $file"
        }

        # 驗證 Authenticode 簽章
        $sig = Get-AuthenticodeSignature $file
        Write-Info "  簽章狀態: $($sig.Status) | 簽署者: $($sig.SignerCertificate.Subject)"
    }

    Write-Ok "所有目標檔案簽署完成！"
    exit 0
}

# ============================================================================
# Action: ExportSecret (匯出適用於 GitHub Secrets 之 Base64)
# ============================================================================
if ($Action -eq "ExportSecret") {
    if (-not (Test-Path $PfxPath)) {
        throw "找不到私鑰憑證 $PfxPath，請先執行 -Action Create！"
    }

    $pass = Get-CertPassword
    $pfxBytes = [System.IO.File]::ReadAllBytes($PfxPath)
    $pfxBase64 = [Convert]::ToBase64String($pfxBytes)

    Write-Info "=========================================================="
    Write-Info "GitHub Actions 雲端自動簽署 Secrets 配置參數"
    Write-Info "=========================================================="
    Write-Info "請前往 GitHub 儲存庫頁面："
    Write-Info "Settings ➔ Secrets and variables ➔ Actions ➔ New repository secret`n"

    Write-Host "Secret 名稱 1: WINDOWS_CERTIFICATE_BASE64" -ForegroundColor Yellow
    Write-Host "Secret 數值 1 (複製下方整段 Base64):" -ForegroundColor Cyan
    Write-Host $pfxBase64 -ForegroundColor White

    Write-Host "`nSecret 名稱 2: WINDOWS_CERTIFICATE_PASSWORD" -ForegroundColor Yellow
    Write-Host "Secret 數值 2: $pass" -ForegroundColor Cyan

    Write-Info "`n=========================================================="
    Write-Ok "配置完成後，GitHub Actions 發布流程即可自動為 MSI 與 EXE 打上數位簽章！"
    exit 0
}
