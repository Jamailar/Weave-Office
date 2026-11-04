# 自动更新

Weave Office 自己检查新版本：发现新版本时提醒，点"更新"后在后台下载，下载完成后点"立即重启"就装好了。Windows 和 macOS 的安装包都支持；Linux 和开发构建不检查更新。

## 用户看到的流程

1. 启动约 5 秒后检查一次，之后应用开着时每 12 小时检查一次。设置 → 常规 里可以关掉"自动检查更新"，也可以点"立即检查"；文件菜单和关于窗口里也有"检查更新…"。
2. 有新版本时弹出提醒，显示版本号和发布说明，按钮是"稍后"和"更新"。同一个版本只自动提醒一次；之后标题栏右侧会一直显示"新版本 x.y.z"，点它可以再打开提醒。
3. 点"更新"后显示下载进度。可以点"后台下载"继续工作，标题栏显示下载百分比；也可以"取消下载"。
4. 下载并校验完成后提示"更新已就绪"，点"立即重启"。有未保存的文档时，和平常退出一样逐个询问是否保存；选"取消"就不重启，更新保持就绪。
5. 选"稍后"时，下次正常退出会自动安装，下次打开就是新版本。

自动检查失败时（例如没网）不打扰；手动检查失败会说明原因并可以重试。

## 安全校验

更新只从项目的 GitHub Releases 下载（仓库地址取自 `Cargo.toml` 的 `repository`），全程 HTTPS，使用系统的证书库和代理设置。

每个 Release 除了五个安装包和 `SHA256SUMS.txt`，还要有 `SHA256SUMS.txt.sig`：用发布私钥对 `SHA256SUMS.txt` 做的 Ed25519 签名。应用内置公钥（`crates/updater/update-public-key.txt`），先验签名，再核对下载的安装包与签名清单里的 SHA-256 一致，任何一步不通过都会删除下载、不安装。即使 GitHub 账号或下载链路被篡改，没有私钥也无法推送安装包。

公钥文件为空、Release 缺少签名文件，或这份应用无法自己替换自己（见下）时，提醒仍会出现，但按钮变成"打开下载页"，由用户手动安装。

## 各平台的安装方式

- **Windows**：下载对应架构的 `…-setup.exe`。重启时应用启动安装包 `/S /UPDATE /RELAUNCH /D=<当前安装目录>` 后退出；安装包等待 `weave-office.exe` 释放（最多 60 秒），静默覆盖安装，再启动新版本。安装包是按用户安装，不需要管理员权限。只有通过安装包安装的副本（目录里有 `Uninstall.exe`）才自动安装。
- **macOS**：下载对应架构的 DMG，在后台挂载，把新的 `.app` 复制到当前应用旁边的临时目录，检查 Bundle ID、版本号和代码签名（`codesign --verify --deep --strict`）。重启时用两次改名把新旧应用互换，应用退出后由一个小脚本删除旧版本并打开新版本。应用所在目录需要可写；从 DMG 里直接运行、被系统随机化路径（App Translocation）运行，或没有写权限时，改为"打开下载页"。下载由应用自己完成，不带隔离属性，新版本启动时不会被 Gatekeeper 拦下。

下载临时放在 macOS 的 `~/Library/Caches/Weave Office/Updates`、Windows 的系统临时目录，下次启动时清理。

## 发布步骤

只需要做一次：生成签名密钥，把公钥写进 `crates/updater/update-public-key.txt` 并提交。私钥不要放进仓库，建议放在本机安全位置并另外备份；丢了私钥只能换新密钥，而旧版本只认旧公钥。

```sh
cargo run -p weave-updater --example release-signing -- keygen ~/.weave-office/update-signing.key
```

每次发布：照常生成五个安装包和 `SHA256SUMS.txt`，然后签名并核对：

```sh
cargo run -p weave-updater --example release-signing -- sign ~/.weave-office/update-signing.key path/to/SHA256SUMS.txt
cargo run -p weave-updater --example release-signing -- verify path/to/SHA256SUMS.txt
```

把 `SHA256SUMS.txt.sig` 和其他文件一起上传到 Release。Release 的 tag 必须是 `vX.Y.Z`，安装包名保持 `WeaveOffice-X.Y.Z-macos-arm64.dmg`、`WeaveOffice-X.Y.Z-windows-x64-setup.exe` 这样的格式；草稿和预发布不会推送给用户。

注意：自动更新从内置了公钥的第一个版本开始生效。已经装了旧版本（没有更新功能）的用户需要手动装一次新版本。

## 测试

`cargo test -p weave-updater` 覆盖版本比较、安装包命名、GitHub Release 解析、签名与校验和（含篡改、外来密钥、格式错误）、Windows 安装参数和 macOS 重启脚本。界面部分见 `src/updates.rs` 的单元测试。完整的下载与替换安装需要在 Windows 和 Mac 上用两个签名版本实际走一遍。
