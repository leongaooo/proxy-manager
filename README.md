# Proxy Manager · 代理设置

基于 **Tauri 2、Vue 3、TypeScript 和 Rust** 的轻量桌面工具，通过三个独立页签管理终端环境变量、操作系统手动代理和 Git 全局代理。界面为中文，当前版本 **0.2.0**。

> 本工具管理代理配置，不提供代理服务器、VPN 或网络连通性检测。请先准备可用的代理服务。

## 功能

- 查看应用进程与持久化配置中的 `HTTP_PROXY`、`HTTPS_PROXY`、`ALL_PROXY`、`NO_PROXY`。
- 分别设置 HTTP、HTTPS、SOCKS 系统手动代理及绕过列表。
- Windows：管理当前用户环境变量与 WinINet 代理，不修改 HKLM 或 WinHTTP。
- macOS：管理默认 Shell 的托管配置区块、当前用户 launchd 环境，以及选定网络服务的系统代理。
- 终端、系统、Git 分页编辑与保存，切换保留草稿，重新读取有未保存内容时需确认。隐藏页的非法输入不影响当前页保存。
- 保存前校验输入，保存后回读实际配置；失败保留草稿，明确显示失败或部分完成结果。
- Git 全局 `http.proxy` 支持指定代理、继承环境与不使用代理，覆盖 HTTP/HTTPS Git 传输。
- 对已有认证代理信息脱敏展示；终端页不写入新认证 URL，Git 页支持认证地址并默认隐藏新输入。

## 平台与限制

| 平台    | 实现范围                          | 验证说明                                                                                    |
| ------- | --------------------------------- | ------------------------------------------------------------------------------------------- |
| Windows | 用户环境变量、WinINet 手动代理    | 支持 Windows x64；自动化测试覆盖 fake 适配、临时 Git 配置及隔离注册表，不等于代理连通性验收 |
| macOS   | 默认 Shell、launchd、网络服务代理 | 包含平台适配及测试；0.2.0 未做 macOS 真机验证                                               |
| Linux   | 暂不支持                          | 平台入口返回不支持提示                                                                      |

修改持久化环境变量后，已启动的终端和应用可能仍持有旧环境，需要重新启动。系统代理是否被某个应用采用取决于该应用。PAC、自动发现和代理认证存在保护限制，应按界面给出的具体原因处理。

保存可能部分完成，且不提供跨区块的事务回滚；请以结果和回读快照为准。更改前建议记录原有代理配置；macOS 用户还应备份自己的 Shell 配置文件。

## Git 全局代理

Git 页只管理全局 `http.proxy`，使用独立参数调用 `git config --global`，不会拼接 Shell 命令。

| 模式       | 写入语义                                                        |
| ---------- | --------------------------------------------------------------- |
| 指定代理   | 设置 `http.proxy` 为一个代理 URL                                |
| 继承环境   | 删除全局 `http.proxy`；Git 继续按其配置优先级与环境变量选择代理 |
| 不使用代理 | 显式设置 `http.proxy` 为空字符串                                |

`http.proxy` 同时适用于 HTTP 和 HTTPS 传输。设置范围及环境变量关系见 [Git 官方文档](https://git-scm.com/docs/git-config#Documentation/git-config.txt-httpproxy)。系统级、仓库、worktree、命令行、`http.<url>.proxy` 和 `remote.<name>.proxy` 等设置仍可能影响实际行为；本工具不编辑这些覆盖项，也不删除已有非规范 `https.proxy`。

已有认证 URL 只显示脱敏值，地址未编辑时保留原值；新增地址遵循界面校验。保存前核对读取版本，配置发生变化时要求重读。发现重复键、http.proxy 来自 include 或多个全局来源时采取保守拒写，避免误改外部文件；请先在 Git 配置中消除冲突。Git 未安装、旧版本无法确认全局配置路径、文件锁占用、不可写及回读失败会给出可恢复提示，原始命令 stderr 不进入界面。Git 写入与回读不是一个跨进程原子事务，外部程序仍可能并发修改；已写入但无法确认时会明确提示部分完成，请重新读取。

## 本地开发

需要 Node.js 22.12+、pnpm 10、Rust stable，以及对应平台的 [Tauri 2 系统依赖](https://v2.tauri.app/start/prerequisites/)。Windows 需要 MSVC C++ 构建工具和 WebView2；macOS 需要 Xcode Command Line Tools。

```sh
git clone https://github.com/leongaooo/proxy-manager.git
cd proxy-manager
pnpm install --frozen-lockfile
pnpm tauri dev
```

`pnpm dev` 仅启动前端，操作系统读写依赖 Tauri 原生进程；完整功能请使用 `pnpm tauri dev`。

## 检查与打包

```sh
pnpm test
pnpm format:check
pnpm build
cargo test --manifest-path src-tauri/Cargo.toml
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
```

在对应操作系统上构建安装包：

```sh
# Windows NSIS 安装包
pnpm tauri build --bundles nsis

# macOS 应用及磁盘镜像（在 Mac 上运行）
pnpm tauri build --bundles app,dmg
```

产物默认位于 `src-tauri/target/release/bundle/`，不会随源码提交。发布给其他用户前，需要自行处理适用的代码签名与 macOS 公证。

## 代码结构

```text
src/
  api/              Tauri 调用边界
  components/       表单与保存结果
  composables/      草稿、读取与保存编排
  generated/        ts-rs 导出的 TypeScript 契约
  __tests__/        前端自动化测试
src-tauri/src/
  contracts.rs      Rust 数据契约
  rules.rs          输入校验与纯转换
  service.rs        终端与系统按区块保存及回读
  git_proxy.rs      Git 全局代理读取、冲突保护与保存
  platform/         Windows / macOS 平台适配
```

`src/generated/` 由 Rust 契约测试导出，请修改 Rust 契约并运行测试，不要手动维护生成类型。

## 贡献

欢迎通过 Issue 报告问题或提交 Pull Request。请说明操作系统、应用版本、复现步骤及预期行为，并在提交前运行上述检查。不要在 Issue、日志或截图中包含代理密码、访问令牌或私人网络配置。

## 许可证

[MIT](LICENSE)。第三方依赖及其商标仍遵循各自许可证和使用条件。
