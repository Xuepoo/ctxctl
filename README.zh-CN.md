# ctxctl

**你的编程代理为每一个字节的上下文付费。ctxctl 让文件和命令输出在到达模型之前就变小。**

代理习惯把整个文件和原始构建日志塞进上下文，然后反复烧 token 去重读已经看过的内容。ctxctl 是一个纯 CLI、零 MCP、无状态的上下文层：代理只读它需要的符号——通过 tree-sitter AST 定位 → 原始源码切片——并用字节稳定（byte-stable）的输出压缩命令结果，稳稳命中 provider 的提示词缓存。

```bash
ctxctl outline src/server.rs
```

```text
# src/server.rs  [12 symbols, ~2.1 KB -> ~410 tokens, saved ~80%]
  fn     handle_request  L:42-58      pub async fn handle_request(&self, id: u64)
  struct Config           L:60-71      pub struct Config {
  fn     validate         L:73-88      fn validate(cfg: &Config) -> Result<(), Error> {
```

```bash
ctxctl symbol src/server.rs --name handle_request --compact
```

```text
# handle_request  src/server.rs:42-58  (58 tokens, saved ~85%)
pub async fn handle_request(&self, id: u64) -> Result<String, Error> { ... }
```

```bash
ctxctl exec "cargo build"
```

```text
$ cargo build
error[E0308]: mismatched types --> src/main.rs:12
... [34 lines omitted]
Saved ~70% (1,240 -> 372 tokens)
```

不整文件倾倒，不堆原始日志。输出确定性强——相同输入产出相同字节，缓存持续命中。

[English](README.md) | 简体中文

## 安装

### Cargo（推荐）

```bash
cargo install ctxctl
```

### npm

```bash
npm install -g ctxctl
# 或
bun add -g ctxctl
```

### GitHub Releases

从 [releases 页面](https://github.com/Xuepoo/ctxctl-cli/releases) 下载对应平台的预编译二进制（同时提供 `.deb`、`.rpm`、`.apk` 和 Arch 包）。

## 快速上手

```bash
ctxctl outline src/main.rs                         # 符号地图 + 节省统计
ctxctl symbol src/main.rs --name run --compact     # 单个符号，折叠函数体
ctxctl read src/main.rs --lines 40-80              # 原始行切片
ctxctl deps src/main.rs                            # 依赖图（本地/外部）
ctxctl exec "cargo test" --keep "FAILED|passed"    # 运行命令 + 压缩输出
ctxctl outline src/main.rs --json                  # 机器可读契约
```

## 命令

| 命令                                  | 用途                                                          |
| ------------------------------------- | ------------------------------------------------------------- |
| `outline <file>`                      | 符号大纲 + token 节省统计                                     |
| `symbol <file> --name <s>`            | 单个符号的原始源码切片（`--compact`/`--signature`/`--lines`） |
| `read <file> --lines 100-150,200-210` | 原始行区间切片（不走 AST）                                    |
| `deps <file>`                         | 导入/模块依赖图（本地 / 外部 / 忽略）                         |
| `exec <cmd> [--keep <pat>]`           | 运行命令并压缩其输出                                          |

全局参数：`--json`（机器契约）、`--config <path>`、`--no-saved`。配置优先级：`--config` > `.ctxctl/config.toml`（向上逐级查找）> XDG > 默认值。

## Agent Skill 安装

为你的编程代理加载 ctxctl skill，让它学会按符号切片读文件、压缩命令输出，而不是倾倒原始上下文：

```bash
# 列出可用 skills
npx skills add Xuepoo/ctxctl-skills --list

# 为所有已检测到的代理安装核心 skill
npx skills add Xuepoo/ctxctl-skills --all

# 或只为某个代理安装
npx skills add Xuepoo/ctxctl-skills --skill ctxctl-core --agent claude-code
```

临时使用（不安装）：

```bash
npx skills use Xuepoo/ctxctl-skills --skill ctxctl-core
```

skill 赋予代理对 `outline` / `symbol` / `read` / `deps` / `exec` 工作流的一等感知——每个任务更少 token，运行更友好地命中缓存。CLI 支持 GitHub 简写、完整 URL 与本地路径，详见 <https://github.com/vercel-labs/skills>。

## 文档

- 网站与指南：[ctxctl.xuepoo.xyz](https://ctxctl.xuepoo.xyz)
- Agent skill 源码：[ctxctl-skills](https://github.com/Xuepoo/ctxctl-skills)

## 设计原则

- **零 MCP、无状态**——没有服务端、没有状态文件、没有后台进程。每次调用自包含。
- **字节稳定输出**——没有时间戳、没有计数器；相同输入产生相同字节，让 provider 提示词缓存保持热度。
- **切片而非摘要**——符号按字节区间取自原始源码，不做任何改写。
- **依赖最小**——11 种语言后端全部基于 tree-sitter，零网络依赖。

## 开发

Rust edition 2024 workspace：`ctx-symbol`（符号引擎）、`ctx-exec`（输出压缩）、`ctxctl`（薄 clap 外壳）。

```bash
just check      # fmt + clippy + check + test
cargo test      # 128 tests
```

## 许可证

MIT
