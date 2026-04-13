# AI Code Review Implementation Plan

> 基于 `require2.md` 的实施计划。目标是把当前 CLI 工具逐步改造成一个具备准入控制、分层审查、结构化输出、模式差异化和后续 Web 演进能力的可落地系统。

---

## 1. 实施目标

本计划解决五个核心问题：

1. **输入不够硬**  
   把当前“打分 + 建议补充”升级为“准入检查 + 阻断 / 降级”。

2. **四层能力只有 prompt，没有系统闭环**  
   抽象基础层、工程层、业务层、风险层，让系统内部知道自己在审什么。

3. **输出是自由文本，不稳定**  
   定义结构化 review schema，并把模型输出解析、校验、必要时修复。

4. **三种模式差异太弱**  
   让 lite / standard / critical 在准入规则、流程、输出上都真正不同。

5. **当前只能 CLI 本地跑**  
   在不破坏 CLI 的前提下拆出 service 层，为 API / Web 做准备。

---

## 2. 实施原则

- **先闭环，再扩展**：先把 CLI 的输入、分析、输出闭环做实，再谈 Web。
- **先约束，再增强**：先让系统拒绝坏输入、约束坏输出，再继续补能力。
- **最小可落地**：先做能验收的基础版本，不一开始追求复杂规则引擎。
- **兼容现有命令**：尽量不破坏现有 `prompt / run / deep-review / review / validate` 使用方式。
- **JSON 优先**：新增能力优先提供稳定 JSON 输出，文本只是展示层。

---

## 3. 当前代码基线与影响面

### 3.1 当前核心模块
- `src/cli.rs`：命令定义与参数结构
- `src/lib.rs`：主流程调度
- `src/prompt.rs`：prompt 构建与当前 validate 逻辑
- `src/jira.rs`：Jira enrich
- `src/context.rs`：上下文读取与预算控制
- `src/expand.rs`：关联文件自动扩展
- `src/copilot.rs`：调用本地 copilot CLI
- `src/config.rs`：配置读取

### 3.2 改造影响最大的文件
- `src/cli.rs`
- `src/lib.rs`
- `src/prompt.rs`
- `src/config.rs`

### 3.3 建议新增模块
- `src/admission.rs`
- `src/review_schema.rs`
- `src/review_parser.rs`
- `src/review_layers.rs`
- `src/risk.rs`
- `src/services/mod.rs`
- `src/services/review_service.rs`

---

## 4. 总体分期

### Phase 1：CLI 核心闭环增强（必须先完成）
目标：让系统在 CLI 层面已经是“可控的 review 系统”，而不只是 prompt 工具。

交付内容：
- 准入检查
- 模式差异化
- 四层审查抽象
- 结构化输出 schema
- 结果解析与校验
- CLI JSON 输出统一化

### Phase 2：标准化流程增强（第二阶段）
目标：让 standard / critical 更像真实团队流程。

交付内容：
- critical 知识挂载
- risk analyzer 雏形
- CI 友好输出
- 人工复核标记

### Phase 3：服务化与网页化（第三阶段）
目标：支持 API 和网页访问。

交付内容：
- service 层
- HTTP API
- 任务模型
- 基础 Web UI

---

## 5. Phase 1 详细实施计划

---

### Task 1：建立准入检查模块 `admission.rs`

#### 目标
把当前 `validate_args()` 的打分建议机制，升级为统一的准入检查系统。

#### 需要解决的问题
- standard 模式缺 P0 还能跑，这是不符合要求的
- review / run / deep-review 各自逻辑不统一
- 当前 validate 只是评分，不是执行门禁

#### 实现内容
新增：`src/admission.rs`

定义核心结构：
- `AdmissionLevel`：`Pass / Warn / Block`
- `AdmissionCheckResult`
- `MissingContext`
- `DegradeReason`
- `ReviewConfidence`

定义核心函数：
- `check_admission(args, has_diff, has_context, has_p2) -> AdmissionCheckResult`
- `count_p1_coverage(...)`
- `detect_p2_support(...)`

#### 规则落地
- **Lite**：diff 必需；goal/rules 缺失不阻断，但标记 `low confidence`
- **Standard**：缺任何 P0 直接 block
- **Critical**：缺 P0 block；P1 少于 2 项 block；无 P2 block

#### CLI 接入点
- `validate`
- `review`
- `run`
- `deep-review`

这些命令全部统一调用 `check_admission()`。

#### 验收标准
- standard 模式缺 goal 时退出失败
- standard 模式缺 rules 时退出失败
- critical 模式缺 baseline/focus/incident 时退出失败
- lite 模式允许继续，但返回 `confidence=low`

#### 风险
- 现有用户使用习惯可能被“阻断”影响
- 需要定义一个清晰的错误提示文案，避免“怎么突然不能用了”

---

### Task 2：抽象四层审查定义 `review_layers.rs`

#### 目标
让系统内部显式知道：每次 review 包含哪些层、每层关心什么。

#### 需要解决的问题
- 现在只是 prompt 里提几句，没有内部结构
- 后续 risk/business 增强没地方挂

#### 实现内容
新增：`src/review_layers.rs`

定义：
- `ReviewLayer`：`Basic / Engineering / Business / Risk`
- `LayerChecklist`
- `LayerContextRequirements`
- `LayerPromptHints`

核心函数：
- `build_review_layers(args) -> Vec<LayerChecklist>`
- `collect_layer_requirements(args) -> LayerContextRequirements`
- `render_layer_prompt_section(...) -> String`

#### 最低落地方式
先不要搞复杂规则引擎，先做到：
- 每层有明确检查项列表
- prompt 按层生成 section
- 结果 schema 可记录 layer 来源

#### 验收标准
- prompt 中四层检查项不再是硬编码大段文本，而是来自 layer 定义
- 不同 change type / mode 下，可动态增加某层关注项

#### 风险
- 如果设计得太重，会拖慢落地
- 所以第一版只做“结构抽象 + prompt 装配”即可

---

### Task 3：定义结构化输出 Schema `review_schema.rs`

#### 目标
把 review 结果从“纯文本”升级为“结构化对象 + 文本展示”。

#### 实现内容
新增：`src/review_schema.rs`

定义核心结构：
- `ReviewResult`
- `RiskItem`
- `MissingTestCase`
- `ImpactItem`
- `HumanCheckItem`
- `ReviewMeta`

建议字段：
- `mode`
- `input_ok`
- `input_score`
- `confidence`
- `used_rules`
- `high_risk`
- `medium_risk`
- `low_risk`
- `missing_tests`
- `summary`
- `needs_human_review`
- `impact_scope`
- `release_checks`
- `raw_text`

#### 关键设计要求
- `RiskItem` 必须包含：
  - `title`
  - `file`
  - `location`
  - `reason`
  - `trigger`
  - `impact`
  - `suggestion`
- critical 模式额外要求：
  - `impact_scope`
  - `release_checks`

#### 验收标准
- review 结果能够稳定序列化为 JSON
- text 输出只是 schema 的渲染结果，而不是直接打印原始模型输出

---

### Task 4：实现结果解析器 `review_parser.rs`

#### 目标
把模型返回文本解析成 `ReviewResult`。

#### 需要解决的问题
- 模型可能不按格式输出
- 即便分了 1~5 段，也不一定字段齐全
- 没有 parser 就无法让输出真正结构化

#### 实现内容
新增：`src/review_parser.rs`

分两层：

##### 第一层：宽松分段解析
识别这些 section：
- 高风险问题
- 中风险问题
- 低风险优化建议
- 缺失的测试场景
- 总结结论

可兼容：
- `1.` / `一、` / `##` / `###`
- `High Risk` / `高风险`

##### 第二层：问题项字段抽取
每条风险项尝试抽取：
- 文件
- 函数/位置
- 原因
- 触发条件
- 影响
- 修复建议

##### 第三层：修复机制
如果解析失败：
- 触发一次格式修复 prompt
- 要求模型重新输出 JSON 或更规范文本

##### 第四层：校验机制
- 缺 summary -> 失败
- 缺五大 section -> 失败或 warning
- high_risk 项缺关键字段 -> warning / critical 下失败

#### 验收标准
- 典型文本输出可解析为 JSON
- 故意打乱格式时能识别失败并报错
- critical 模式不允许“只有一段散文式总结”直接通过

#### 风险
- 文本 parser 一开始容易脆
- 建议第一版允许“部分解析 + 警告”，先别追求 100% 严格

---

### Task 5：重构 prompt 构建逻辑

#### 目标
让 prompt 生成从“拼大字符串”升级为“由 admission + layers + schema 共同驱动”。

#### 需要改的文件
- `src/prompt.rs`

#### 重构方向
当前 `build_prompt_from_sources()` 里有很多硬编码文本。需要改成：
- 输入概览 section
- Admission / confidence section
- Layer-based checklist section
- Output schema section
- Diff / context section

#### 实现要求
- 把“四层检查要求”从硬编码迁出到 `review_layers.rs`
- 把“输出格式要求”改成更明确的 schema 指导
- critical 模式额外注入 impact / release check 要求

#### 验收标准
- `build_prompt_from_sources()` 明显瘦身
- 不同 mode 的 prompt 差异来自结构配置，不靠 if/else 拼命堆文本

---

### Task 6：统一 review 执行主流程

#### 目标
让 `review / run / deep-review / validate` 共享一套主流程，而不是各自拼。

#### 需要改的文件
- `src/lib.rs`
- 新增 `src/services/review_service.rs`

#### 抽象建议
定义 service 层流程：
1. 收集输入
2. enrich Jira / issue
3. 扩展 context
4. admission check
5. build prompt
6. run model
7. parse result
8. validate result
9. render text/json

#### 推荐接口
- `prepare_review_input(...)`
- `execute_review(...)`
- `execute_deep_review(...)`
- `render_review_output(...)`

#### 验收标准
- `run/review/deep-review` 最终都能复用同一套输出结构
- `validate` 与执行命令使用同一 admission 逻辑

---

### Task 7：CLI 参数与输出扩展

#### 目标
补齐结构化输出和 critical 增强所需的参数。

#### 需要改的文件
- `src/cli.rs`

#### 新增建议参数
在 `PromptArgs` 中增加：
- `--incident-file`
- `--redline-file`
- `--release-note`
- `--rollback-note`
- `--output text|json`

如不想太快加太多，也可以先统一复用：
- `baseline_files`
- `focus`

但建议至少补一个更明确的 incident 输入。

#### 新增建议命令
可选新增：
- `admission-check`
- `review-json`

如果不新增命令，也至少保证：
- 现有命令统一支持 `--format json`

#### 验收标准
- 用户能通过 CLI 显式输入 critical 所需增强信息
- review 执行结果可直接输出 JSON

---

### Task 8：critical 模式知识挂载最小实现

#### 目标
让 critical 模式真的比 standard 多出内容。

#### 实现方式
先做最小版：
- 允许挂 `baseline_files`
- 允许挂 `incident_file`
- 自动加入 `focus`
- risk analyzer 根据 `change_type` 增加检查项

#### 输出增强
critical 模式必须额外输出：
- `impact_scope`
- `release_checks`
- `needs_human_review = true`

#### 验收标准
- critical 模式不只是 prompt 文字更重，而是输出字段真的更多
- 无 baseline/incident/focus 时直接 block

---

## 6. Phase 2 详细实施计划

---

### Task 9：实现 `risk.rs` 风险分析器雏形

#### 目标
为风险层提供程序级辅助，而不是全靠模型自由发挥。

#### 第一版能力
基于以下信息生成风险提示：
- 文件后缀
- 目录名
- `change_type`
- diff 中关键词

#### 示例规则
- 命中 `.sql` / `migration` -> DB migration risk
- 命中 `api` / `dto` / `proto` -> 契约变更风险
- 命中 `auth` / `permission` -> 权限风险
- 命中 `handler + service + model` 跨层大改 -> 影响面扩大

#### 输出
- `Vec<ImpactItem>`
- `Vec<String>` 风险提示

---

### Task 10：增强 Jira / Issue / PR enrich

#### 目标
补足 standard / critical 在上下文完整性上的真实能力。

#### 范围
- 更稳定提取 acceptance
- 提取 linked issue title
- 提取 comments 中测试/风险关键词
- 为 future PR 接口预留结构

#### 产出
- 更强的 `business context pack`
- 更稳的 `expected behavior` 自动补全

---

### Task 11：CI 友好输出

#### 目标
让 standard 模式结果可以直接被 CI 消费。

#### 实现内容
- JSON 输出稳定字段
- 退出码约定：
  - `0`：无高风险或仅低风险
  - `2`：有高风险 / 需要人工复核
  - `3`：输入不满足准入
  - `4`：模型输出格式失败

#### 验收标准
- CI 能基于 exit code 和 JSON 判定是否拦截

---

## 7. Phase 3 详细实施计划

---

### Task 12：抽出 service 层

#### 目标
让 CLI 和后续 HTTP API 共用同一套核心逻辑。

#### 推荐结构
- `src/services/mod.rs`
- `src/services/review_service.rs`
- `src/services/prompt_service.rs`

#### 要求
`lib.rs` 只做 CLI dispatch，不再直接承载复杂业务流程。

---

### Task 13：HTTP API 初版

#### 目标
让系统可被网页访问和脚本调用。

#### 技术建议
- 新增依赖：`axum`, `tokio`, `tower-http`

#### 初版接口
- `POST /api/validate`
- `POST /api/run`
- `POST /api/deep-review`
- `GET /api/models`

#### 第一版要求
- 可以同步返回结果
- 如果 deep-review 太慢，后面再升级成 task 模型

---

### Task 14：任务模型与基础 Web 页面

#### 目标
让网页使用体验不至于卡死。

#### 任务模型
- `pending`
- `running`
- `done`
- `failed`

#### 页面最小需求
- 提交 review 表单
- 展示 review 结果
- 展示任务状态

---

## 8. 推荐开发顺序

按最稳的顺序，我建议这样做：

### 第 1 批
1. `admission.rs`
2. `review_schema.rs`
3. `review_layers.rs`
4. `prompt.rs` 重构接入 layers

### 第 2 批
5. `review_parser.rs`
6. `review_service.rs`
7. `lib.rs` 流程统一
8. `cli.rs` 参数补充

### 第 3 批
9. `risk.rs`
10. critical 知识挂载
11. CI 输出与退出码

### 第 4 批
12. service 层整理
13. HTTP API
14. Web 页面

---

## 9. 里程碑与交付物

### Milestone A：输入闭环完成
交付物：
- `admission.rs`
- `validate` / `review` / `run` / `deep-review` 统一准入
- 新增相关测试

完成标志：
- standard / critical 不再接受不完整 P0 输入

### Milestone B：输出闭环完成
交付物：
- `review_schema.rs`
- `review_parser.rs`
- JSON 输出

完成标志：
- review 结果能稳定输出结构化 JSON

### Milestone C：模式差异化完成
交付物：
- `review_layers.rs`
- critical 增强上下文
- `risk.rs` 雏形

完成标志：
- lite / standard / critical 不再只是 prompt 长度不同

### Milestone D：服务化基础完成
交付物：
- `services/`
- API 初版

完成标志：
- 核心逻辑可通过 HTTP 调用

---

## 10. 测试计划

### 10.1 单元测试
需要为以下模块补测试：
- `admission.rs`
- `review_schema.rs`
- `review_parser.rs`
- `review_layers.rs`
- `risk.rs`

### 10.2 集成测试
建议增加：
- lite / standard / critical 三种模式准入测试
- review 结果解析测试
- critical 缺 P2 阻断测试
- run/deep-review 共享输出格式测试

### 10.3 回归测试
保留当前已有测试：
- context budget
- expand related files
- jira enrich
- session/token 安全

---

## 11. 技术风险与应对

### 风险 1：文本解析不稳定
应对：
- 第一版先宽松解析
- 保留 raw_text
- 失败时支持一次 repair 流程

### 风险 2：改动面过大导致 CLI 混乱
应对：
- 先抽 service 层
- 每个 phase 控制文件变更范围

### 风险 3：critical 要求太严，影响可用性
应对：
- 提供明确 block reason
- 提供如何补齐上下文的指导

### 风险 4：工程规则做太重
应对：
- 第一版只做轻规则 + prompt 协同
- 不做复杂静态分析引擎

---

## 12. 建议第一轮 Sprint 内容

如果现在马上开始干，我建议第一轮只做这些：

### Sprint 1
- 新增 `admission.rs`
- 用 admission 替换现有 `validate_args()` 的核心准入逻辑
- 让 `validate/run/review/deep-review` 共享 admission
- 新增相关单元测试

### Sprint 2
- 新增 `review_schema.rs`
- 定义 JSON 输出结构
- 先让 review 结果能包装成 schema，即使 parser 还比较简单

### Sprint 3
- 新增 `review_parser.rs`
- 把模型文本转为结构化结果
- 接入 `review` 和 `deep-review`

### Sprint 4
- 新增 `review_layers.rs`
- 重构 `prompt.rs`
- 补 critical 模式增强字段

---

## 13. 一句话执行建议

最值得先做的不是 Web，也不是更花哨的 prompt，而是三件事：

1. **把准入卡死**
2. **把输出结构化**
3. **把模式差异做真**

这三件做完，这个项目才算真的从“AI 提示工具”升级成“AI Review 系统”。
