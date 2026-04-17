# 项目问题分析与多轮对话 Review 设计

## 一、项目当前主要问题

### 1. LLM 调用层强耦合
当前实现把 Review 能力强绑定在 `copilot.rs` 上，核心调用路径依赖 `copilot::run_review()`，本质上是通过 GitHub Copilot CLI 发起请求。

这会带来几个问题：
- 很难替换模型或 provider
- 无法自然支持 OpenAI / Claude / Ollama / 自建网关
- API 层、服务层、Prompt 层都被底层执行方式反向影响

虽然设计文档里已经有 Provider 抽象方向，但代码里还没真正落地。

---

### 2. 现在的“两阶段 review”不是真正的多轮对话
项目里已经有 `execute_deep_review()`，看起来像 stage1 + stage2。

但本质上：
- stage1 先跑一次模型
- 再从 stage1 输出里用正则抽文件名、函数名、风险提示
- 然后拼一个新的 prompt 给 stage2

也就是说，**stage2 并不是基于完整对话历史继续推理**，而只是消费了一份“阶段一摘要”。

这不是严格意义上的多轮对话，而是：
- 单轮分析
- 单轮追问
- 中间靠文本摘要桥接

问题在于：
- 上下文容易丢失
- stage1 中的隐含推理无法传递到 stage2
- stage2 的理解质量完全取决于 `extract_stage2_focus()` 的提取质量

---

### 3. 输出解析过于脆弱
`review_parser.rs` 主要靠标题和关键词来切分模型输出，比如：
- 高风险问题
- 中风险问题
- 缺失的测试场景
- 总结结论

这种解析方式的问题很明显：
- 模型换个说法就可能解析失败
- 中英文混排时稳定性一般
- 结构变化时缺少强约束
- 解析失败后容易出现“部分字段为空但流程仍继续”的情况

更直接地说，现在更像是“尽量猜测模型输出”，不是“消费稳定协议”。

---

### 4. API 错误分类依赖字符串 contains
`api.rs` 里根据错误文本内容决定 HTTP 状态码，比如：
- 包含 `not authenticated`
- 包含 `git diff is empty`
- 包含 `blocked`

这种方式开发初期很快，但长期会出问题：
- 错误文本一改就失效
- 不同 provider / 不同平台的错误文案不一致
- 很难形成稳定可测试的错误协议

---

### 5. 服务层职责过重
`review_service.rs` 里集中处理了很多事情：
- admission 检查
- prompt 参数补全
- diff/context 读取
- 模型调用
- review 解析
- repair 重试
- 深度 review 编排

这个文件已经偏“总装配车间”了。问题不是功能做不了，而是后面越来越难维护：
- 测试切面不清楚
- 扩展策略困难
- 不同 review 模式会继续堆逻辑

---

### 6. 没有真正的对话会话模型
当前的 `SessionStore` 主要服务于认证信息存储，不是用于保存审查会话。

项目里缺少这些关键能力：
- 会话 ID
- 多轮 message 历史
- 每轮补充的上下文文件
- 某轮追问针对哪些风险点
- 最终结果是在哪轮收敛出来的

所以现在系统天然偏“请求-响应式”，不是“会话式”。

---

### 7. 若干实现细节上的隐患

#### `extract_stage2_focus()` 噪音偏大
目前依赖正则从 stage1 文本里抽：
- 文件名
- 函数名
- 不确定/高风险语句

这种方式很容易抽出噪音标识符，造成 stage2 的 focus 不够准。

#### repair 流程和主流程耦合太紧
当前 repair 是在结构校验失败后再次发起模型调用。思路没问题，但缺少：
- 统一的 retry/backoff 策略
- provider 级别超时控制
- repair 的失败分级处理

#### 一些阈值是硬编码
例如自动扩上下文文件数量、某些阶段行为限制，都写死在逻辑里。后面做不同仓库适配时会比较痛苦。

---

## 二、什么才叫“多轮对话式 Code Review”

真正的多轮对话 review，不是简单的“先看一遍，再看一遍”。

它应该满足：
- 模型能看到前面所有轮次的内容
- 每轮结论都能成为后续推理的一部分
- 追问是围绕前面发现的问题展开的
- 每一轮允许补充新的代码上下文和业务信息
- 最终输出不是孤立生成，而是逐步收敛

可以把它理解成：

> 第一轮做广泛扫描，第二轮做重点核查，第三轮做证据对齐，第四轮输出最终结论。

而不是每一轮都重新从零开始。

---

## 三、推荐的多轮对话架构

### 1. 核心对象：ConversationSession
建议新增独立的会话模型，而不是继续把状态塞进现有执行结构里。

```rust
pub struct ConversationSession {
    pub id: String,
    pub review_mode: ReviewMode,
    pub model: String,
    pub repo_root: PathBuf,
    pub created_at: String,
    pub updated_at: String,
    pub messages: Vec<ChatMessage>,
    pub attachments: Vec<SessionAttachment>,
    pub state: ReviewConversationState,
}

pub struct ChatMessage {
    pub role: MessageRole,   // system / user / assistant / tool
    pub content: String,
    pub turn: u32,
    pub created_at: String,
}
```

这里的重点是 `messages`。

后续每一轮都不是重新造 prompt，而是：
- 取出 session 历史
- 加入本轮输入
- 发送完整 message 列表给 provider

---

### 2. Provider 层改成 Chat 接口
当前能力更像：

```rust
fn run_review(prompt: &str) -> Result<String>
```

建议升级成：

```rust
trait LlmProvider {
    fn chat(&self, req: ChatRequest) -> Result<ChatResponse>;
}

pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

pub struct ChatResponse {
    pub content: String,
    pub raw: Option<String>,
    pub usage: Option<TokenUsage>,
}
```

这样可以同时支持：
- Copilot CLI provider
- OpenAI compatible provider
- Claude provider
- Ollama provider

也为以后做 streaming、tool calling、结构化输出打底。

---

### 3. 多轮 review 的标准流程
建议把 review 拆成 4 类 turn：

#### Turn 1：全局扫描
输入：
- system prompt
- diff
- changed files
- 初始上下文文件
- review mode / rules / risk hints

输出目标：
- 找出高风险点
- 列出不确定点
- 给出建议补充的上下文文件
- 给出下一轮应重点核查的对象

#### Turn 2：重点深挖
输入：
- Turn 1 历史
- 补充的文件内容
- 需要验证的高风险点

输出目标：
- 判断哪些风险是真问题
- 哪些只是表面告警
- 明确跨文件联动影响

#### Turn 3：业务与发布校验
输入：
- 前两轮历史
- 业务规则/Jira/测试结果/发布约束

输出目标：
- 校验业务影响面
- 检查兼容性、回滚、数据迁移、灰度风险
- 收敛到发布建议

#### Turn 4：最终报告生成
输入：
- 全量历史
- 结构化输出约束

输出目标：
- 输出稳定格式的 review report
- 适合 CLI / API / Web UI 展示

---

## 四、建议的数据结构

### 1. 会话状态
```rust
pub struct ReviewConversationState {
    pub status: ConversationStatus,
    pub current_turn: u32,
    pub findings: Vec<TrackedFinding>,
    pub requested_files: Vec<String>,
    pub confirmed_files: Vec<String>,
    pub final_report: Option<ReviewResult>,
}
```

### 2. 跟踪问题项
```rust
pub struct TrackedFinding {
    pub id: String,
    pub severity: String,
    pub title: String,
    pub evidence: Vec<String>,
    pub related_files: Vec<String>,
    pub status: FindingStatus, // suspected / confirmed / dismissed
}
```

这个结构很重要。

因为多轮 review 的核心不是“保留聊天记录”本身，而是**让问题项在多轮里被追踪、确认、驳回、收敛**。

---

## 五、API 设计建议

### 1. 新增会话式接口
建议不要只保留一次性 `/review`，而是增加会话接口。

#### 创建会话
`POST /api/review-sessions`

返回：
- session_id
- 第一轮输出
- 建议补充文件
- 当前状态

#### 继续下一轮
`POST /api/review-sessions/{id}/turns`

请求中可以包含：
- 本轮补充说明
- 补充文件
- 指定重点核查项
- 是否要求生成最终报告

#### 获取会话状态
`GET /api/review-sessions/{id}`

返回：
- 已有消息
- 当前 findings
- 状态
- 最终报告（如果已完成）

---

## 六、落地时的模块拆分建议

### 推荐新增模块

#### `conversation.rs`
定义：
- `ConversationSession`
- `ChatMessage`
- `ReviewConversationState`
- `TrackedFinding`

#### `providers/mod.rs`
定义：
- `LlmProvider` trait
- provider registry
- provider config

#### `providers/copilot.rs`
把当前 `copilot.rs` 迁进去，变成 provider 实现之一。

#### `conversation_store.rs`
负责：
- 保存会话
- 读取会话
- append 消息
- 更新状态

#### `orchestrator.rs`
负责：
- turn 编排
- 判断是否继续下一轮
- 根据 findings 请求额外上下文
- 最终汇总为 report

现在 `review_service.rs` 里太多东西都可以拆到这里。

---

## 七、输出协议建议：从“文本解析”升级到“结构化优先”

当前 parser 最大的问题是依赖自由文本。

建议改成两层策略：

### 第一层：优先要求 JSON 输出
比如要求模型输出：

```json
{
  "high_risk": [],
  "medium_risk": [],
  "low_risk": [],
  "missing_tests": [],
  "impact_scope": [],
  "release_checks": [],
  "summary": ""
}
```

### 第二层：失败时退回文本 repair
如果 provider 不稳定，或者输出了脏 JSON，再用 repair prompt 兜底。

这样做的好处：
- parser 简化很多
- API 稳定性明显提升
- 前端渲染也更容易

---

## 八、推荐的演进路线

### Phase 1：先抽 provider
先把 `copilot.rs` 从业务逻辑里抽出来，形成统一接口。

目标：
- review_service 不再直接依赖具体 CLI
- 能支持 chat(messages)

### Phase 2：引入 ConversationSession
先把 stage1 / stage2 接到统一 session 上。

目标：
- 两阶段共享 message 历史
- 每轮可追加上下文

### Phase 3：引入 findings 跟踪
不要只传“阶段摘要”，而是把问题项做成结构化状态。

目标：
- 哪些问题待确认
- 哪些问题被排除
- 哪些问题升级为高风险

### Phase 4：改 API 为会话式
给 CLI / Web UI / 第三方调用提供统一会话能力。

### Phase 5：结构化输出优先
把文本 parser 从主路径降级为 fallback。

---

## 九、一句话结论
这个项目现在的基础并不差，编译也正常，说明框架已经能跑起来。

但它现在更像：

> 一个“有两阶段能力的单次 Review 工具”

还不是：

> 一个真正有会话状态、有上下文继承、有问题跟踪能力的多轮 Code Review 系统

要把它做成真正的多轮对话 review，最关键的不是再补几个 prompt，而是补三件事：

1. **会话模型**：保存完整 messages 历史  
2. **Provider 抽象**：让模型调用具备 chat 能力  
3. **问题跟踪状态**：让多轮过程围绕 findings 收敛  

只要这三块搭起来，后面的 stage3、人工复核、Web UI、Jira 联动都会顺很多。
