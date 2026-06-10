# korg

**一个为自主 AI 智能体打造的、按因果排序、可回退的事件账本。**
*你的 AI 智能体走的每一步,都记进一本可独立验证的哈希链账本——防篡改、零信任、不用区块链。*

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg?style=flat-square)](https://opensource.org/licenses/MIT)
[![Rust 2021](https://img.shields.io/badge/rust-2021-93450a.svg?style=flat-square)](https://www.rust-lang.org)
[![Tests](https://img.shields.io/badge/tests-175%20passing-brightgreen.svg?style=flat-square)](https://github.com/New1Direction/korg)

<p align="center">
  <a href="README.md">English</a> · <b>简体中文</b> · <a href="README.zh-TW.md">繁體中文</a>
</p>

---

![korg 演示 —— 实时回退、分叉、重放 AI 智能体的决策](demo.gif)

---

> AI 智能体是黑盒。出错时,你无法调试;成功时,你无法复现;做错事时,你无法撤销。
>
> **Korg 解决这个问题。**

---

## Korg 做什么

> [!NOTE]
> **通用接入模式:**
> korg v1 是一个可通过 MCP 调用的审计接收端。任何兼容 MCP 的编程智能体(Claude Code、Codex 等)都可以调用 korg 的工具,把自己的会话记录成一本按因果相连、可重放、可回退的账本。需要让该智能体记录自己的动作——通常通过系统提示词或 MCP 服务配置实现。无需智能体配合的完全被动审计已列入后续版本的路线图。

> [!WARNING]
> **信任边界与部署范围:**
> korg v1 严格面向本地、单用户的工作区。多租户与联网部署所需的加密认证与权限边界尚未提供。在不受信任或公开的网络上运行该服务,会暴露工作区的读写权限。

Korg 是一个**认知管理程序(cognitive hypervisor)**——一个位于你的 AI 智能体之下、管控它们每一个决策的运行时层。

它不替代你的 LLM,而是管控 LLM 所做的事。

```
基础模型            →  预测、建议、生成
────────────────────────────────────────────────────────────
Korg 认知运行时     →  调度、校验、隔离、对账、重放、自愈、治理
```

每一个智能体动作都会:
- **追加**到一本不可变、带密码学签名的账本
- 用混合逻辑时钟(HLC)**排序**(因果、确定、全局一致)
- **可重放**——在历史上任意一点重建出精确的状态
- **可逆**——回退、分叉,或为任意决策开一条新分支

---

## 试试时间旅行演示

运行内置的沙箱演示,亲眼看看“认知时间旅行”:它会建一个临时工作区(含一个有 bug 的 Python 脚本),让一个模拟的编程智能体做出错误改动,捕获测试失败,把工作区和账本回退到改动之前,再推测性地提交正确的修复:

```bash
cargo run -- demo
```

你会看到完整的彩色时间旅行过程(完整输出见[英文 README](README.md)):错误路径 → 回退到 seq 391 → 沿正确路径重新推进 → 测试通过。

> *没有别的 AI 智能体运行时能做到这一点。*

---

## 核心架构

Korg 建立在那些让数据库与操作系统可靠的理论基础之上——这是首次把它们用于 AI 认知。

| 不变量 | 含义 |
|:---|:---|
| **仅追加 WAL** | 每个认知事件都是一条账本记录。只追加,绝不就地修改——像数据库的 WAL,但记录的是 AI 的“思考”。 |
| **HLC 因果排序** | 混合逻辑时钟保证全局一致、按因果排序的事件流——即使跨分布式集群工作者也成立。 |
| **确定性重放** | 任意一次行动都能从账本逐字节重放。相同输入,每次都得到相同输出。 |
| **推测性分支** | 把执行分叉成多条并行的假设路径,提交前先预览,可随意丢弃。 |
| **执行检查点** | 快照整个运行时状态(账本偏移、投影视图、租约表、工作区树),O(1) 还原。 |
| **微自愈** | 瞬时故障(锁冲突、过期状态)在副作用层自动修复,并留下完整的重试审计轨迹。 |
| **语义治理** | 集群动作以 BERT 嵌入的余弦相似度校验——靠语义对齐,而非关键字匹配。 |

---

## 快速开始

### 从源码构建

该 crate 暂未发布到 crates.io,请从源码安装:

```bash
git clone https://github.com/New1Direction/korg
cd korg
cargo build --release
./target/release/korg-tui --help
```

> 在中国大陆?用 cargo 国内镜像更快——见 korgex 文档的[在中国安装](https://korgex-docs.pages.dev/zh-CN/docs/install-china)(rsproxy.cn / 清华)。

### Python 桥接(供 korgex / korgchat 使用)

```bash
cd crates/korg-bridge
maturin develop  # 把 PyO3 扩展构建进当前 venv
python3 -c "import korg_bridge; print(korg_bridge.__version__)"
```

### 跑你的第一个 campaign

```bash
# 交互式 TUI 仪表盘
korg campaign --tui --prompt "把鉴权层重构为使用 JWT"

# localhost:8080 的 Web 驾驶舱
korg campaign --web --prompt "优化数据库连接池"

# 纯自主目标模式
korg goal "为 src/parser.rs 编写并验证一整套测试"

# 预览但不提交(推测性沙箱)
korg run --preview "重构主事件循环"
```

### 回退与分叉

```bash
korg rewind --seq 4                       # 回退到指定的账本序号
korg checkpoints list                      # 列出当前会话的所有检查点
korg checkpoints restore --id <uuid>       # 从指定检查点还原
```

---

## 认知模式

Korg 会按任务复杂度切换智能层级。模式只通过能力解析器(capability resolver)切换,每次切换都会记进账本。

| 模式 | 适用场景 |
|:---|:---|
| `instant` | 极低延迟。跳过协商,乐观执行。 |
| `balanced` | 默认。结构化的多轮合约协商。 |
| `heavy` | 深度多智能体研议,多轮评估。 |
| `research` | 广度发散探索,跨所有 crate 扫描语义索引。 |
| `recovery` | 安全回滚模式,每次变更前先建检查点。 |
| `autonomous` | 完全目标模式,自我导航并自动重新规划。 |
| `heavy-consciousness` | 最大深度,注入完整的 HeavyConsciousness 上下文。 |

---

## 为什么需要 Korg

如今的 AI 编程智能体是概率黑盒。它们:
- **无法重放**——同样的提示词,每次输出都不同
- **无法回退**——一个错误动作,你就得手动去 diff git 历史
- **无法审计**——没有任何关于智能体决定了什么、为什么这么决定的记录
- **无法治理**——没有办法在运行时设定策略边界

Korg 对待 AI 认知的方式,就像管理程序对待算力、Git 对待代码:

> **不在账本里,就等于没发生过。**

---

## 对比

| 能力 | Korg | LangChain / LangGraph | CrewAI | 普通 CLI 智能体 |
|:---|:---:|:---:|:---:|:---:|
| 确定性重放 | ✅ | ❌ | ❌ | ❌ |
| HLC 因果排序 | ✅ | ❌ | ❌ | ❌ |
| 回退执行 | ✅ | ❌ | ❌ | ❌ |
| 推测性分支 | ✅ | ❌ | ❌ | ❌ |
| 执行检查点 | ✅ | ❌ | ❌ | ❌ |
| 密码学审计轨迹 | ✅ | ❌ | ❌ | ❌ |
| 微自愈 | ✅ | ❌ | ❌ | ❌ |
| 不挑模型 | ✅ | ✅ | ✅ | ✅ |

> **Korg 不是一个智能体框架,而是跑在所有框架之下的治理内核。**

---

## 技术栈

| 组件 | 技术 |
|:---|:---|
| 核心运行时 | Rust 2021、Tokio 异步 |
| 账本排序 | 混合逻辑时钟(HLC) |
| 工作区快照 | Git Merkle 树(`write-tree`/`read-tree`,O(1) 还原) |
| 密码学证明 | Ed25519(ed25519-dalek) |
| 语义治理 | BERT 余弦相似度(Candle / Hugging Face) |
| TUI 仪表盘 | Ratatui + Crossterm |
| Web 驾驶舱 | Axum + SSE |
| 语法高亮 | Syntect + tree-sitter |

---

## 状态

Korg 正在积极开发中。当前测试覆盖:**175 个测试,0 失败**(8 个 crate 共 162 个 cargo 测试 + PyO3 桥接的 13 个 pytest)。完整的功能清单与路线图见[英文 README](README.md)。

---

## 许可证

在 [MIT](LICENSE-MIT) 或 [Apache-2.0](LICENSE-APACHE) 中任选其一。

---

<p align="center">
  <sub>用 Rust 构建。由不变量治理。没有黑盒。</sub>
</p>
