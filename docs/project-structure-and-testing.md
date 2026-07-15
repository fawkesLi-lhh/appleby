# 项目目录结构与测试规划

> 更新日期：2026-07-15
>
> 本文基于当前仓库代码整理，用于约定“每个目录应该放什么”，并给出后续测试代码的推荐组织方式。

## 1. 项目概览

Appleby 当前是一个 Rust 2024 edition 项目，同时包含：

- 一个可复用的库 crate：导出配置模块和工具模块；
- 两个二进制入口：`appleby` 与 `r1`；
- 一组面向 AI 工具调用的文件和 Shell 工具；
- 一份本地 TOML 运行配置。

当前主要调用关系如下：

```text
src/bin/appleby.rs
    └── appleby::config::CONFIG
            └── conf/config.toml
    └── anthropic-ai-sdk
            └── 发送一条消息到配置的 API 地址

src/lib.rs
    ├── config
    └── tool
          ├── bash
          ├── read_file
          ├── write_file
          └── edit_file
```

需要注意：`src/tool/` 已经由库导出，但当前 `src/bin/appleby.rs` 还没有把这些工具接入模型的 tool-use 循环。`src/bin/r1.rs` 目前只是一个占位程序。

## 2. 当前目录树

以下仅列出有意义的项目文件，省略 `.git/` 内部文件和 `target/` 编译产物：

```text
Appleby/
├── .claude/
│   └── settings.local.json
├── conf/
│   └── config.toml
├── docs/
│   └── project-structure-and-testing.md
├── src/
│   ├── bin/
│   │   ├── appleby.rs
│   │   └── r1.rs
│   ├── tool/
│   │   ├── mod.rs
│   │   ├── bash.rs
│   │   ├── read_file.rs
│   │   ├── write_file.rs
│   │   └── edit_file.rs
│   ├── config.rs
│   └── lib.rs
├── target/
├── .gitignore
├── Cargo.lock
└── Cargo.toml
```

## 3. 每个文件夹放什么

### 3.1 项目根目录 `/`

根目录只放整个 Rust package 级别的文件，不放具体业务实现。

当前文件：

| 文件 | 当前职责 | 后续约定 |
| --- | --- | --- |
| `Cargo.toml` | 声明 package 信息和依赖 | 生产依赖放 `[dependencies]`，仅测试使用的依赖放 `[dev-dependencies]` |
| `Cargo.lock` | 锁定依赖版本 | 本项目包含可执行程序，建议继续提交到 Git |
| `.gitignore` | 忽略 Cargo 构建目录 | 后续应补充本地密钥配置、临时文件、覆盖率产物等规则 |

根目录后续还可以放：

- `README.md`：项目简介、启动方式和最短使用示例；
- `LICENSE`：项目许可证；
- `rustfmt.toml`、`clippy.toml`：确有定制需求时再添加；
- CI 配置目录，例如 `.github/workflows/`。

### 3.2 `.claude/`

用途：项目级 Claude Code 辅助配置。

当前的 `settings.local.json` 配置了本地允许执行的 Cargo、rustfmt 等命令。文件名带 `local`，因此更适合保存开发者本机设置，不应承载项目运行配置、业务数据或密钥。

约定：

- 可共享的协作规则放项目级配置；
- 仅个人适用的权限和工具设置保留在 `settings.local.json`；
- 不在这里放 Rust 源码或应用配置。

### 3.3 `conf/`

用途：应用运行时配置及其模板。

当前 `config.toml` 提供以下字段：

- `anthropic_api_key`：API 凭据；
- `anthropic_model`：模型名；
- `anthropic_base_url`：API 基础地址。

`src/config.rs` 当前从固定相对路径 `conf/config.toml` 读取配置，并反序列化为 `Config`。

推荐调整为：

```text
conf/
├── config.example.toml   # 可提交；只包含占位值和字段说明
└── config.toml           # 本地真实配置；加入 .gitignore
```

约定：

- 仓库中只提交不含真实密钥的示例配置；
- 本地密钥优先从环境变量或未跟踪的 `config.toml` 读取；
- 测试配置不要放在这里，应放到 `tests/fixtures/config/`，避免测试修改开发者配置；
- 生产部署配置由部署系统注入，不应写死在仓库。

### 3.4 `docs/`

用途：项目文档。

适合放：

- 架构与模块边界说明；
- 目录结构约定；
- 配置说明；
- 测试策略；
- 重要技术决策记录（可进一步建立 `docs/adr/`）。

不放：编译产物、运行日志、测试临时文件或真实凭据。

### 3.5 `src/`

用途：所有生产 Rust 源码。

约定：

- `lib.rs` 负责定义库的公开模块和公开 API；
- 可复用逻辑应进入库模块，不要堆在二进制的 `main` 函数里；
- 与某个模块私有实现紧密相关的单元测试，直接写在对应 `.rs` 文件底部的 `#[cfg(test)] mod tests` 中；
- 当单个模块持续变大时，再从单文件拆成同名子目录，不提前制造空层级。

#### `src/lib.rs`

库 crate 根入口，目前：

- 导出 `config`；
- 导出 `tool`；
- 将 SDK 的 `Tool` 类型重命名导出为 `ToolSpec`，供内部工具实现生成模型可识别的工具定义。

这里应保持轻量，只做模块声明、公开 re-export 和 crate 级文档，不放具体执行流程。

#### `src/config.rs`

配置领域模块，目前负责：

- 定义 `Config` 数据结构；
- 从 `conf/config.toml` 加载配置；
- 通过 `LazyLock` 暴露全局 `CONFIG`。

后续建议把“解析字符串”“从指定路径加载”“加载默认配置”拆成可独立测试的方法，例如：

```rust
Config::from_toml(...)
Config::load_from(path)
Config::load_default()
```

这样测试不需要依赖进程当前工作目录，也不会读写真实配置。

### 3.6 `src/bin/`

用途：Cargo 二进制程序入口。该目录下每个 `.rs` 文件都会形成一个独立 binary target。

约定：二进制入口应尽量薄，只负责：

1. 初始化配置、日志和运行时；
2. 解析命令行参数；
3. 调用 `src/` 中可复用的应用逻辑；
4. 将错误转换成合适的退出码和用户输出。

不应在 `src/bin/` 中长期堆放可复用业务逻辑，否则集成测试和复用都会变困难。

#### `src/bin/appleby.rs`

当前主程序入口，执行流程是：

1. 克隆全局配置；
2. 创建 API 客户端；
3. 构造一条固定的用户消息；
4. 调用消息接口；
5. 打印返回内容。

后续建议把“构造客户端”“执行一轮对话”“处理工具调用”下沉到库模块，例如 `src/app.rs` 或 `src/agent.rs`，`main` 只调用这些函数。

#### `src/bin/r1.rs`

当前仅打印 `Hello, world!`，属于实验或占位入口。

如果它会发展成正式功能，应明确命名和职责；如果只用于临时验证，建议迁移到 `examples/` 或在验证结束后删除，避免用户误认为它是受支持的正式命令。

### 3.7 `src/tool/`

用途：AI 可调用工具的抽象、注册和具体实现。

当前结构：

| 文件 | 职责 |
| --- | --- |
| `mod.rs` | 定义 `Tool` trait、创建工具注册表 `toolset()`、提供工作区路径检查 `safe_path()` |
| `bash.rs` | 通过 `sh -c` 执行命令，合并标准输出和错误输出，限制输出长度并设置 120 秒超时 |
| `read_file.rs` | 异步读取文本文件的指定行范围并返回总行数 |
| `write_file.rs` | 异步写入完整文件，并尝试创建父目录 |
| `edit_file.rs` | 对文件执行精确字符串替换，支持替换一次或全部替换 |

新增工具时推荐保持“一种工具一个文件”：

```text
src/tool/
├── mod.rs
├── bash.rs
├── read_file.rs
├── write_file.rs
├── edit_file.rs
└── new_tool.rs
```

每个工具文件应包含：

- 工具实现类型；
- 创建 `Box<dyn Tool>` 的构造函数；
- `Tool::invoke` 实现；
- `name()` 和 `tool_spec()`；
- 与私有实现紧密相关的单元测试。

`mod.rs` 只负责公共抽象、共享安全逻辑和注册，不应逐渐变成所有工具实现的集合。

### 3.8 `target/`

用途：Cargo 自动生成的构建目录，包括：

- 编译后的二进制和库；
- 增量编译缓存；
- build script 输出；
- 测试可执行文件和依赖元数据。

约定：

- 不手工修改；
- 不提交 Git；
- 出现异常构建缓存时可用 `cargo clean` 重建；
- 测试报告或覆盖率报告如需长期保存，应输出到明确的报告目录或 CI artifact，而不是依赖 `target/` 内部结构。

### 3.9 `.git/`

用途：Git 自身的版本库元数据，不属于项目源码。

约定：不手工编辑其中内容，不在项目文档中依赖其内部目录结构。

## 4. 推荐的测试目录结构

当前执行 `cargo test -- --list` 的结果是：库、`appleby` 二进制、`r1` 二进制和文档测试均为 **0 个测试**。

推荐逐步形成以下结构：

```text
Appleby/
├── src/
│   ├── config.rs                 # 文件底部放配置模块单元测试
│   └── tool/
│       ├── mod.rs                # safe_path、toolset 单元测试
│       ├── bash.rs               # BashTool 单元测试
│       ├── read_file.rs          # ReadFileTool 单元测试
│       ├── write_file.rs         # WriteFileTool 单元测试
│       └── edit_file.rs          # EditFileTool 单元测试
└── tests/
    ├── support/
    │   └── mod.rs                # 多个集成测试共享的辅助函数
    ├── fixtures/
    │   ├── config/
    │   │   ├── valid.toml
    │   │   ├── missing_field.toml
    │   │   └── invalid.toml
    │   └── files/
    │       ├── multiline.txt
    │       └── unicode.txt
    ├── tool_workflow.rs          # 多工具组合的集成测试
    ├── config_loading.rs         # 从外部视角验证配置加载
    ├── cli_r1.rs                 # r1 命令行行为测试
    ├── cli_appleby.rs            # 使用 mock HTTP 服务测试主程序
    └── live_api.rs               # 可选；真实服务冒烟测试，默认 #[ignore]
```

## 5. 测试应该分别放在哪里

### 5.1 模块内单元测试：放在 `src/**/*.rs`

适用范围：

- 私有函数；
- 单个工具的输入校验；
- 精确边界条件；
- 不需要从 crate 外部验证公开 API 的行为。

标准写法：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptive_test_name() {
        // arrange / act / assert
    }
}
```

异步工具使用现有 Tokio 依赖：

```rust
#[tokio::test]
async fn reads_requested_lines() {
    // ...
}
```

建议测试项：

#### `src/config.rs`

- 完整 TOML 可以正确解析；
- 缺少必填字段时返回明确错误；
- TOML 语法错误时返回明确错误；
- 指定路径不存在时返回错误而不是 panic；
- 测试不会读取真实的 `conf/config.toml`。

#### `src/tool/mod.rs`

- `toolset()` 注册四个预期工具且名称一致；
- 工作区内路径可以通过；
- `..` 路径逃逸、绝对路径逃逸和符号链接逃逸被拒绝；
- 新文件目标路径的安全校验有明确行为。

#### `src/tool/read_file.rs`

- 默认和指定行范围；
- 第一行、最后一行、空文件、越界范围；
- `start_line > end_line` 的错误；
- Unicode 内容；
- 50,000 字符截断；
- 返回的行号必须与源文件真实行号一致。

#### `src/tool/write_file.rs`

- 写入新文件；
- 自动创建父目录；
- 覆盖已有文件；
- Unicode 和空内容；
- 拒绝工作区外路径；
- 写入失败时保留底层错误上下文。

#### `src/tool/edit_file.rs`

- 唯一匹配时替换成功；
- 默认模式遇到多个匹配时拒绝；
- `replace_all = true` 替换全部；
- 空 `old_string`、找不到文本和文件不存在；
- 替换后其他内容保持不变；
- 拒绝工作区外路径。

#### `src/tool/bash.rs`

- 捕获 stdout；
- 捕获 stderr；
- 无输出时返回 `(no output)`；
- 非零退出码的产品语义要先明确，再用测试固定；
- 危险命令规则；
- 50,000 字符截断；
- 超时终止子进程；
- Windows 与 Unix Shell 的平台差异。

120 秒超时不适合每次单元测试真实等待。建议把超时时长变成 `BashTool` 的可注入字段，生产默认 120 秒，测试传入几十毫秒。

### 5.2 集成测试：放在 `tests/*.rs`

Cargo 会把 `tests/` 第一层的每个 `.rs` 文件编译成独立测试 crate。这里的测试只能使用库公开 API，适合验证模块组合和外部调用方式。

#### `tests/tool_workflow.rs`

建议通过 `appleby::tool::toolset()` 完成真实组合流程：

1. `write_file` 创建临时文件；
2. `read_file` 验证内容；
3. `edit_file` 修改内容；
4. 再次 `read_file` 验证结果；
5. 验证工作区逃逸会被拒绝。

这能验证工具注册表、JSON 输入、trait object 调用和文件系统实现是否真正连通。

#### `tests/config_loading.rs`

从 crate 外部验证公开配置 API。为避免全局 `LazyLock` 和测试并发互相影响，优先测试 `Config::load_from(temp_path)`，不要在测试中修改真实配置或进程全局工作目录。

#### `tests/cli_r1.rs`

验证：

- 进程能正常启动；
- 退出码符合预期；
- stdout 内容符合命令契约。

如果 `r1` 只是临时占位程序，则无需投入测试，应优先明确它是否保留。

#### `tests/cli_appleby.rs`

主程序依赖 HTTP API，自动测试不应调用真实外部服务。建议：

- 启动本地 mock HTTP server；
- 注入临时 API 地址和临时配置；
- 返回固定响应；
- 断言请求模型、消息和输出；
- 覆盖超时、401、429、5xx 和非法响应。

### 5.3 测试共享代码：放在 `tests/support/`

`tests/support/mod.rs` 只放测试辅助代码，例如：

- 创建临时工作区；
- 构造工具 JSON 输入；
- 写入 fixture；
- 启动 mock API；
- 统一定位 `tests/fixtures/`。

不要把测试辅助代码放入生产 `src/`，除非它本身就是应用需要公开的能力。

### 5.4 测试数据：放在 `tests/fixtures/`

fixture 是静态输入和预期输出，不包含 Rust 测试逻辑。

约定：

- 按领域分子目录，例如 `config/`、`files/`、`api/`；
- 文件体积保持小；
- 不包含真实 API key、用户数据或机器绝对路径；
- 动态生成的文件放临时目录，不提交到 fixtures；
- 每个 fixture 的用途应能从文件名看出来。

### 5.5 真实服务测试：`tests/live_api.rs`

真实 API 测试不应加入默认测试路径。若确实需要：

- 使用 `#[ignore = "requires live API credentials"]`；
- 仅从环境变量读取凭据；
- 不打印密钥；
- 在 CI 中使用受控 secret；
- 单独执行：`cargo test --test live_api -- --ignored`。

## 6. 推荐测试依赖

可以按实际需求加入：

```toml
[dev-dependencies]
tempfile = "3"      # 隔离文件系统测试
assert_cmd = "2"    # 启动并断言 binary 行为
predicates = "3"    # 配合 assert_cmd 断言输出
wiremock = "0.6"    # mock HTTP API；也可选用同类库
```

现有 `tokio` 已启用 `full` features，可直接用于 `#[tokio::test]`。

不要因为“以后可能用到”一次性加入所有测试库；先为实际测试加入最小依赖。

## 7. 测试命名和执行约定

### 命名

测试名建议描述可观察行为：

```text
loads_valid_toml
rejects_path_outside_workspace
replaces_all_occurrences_when_enabled
returns_no_output_marker_for_empty_command
```

避免 `test1`、`works`、`basic_test` 这类无法说明意图的名字。

### 常用命令

```bash
cargo fmt --check
cargo check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

可按层运行：

```bash
cargo test config::tests
cargo test tool::read_file::tests
cargo test --test tool_workflow
cargo test --test cli_appleby
```

## 8. 建议实施顺序

### 第一阶段：建立可测试边界

1. 配置加载改为返回 `Result`，避免 `unwrap()`；
2. 增加 `Config::load_from(path)`，让测试可使用临时配置；
3. 调整路径校验，使新文件写入既安全又可测试；
4. 让 Bash 超时时长可注入；
5. 把 `appleby` 主程序中的对话逻辑下沉到库模块。

### 第二阶段：高价值单元测试

优先覆盖：

1. `safe_path` 的工作区逃逸防护；
2. `write_file` 新文件和父目录创建；
3. `edit_file` 唯一匹配与全量替换；
4. `read_file` 行号和范围边界；
5. 配置错误处理；
6. Bash 危险命令和超时。

### 第三阶段：组合与 CLI 测试

1. 增加 `tests/tool_workflow.rs`；
2. 增加配置加载集成测试；
3. 用 mock API 测试 `appleby`；
4. 根据 `r1` 是否保留决定是否添加 CLI 测试。

### 第四阶段：CI 质量门槛

在 CI 中至少执行：

```text
cargo fmt --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
```

等测试稳定后，再增加覆盖率统计；不建议一开始用覆盖率百分比替代关键行为测试。

## 9. 当前代码中特别值得用测试固定的行为

以下不是目录调整要求，但会直接影响测试设计：

1. `Config::new()` 目前对文件读取和 TOML 解析都使用 `unwrap()`，配置错误会直接 panic；
2. `safe_path()` 先调用 `canonicalize()`，而不存在的新文件无法 canonicalize，可能导致 `write_file` 无法按设计创建新文件；
3. `read_file` 的行范围计算需要重点验证 1-based 边界和返回行号；
4. `bash` 通过 `sh` 启动，在 Windows 环境依赖额外 Shell，且当前黑名单只能覆盖少量字符串形式；
5. `appleby` 当前调用真实远程接口，不适合直接作为默认自动化测试；
6. `conf/config.toml` 当前承载凭据字段，测试和文档中都不应复制其真实值。

这些行为应先通过小范围重构形成清晰契约，再由测试锁定预期结果。
