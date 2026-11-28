# Windows 代碼簽名與 SmartScreen 信任配置指南 (Windows Code Signing & SmartScreen SOP)

本手冊說明如何為 **Rust RAM Disk (`rust_ram_disk`)** 與 **Rust File Manager (`rust_file_manager`)** 配置數位代碼簽署（Authenticode Code Signing），徹底消除 Windows Defender SmartScreen 藍色攔截警告（「Windows 已保護您的電腦」），並在安裝與執行時呈現合法的發行者身份資訊。

---

## 1. 核心原理 (Architecture & Background)

### 為什麼 Windows 會出現 SmartScreen 警告？
當使用者從瀏覽器（Chrome、Edge 等）下載 `.msi` 或 `.exe` 檔案時，Windows 會自動在 NTFS 檔案系統附加一個隱藏的替代資料串流（Mark of the Web / `Zone.Identifier`，標示 `ZoneId=3`）。

當使用者雙擊執行帶有網路標記的檔案時，Windows Defender SmartScreen 會執行兩道查核：
1. **數位簽章查核 (Authenticode Check)**：該檔案是否具備合法的代碼簽名？（未簽名者直接列為未知發行者）
2. **微軟雲端信譽查核 (Cloud Reputation Check)**：該檔案在微軟雲端資料庫中是否已有足夠全球安裝量與信譽累積？（新發布的版本缺乏遙測樣本）

若兩者皆未通過，SmartScreen 便會強制阻擋並顯示警告。

### 解決方案架構
藉由本架構建立專屬的 **自簽代碼簽章憑證 (Self-Signed Code Signing Certificate)**，並將公開金鑰（`.cer`）註冊至本機的「受信任的根憑證授權單位 (Trusted Root Certification Authorities)」：
- **本機或受信任電腦**：Windows 會驗證簽章為合法有效，發行者清楚標註為「`William Wang`」，並直接解除 SmartScreen 攔截。
- **CI/CD 自動化**：將私鑰憑證轉為 Base64 託管於 GitHub Secrets，GitHub Actions 發布時自動簽署，發行出來的安裝包天然自帶數位憑證與防竄改時間戳。

```
[ 產生自簽憑證 (manage-code-signing.ps1 -Action Create) ]
         │
         ├─── 公開金鑰 (.cer) ───► 匯入本機信任區 (InstallTrust) ───► 本機雙擊安裝 0 警告
         │
         └─── 私鑰憑證 (.pfx) ───► Base64 注入 GitHub Secrets ───► GitHub Actions 自動簽署發布
```

---

## 2. 系統需求與環境依賴 (System Requirements)

| 元件 | 需求說明 | 本機就緒狀態 |
| :--- | :--- | :--- |
| **作業系統** | Windows 10 (1809+) 或 Windows 11 / Windows Server 2019+ | 已就緒 |
| **Windows SDK** | 包含 `signtool.exe`（通常位於 `Windows Kits\10\bin\<version>\x64\`） | **已就緒** (`10.0.26100.0`) |
| **Shell 環境** | PowerShell 7.x (`pwsh`) 或 Windows PowerShell 5.1+ | 已就緒 |
| **權限要求** | 匯入至當前使用者根憑證 (`Cert:\CurrentUser\Root`) **完全無需 Administrator 管理員提權** | 一般使用者即可執行 |

---

## 3. 本機一鍵自動配置 SOP (Local Setup)

專案提供全自動配置腳本 `./scripts/manage-code-signing.ps1`，兩大 Rust 專案皆已整合並共用統一憑證庫（預設位於 `$HOME\.code-signing`）。

### 步驟 1：建立專屬代碼簽章憑證
開啟 PowerShell，執行以下指令：
```powershell
./scripts/manage-code-signing.ps1 -Action Create
```
- 自動產生有效期限 **5 年** 的 SHA-256 代碼簽章憑證。
- 公開金鑰輸出至：`$HOME\.code-signing\code-signing.cer`
- 私鑰憑證輸出至：`$HOME\.code-signing\code-signing.pfx`

### 步驟 2：將憑證加入本機信任（消除 SmartScreen）
```powershell
./scripts/manage-code-signing.ps1 -Action InstallTrust
```
- 自動將公開金鑰匯入至 Windows `Cert:\CurrentUser\Root`。
- **立即生效**：此後由該憑證簽署的任何 MSI 或 EXE，在該電腦上執行將被視為完全信任，不再彈出 SmartScreen 攔截畫面！

### 步驟 3：手動或自動簽署安裝包
```powershell
# 手動簽署單一或多個 MSI / EXE (自動附加 RFC 3161 國際公認時間戳)
./scripts/manage-code-signing.ps1 -Action Sign -Path .\release-artifacts\RustRAMDisk-v0.2.0-x64.msi

# 或簽署整個目錄下的產物
./scripts/manage-code-signing.ps1 -Action Sign -Path .\release-artifacts
```

### 步驟 4：檢測環境與信任狀態
隨時可透過 Status 參數檢查工具鏈與憑證安裝情形：
```powershell
./scripts/manage-code-signing.ps1 -Action Status
```

---

## 4. GitHub Actions 雲端自動簽署配置 (CI/CD Integration)

若希望 GitHub Actions 每次打 tag 自動編譯發布的 MSI 都具備數位簽章，只需完成一次性金鑰登記：

### 步驟 1：取得憑證的 Base64 與密碼
執行腳本匯出 Secret 內容：
```powershell
./scripts/manage-code-signing.ps1 -Action ExportSecret
```
腳本將印出兩項參數：
1. `WINDOWS_CERTIFICATE_BASE64`（很長一段 Base64 文字）
2. `WINDOWS_CERTIFICATE_PASSWORD`（憑證密碼）

### 步驟 2：登錄至 GitHub 儲存庫 Secrets
分別前往 `rust_ram_disk` 與 `rust_file_manager` 的 GitHub 網頁：
1. 進入 **Settings** ➔ **Secrets and variables** ➔ **Actions**。
2. 點擊 **New repository secret**：
   - Name: `WINDOWS_CERTIFICATE_BASE64`，Value 貼上整段 Base64。
   - Name: `WINDOWS_CERTIFICATE_PASSWORD`，Value 輸入憑證密碼。
3. 點擊 **Add secret** 儲存。

### 步驟 3：GitHub Actions 工作流程原理
Release Workflow（`.github/workflows/release.yml`）已自動配置簽署步驟：
- 檢查若有提供 `WINDOWS_CERTIFICATE_BASE64`，便會在 Windows Runner 環境自動解碼並調用 `signtool.exe` 為 `.msi` 與 `.exe` 蓋上數位簽章與時間戳。
- 若尚未配置 Secret，則會自動安全跳過簽署，不中斷常規發布。

---

## 5. 跨專案共用與移植指南 (Cross-Project Compatibility)

### 跨專案支援
`manage-code-signing.ps1` 具備跨專案相容性，且預設共用 `$HOME\.code-signing` 憑證存放區：
- **Rust RAM Disk**：位於 `private/rust_ram_disk/scripts/manage-code-signing.ps1`
- **Rust File Manager**：位於 `private/rust_file_manager/scripts/manage-code-signing.ps1`
- **全域捷徑**：位於 `scripts/manage-code-signing.ps1`

只要在本機執行過一次 `-Action Create` 與 `-Action InstallTrust`，兩個專案在本地進行 Release 打包時，皆可自動讀取同一組憑證完成簽署！

### 分享至其他測試電腦
若要在另一台 Windows 電腦上測試，且同樣希望不出現 SmartScreen：
1. 將 `$HOME\.code-signing\code-signing.cer`（僅需公開金鑰，私鑰無需外流）複製到該電腦。
2. 在該電腦以 PowerShell 執行：
   ```powershell
   certutil.exe -addstore -user Root code-signing.cer
   ```
   或直接雙擊 `.cer` 檔 ➔ 點選「安裝憑證」➔ 存放區選擇「受信任的根憑證授權單位」。
3. 該電腦爾後執行所有您簽署的安裝包皆直接信任。

---

## 6. 簽章有效性驗證 (Verification)

簽署完成後，可透過以下方式驗證數位簽章是否有效：

### 1. PowerShell 檢驗
```powershell
Get-AuthenticodeSignature .\release-artifacts\RustRAMDisk-v0.2.0-x64.msi
```
預期輸出：
```text
SignerCertificate                         Status                    StatusMessage             Path
-----------------                         ------                    -------------             ----
XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX  Valid                     Success                   RustRAMDisk...
```
`Status` 顯示為 **`Valid`** 即代表數位簽章完整且受本機信任。

### 2. Windows 檔案總管 GUI 檢驗
1. 在檔案上按右鍵 ➔ 點選 **「內容」 (Properties)**。
2. 頂部將多出 **「數位簽章」 (Digital Signatures)** 分頁。
3. 清單中應顯示發行者名稱為 `William Wang (DevOps Code Signing)`，點擊「詳細資料」顯示「此數位簽章無誤」。
