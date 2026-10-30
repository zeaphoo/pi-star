# 内核代码来源：pi-rust（rpi）

pi-star 的 Agent 内核来自 [pi-rust](https://github.com/bigfish1913/pi-rust)（命令名 `rpi`）的
源码复制。**这是一次性源码融合，不是依赖，也不保留上游同步关系**：融合后的代码由 pi-star
自行维护，rpi 后续的修复不会自动进入本仓库。

## 来源

| 项 | 值 |
| --- | --- |
| 上游 | `https://github.com/bigfish1913/pi-rust` |
| 版本 | `0.14.0` |
| `commit.id` | `534bcbffb7e1e7551d9ee3572dfeb278e203e493` |
| 复制时 HEAD | `0a3593b` |
| 许可 | MIT（含对 pi / Mario Zechner 的署名） |
| 上游来源 | rpi 本身是 [`earendil-works/pi`](https://github.com/earendil-works/pi) 的 Rust 移植 |

## 映射

| 本仓库 | 上游 |
| --- | --- |
| `crates/pi-star-ai` | `crates/rpi-ai` |
| `crates/pi-star-agent` | `crates/rpi-agent` |
| `crates/pi-star-tools` | `crates/rpi-tools` |
| `crates/pi-star-harness` | `crates/rpi-harness` |

未复制的上游 crate：`rpi-cli`、`rpi-tui`、`rpi-runtime`、`rpi-plugin-sdk`、`rpi-extensions`、
`rpi-telemetry`。pi-star 保留自己的配置/凭据/模型目录、终端、HTTP 路由和扩展边界。

## 已做的修改

- crate 名与 Rust 标识符 `rpi_*` → `pi_star_*`，包名改为 `pi-star-*`。
- 去掉 `rpi-telemetry`（上游四个 crate 中无实际引用）。
- `pi-star-tools` 不再依赖 `rpi-plugin-sdk`：`SESSION_ID_ENV` / `SUBAGENT_PARENT_ENV`
  改为本地常量。
- WebP 输出改用 `image 0.25` 的 lossless encoder（上游的 lossy API 在当前依赖版本中不存在）。
- 后续（v3 存储改造）会删除上游的 `session/jsonl/**` 与 `session/sqlite.rs`，改用 pi-star
  现有的 v3 JSONL 读写层。

## 维护策略

- 不跟踪上游分支、不做 rebase。需要比对时可 checkout 上表的 commit 后手工 diff。
- 本仓库的修改直接改在复制后的代码里，不设补丁目录。
- 上游的 MIT 署名保留在根目录 `NOTICE`；新增文件不改变这一归属。
