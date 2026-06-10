# korg

**一個為自主 AI 代理打造的、按因果排序、可回退的事件帳本。**
*你的 AI 代理走的每一步,都記進一本可獨立驗證的雜湊鏈帳本——防竄改、零信任、不用區塊鏈。*

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg?style=flat-square)](https://opensource.org/licenses/MIT)
[![Rust 2021](https://img.shields.io/badge/rust-2021-93450a.svg?style=flat-square)](https://www.rust-lang.org)
[![Tests](https://img.shields.io/badge/tests-175%20passing-brightgreen.svg?style=flat-square)](https://github.com/New1Direction/korg)

<p align="center">
  <a href="README.md">English</a> · <a href="README.zh-CN.md">简体中文</a> · <b>繁體中文</b>
</p>

---

![korg 示範 —— 即時回退、分叉、重放 AI 代理的決策](demo.gif)

---

> AI 代理是黑盒。出錯時,你無法除錯;成功時,你無法重現;做錯事時,你無法復原。
>
> **Korg 解決這個問題。**

---

## Korg 做什麼

> [!NOTE]
> **通用接入模式:**
> korg v1 是一個可透過 MCP 呼叫的稽核接收端。任何相容 MCP 的編程代理(Claude Code、Codex 等)都可以呼叫 korg 的工具,把自己的工作階段記錄成一本按因果相連、可重放、可回退的帳本。需要讓該代理記錄自己的動作——通常透過系統提示詞或 MCP 伺服器設定實現。無需代理配合的完全被動稽核已列入後續版本的路線圖。

> [!WARNING]
> **信任邊界與部署範圍:**
> korg v1 嚴格面向本機、單一使用者的工作區。多租戶與聯網部署所需的加密認證與權限邊界尚未提供。在不受信任或公開的網路上執行該服務,會暴露工作區的讀寫權限。

Korg 是一個**認知管理程式(cognitive hypervisor)**——一個位於你的 AI 代理之下、管控它們每一個決策的執行時層。

它不取代你的 LLM,而是管控 LLM 所做的事。

```
基礎模型            →  預測、建議、生成
────────────────────────────────────────────────────────────
Korg 認知執行時     →  排程、校驗、隔離、對帳、重放、自癒、治理
```

每一個代理動作都會:
- **追加**到一本不可變、帶密碼學簽章的帳本
- 用混合邏輯時鐘(HLC)**排序**(因果、確定、全域一致)
- **可重放**——在歷史上任意一點重建出精確的狀態
- **可逆**——回退、分叉,或為任意決策開一條新分支

---

## 試試時間旅行示範

執行內建的沙箱示範,親眼看看「認知時間旅行」:它會建一個臨時工作區(含一個有 bug 的 Python 指令稿),讓一個模擬的編程代理做出錯誤改動,捕捉測試失敗,把工作區和帳本回退到改動之前,再推測性地提交正確的修復:

```bash
cargo run -- demo
```

你會看到完整的彩色時間旅行過程(完整輸出見[英文 README](README.md)):錯誤路徑 → 回退到 seq 391 → 沿正確路徑重新推進 → 測試通過。

> *沒有別的 AI 代理執行時能做到這一點。*

---

## 核心架構

Korg 建立在那些讓資料庫與作業系統可靠的理論基礎之上——這是首次把它們用於 AI 認知。

| 不變量 | 含義 |
|:---|:---|
| **僅追加 WAL** | 每個認知事件都是一條帳本紀錄。只追加,絕不就地修改——像資料庫的 WAL,但記錄的是 AI 的「思考」。 |
| **HLC 因果排序** | 混合邏輯時鐘保證全域一致、按因果排序的事件流——即使跨分散式叢集工作者也成立。 |
| **確定性重放** | 任意一次行動都能從帳本逐位元組重放。相同輸入,每次都得到相同輸出。 |
| **推測性分支** | 把執行分叉成多條並行的假設路徑,提交前先預覽,可隨意丟棄。 |
| **執行檢查點** | 快照整個執行時狀態(帳本偏移、投影檢視、租約表、工作區樹),O(1) 還原。 |
| **微自癒** | 瞬時故障(鎖衝突、過期狀態)在副作用層自動修復,並留下完整的重試稽核軌跡。 |
| **語義治理** | 叢集動作以 BERT 嵌入的餘弦相似度校驗——靠語義對齊,而非關鍵字比對。 |

---

## 快速開始

### 從原始碼建置

該 crate 暫未發佈到 crates.io,請從原始碼安裝:

```bash
git clone https://github.com/New1Direction/korg
cd korg
cargo build --release
./target/release/korg-tui --help
```

> 在中國大陸?用 cargo 國內鏡像更快——見 korgex 文件的[在中國安裝](https://korgex-docs.pages.dev/zh-TW/docs/install-china)(rsproxy.cn / 清華)。

### Python 橋接(供 korgex / korgchat 使用)

```bash
cd crates/korg-bridge
maturin develop  # 把 PyO3 擴充建置進當前 venv
python3 -c "import korg_bridge; print(korg_bridge.__version__)"
```

### 跑你的第一個 campaign

```bash
# 互動式 TUI 儀表板
korg campaign --tui --prompt "把驗證層重構為使用 JWT"

# localhost:8080 的 Web 駕駛艙
korg campaign --web --prompt "最佳化資料庫連線池"

# 純自主目標模式
korg goal "為 src/parser.rs 編寫並驗證一整套測試"

# 預覽但不提交(推測性沙箱)
korg run --preview "重構主事件迴圈"
```

### 回退與分叉

```bash
korg rewind --seq 4                       # 回退到指定的帳本序號
korg checkpoints list                      # 列出當前工作階段的所有檢查點
korg checkpoints restore --id <uuid>       # 從指定檢查點還原
```

---

## 認知模式

Korg 會依任務複雜度切換智慧層級。模式只透過能力解析器(capability resolver)切換,每次切換都會記進帳本。

| 模式 | 適用場景 |
|:---|:---|
| `instant` | 極低延遲。跳過協商,樂觀執行。 |
| `balanced` | 預設。結構化的多輪合約協商。 |
| `heavy` | 深度多代理研議,多輪評估。 |
| `research` | 廣度發散探索,跨所有 crate 掃描語義索引。 |
| `recovery` | 安全回滾模式,每次變更前先建檢查點。 |
| `autonomous` | 完全目標模式,自我導航並自動重新規劃。 |
| `heavy-consciousness` | 最大深度,注入完整的 HeavyConsciousness 上下文。 |

---

## 為什麼需要 Korg

如今的 AI 編程代理是機率黑盒。它們:
- **無法重放**——同樣的提示詞,每次輸出都不同
- **無法回退**——一個錯誤動作,你就得手動去 diff git 歷史
- **無法稽核**——沒有任何關於代理決定了什麼、為什麼這麼決定的紀錄
- **無法治理**——沒有辦法在執行時設定策略邊界

Korg 對待 AI 認知的方式,就像管理程式對待算力、Git 對待程式碼:

> **不在帳本裡,就等於沒發生過。**

---

## 對比

| 能力 | Korg | LangChain / LangGraph | CrewAI | 一般 CLI 代理 |
|:---|:---:|:---:|:---:|:---:|
| 確定性重放 | ✅ | ❌ | ❌ | ❌ |
| HLC 因果排序 | ✅ | ❌ | ❌ | ❌ |
| 回退執行 | ✅ | ❌ | ❌ | ❌ |
| 推測性分支 | ✅ | ❌ | ❌ | ❌ |
| 執行檢查點 | ✅ | ❌ | ❌ | ❌ |
| 密碼學稽核軌跡 | ✅ | ❌ | ❌ | ❌ |
| 微自癒 | ✅ | ❌ | ❌ | ❌ |
| 不挑模型 | ✅ | ✅ | ✅ | ✅ |

> **Korg 不是一個代理框架,而是跑在所有框架之下的治理核心。**

---

## 技術堆疊

| 元件 | 技術 |
|:---|:---|
| 核心執行時 | Rust 2021、Tokio 非同步 |
| 帳本排序 | 混合邏輯時鐘(HLC) |
| 工作區快照 | Git Merkle 樹(`write-tree`/`read-tree`,O(1) 還原) |
| 密碼學證明 | Ed25519(ed25519-dalek) |
| 語義治理 | BERT 餘弦相似度(Candle / Hugging Face) |
| TUI 儀表板 | Ratatui + Crossterm |
| Web 駕駛艙 | Axum + SSE |
| 語法突顯 | Syntect + tree-sitter |

---

## 狀態

Korg 正在積極開發中。當前測試覆蓋:**175 個測試,0 失敗**(8 個 crate 共 162 個 cargo 測試 + PyO3 橋接的 13 個 pytest)。完整的功能清單與路線圖見[英文 README](README.md)。

---

## 授權

在 [MIT](LICENSE-MIT) 或 [Apache-2.0](LICENSE-APACHE) 中任選其一。

---

<p align="center">
  <sub>用 Rust 建置。由不變量治理。沒有黑盒。</sub>
</p>
