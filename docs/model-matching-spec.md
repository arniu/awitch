# 模型匹配与路由

> 系统**请求面**的形式化规格：模型匹配与路由问题的数学模型。
>
> - 术语定义处用全称（`CONTEXT.md` 词条名），首次出现标注、正文引用处用简称。
> - 未决项集中存放于 §5，正文以 `[TODO: #n]` 引用。

## 1. 问题定义

在请求端（agent）与服务端（provider）之间**动态匹配、确定接口**。三类要素：

- **硬约束**（不满足 = 不候选）：协议正确、请求负载可服务、服务可用、模型
  匹配（名字必须落在池中）、非优化维上限。
- **条件约束**：需求匹配（默认中性；显式需求收紧为硬）——见 §2.4
- **准则**（在可行域内权衡）：g = (cost, latency)——定义偏好（见 §2.5）

## 2. 数学模型

**模型类别**：约束多准则在线选择问题——约束优化（COP）的子类。三个限定词
对应模型三个特征：

- **硬约束**：不满足即 infeasible，不可放宽
- **多准则**：cost 与 latency 两个准则，加权和标量化（λ）
- **在线**：每请求独立决策，观测时变（见 §2.1）

近亲领域：QoS-aware service selection（Web 服务选择）。

**完整分类链**：

```
约束优化（COP）
└─ 在线多准则选择（Online Multi-criteria Selection）
```

### 2.1 输入

- 请求 r = (model 字符串, protocol, app, mode（routing mode）, previous_response_id,
  conversation, 请求体特征)
  - mode ∈ {eco, balanced, speed}——偏好参数，独立于词形
  - 请求体特征 = (tools?, stream?, 多模态?)——负载层的需求
- 池（model pool）P ⊆ Provider × Model（可用对集合；来自协议 models API 的
  选择落库）
- 快照 S = (健康, 余额/quota, 价格曲线, 延迟观测)——时变，每次请求读取
  当下的观测
- 续写（openai responses 的 `previous_response_id` / `conversation`）不是候选，
  是**直连**：不经 filter / match / rank。原生服务过的 id → 直连该 provider
  原生转发；翻译产生过的 id → 无可续形态，拒绝；未知 id 或 `conversation` →
  解析不出 provider，仅限原生，绝不翻译

### 2.2 决策变量

- x ∈ P：服务模型 (provider, model_id)

### 2.3 识别（语义层，每请求变化）

- D(r.model; K) → (intent, 约束谓词集 P_pred)——K = 匹配知识（池 + 需求注册
  表），识别依赖 K，**不是字符串的纯函数**
- intent ∈ {身份, 需求}，判定依赖 K：
  - 身份词形：池中**可精确匹配的模型 id**（`deepseek-chat`、`claude-sonnet-4-6`）
    ——依赖池知识，非词形形状
  - 需求词形：注册表中的逻辑名，携带需求谓词（协议锚定维度）
  - 未知词形：K 中无对应 → **识别失败（实例无效，§2.6）**
- 匹配知识是**显式存储数据**——无内置词形文法（无 tier 词、无 `claude-*`
  前缀解码、无 `[1M]`（1M context 后缀）等需求记号）
- 需求值域 = 协议锚定维度 [TODO: #2]

### 2.4 约束（硬 + 条件）

**硬约束**（不可放宽，失败 = infeasible）：

- **协议正确**：request.protocol 可被 provider 服务（原生或翻译对，v1:
  anthropic → openai chat）——通信层
- **请求负载可服务**：请求体的需求（tools → 工具调用、stream → 流式、
  多模态 → 视觉）⊆ 候选模型属性数据——服务层 [TODO: #3]
- **服务可用**：provider 健康、余额/quota 足够、模型存在于池、pin（routing pin）
  约束
- **续写直连**：`previous_response_id` 锁定的 provider 是唯一候选——不经
  filter / match / rank，原生转发（不锁 model——链式响应可以换模型）
- **非优化维上限**：eco → latency ≤ L_max；speed → cost ≤ C_max
  （"上限"= cap，非下限）[TODO: #4]

**条件约束**（hard-if-explicit）：

- **需求匹配**：意图 → 需求 → 模型属性数据满足——默认中性（尽力放行），
  显式需求时收紧为约束
- `context_min`：显式即**硬**——模型必须满足；属性未知 → 中性放行
- `reasoning`：**软**——仅显式要求时过滤；未声明者放行
- 观察/记账是实现层，不属本规格

### 2.5 目标函数（准则，偏好）

- 准则集 g = (cost, latency)——v1 固定两准则；一般形式 score(x; Λ) =
  Σᵢ λᵢ·normᵢ(x)（λ 向量），两维为特例 [TODO: #12 扩展语义]
- score(x; Λ) = λ·norm(cost_x) + (1-λ)·norm(latency_x)
- cost(x) = 单位价格（input/output 组合，时变：flat/timed）——请求前置 token
  数未知，cost 只能是价格量；**quota 与 CONTEXT.md 定价类型定义冲突**（CONTEXT
  列 flat/timed/quota；本模型判定 quota 边际价格依赖用量状态，模型无此状态
  变量，暂按可用性维度处理）——冲突待产品决定 [TODO: #5、#17]
- latency(x) = 近期观测的稳健统计量（分位数）[TODO: #6]
- λ 来自 mode：{eco: 0.8, balanced: 0.5, speed: 0.2} [TODO: #7 标定]；λ∈(0,1)
  （不取端点：端点退化为单目标）[TODO: #13 端点理由]
- norm = 归一化：**模型决策**（非实现细节）——把准则映射到可比尺度；λ 的
  解释以 norm 确定为前提 [TODO: #8：方法 + 候选集范围 + 退化（min-max 的
  max=min）处理]
- 同分 → 确定性 tiebreak：固定全序（如 id），与偏好无关、不随 F 变化
  （保证总序）
- **冷启动**：新候选无观测/无价格时 latency/cost 无定义——须定义初值语义
  [TODO: #14；交互约束：排除若在约束层则缩 F 误报 infeasible，若在排序层则
  argmin 可能无定义——须明确落点]

### 2.6 决策（求解，固定机制）

- 可行域 F（§2.4）——约束应用（filter）
- **失败分类（三类，不混层）**：
  1. **实例无效**（识别失败：词形在 K 中不可识别，P_pred 不可构造）——不经
     可行域，直接返回
  2. **infeasible**（约束层：F = ∅）——无可行候选
  3. **failover**（执行层：x\* 失败且满足重试条件）——同一 F 排序列表消费
     下一个
- 求解流程：x\* = argmin\_{F} score(x; λ)
- **失败结局全集**：请求结局 ∈ {实例无效, infeasible, 请求侧拒绝（网关侧
  校验 400，直接返回，不重试）, failover 链路（重试 → 成功 / 候选耗尽 →
  错误 + 聚合诊断）}——互斥且穷尽

**failover 触发条件**（重试 ⟺ 三者同时成立）：

1. **失败归因于目标**（候选侧：连接失败/超时、5xx、429 限流、401 **候选侧**
   凭据无效、404 模型不存在）——而非请求（**网关侧**校验 400：同一请求任何
   候选网关侧校验一致，不重试；网关侧认证失败（per-app token 无效）亦归请求
   侧；**翻译后** provider 侧 4xx 依赖候选的目标协议/方言，归目标侧，可重试）
   [TODO: #15 未列状态（403 等）的默认归因]
2. **响应未开始**（已发内容无法重放）
3. **无强制路由约束**：续聊（previous_response_id）锁定 provider，F 为单元素，
   无下一个可消费——failover 无候选，直接返回

## 3. 求解

- filter-then-rank——池为几十量级，精确枚举（O(n·|P_pred|) 过滤 + 排序取头）
  [TODO: #9 性质分析：确定性、复杂度、failover 语义；惰性选择 vs 全排序]

## 4. 验证

[TODO: #10 待实现后逐条执行]

- **完备性**：需求可满足（F ≠ ∅）⟹ 请求终止于服务模型或良定义失败
  ——实质性质（非定义恒真）
- **失败分类闭合**：实例无效 / infeasible / failover 三类互斥且穷尽
- λ 加权排序正确性（同分 tiebreak、归一化、冷启动）
- 约束谓词逐条可单测（协议/负载/可用性/匹配/上限/需求）

## 5. 未决项（Todo）

- [ ] #2 需求值域：协议锚定维度集（context_length 已锚定，其余待定）
- [ ] #3 模型属性数据来源：仅 OpenRouter 原生带，其余靠注册表/静态数据
- [ ] #4 L_max / C_max：非优化维上限的具体值（配置面）
- [ ] #5 cost 定义细节：input/output 组合比例；timed 时段函数；quota 仅可用性维度
- [ ] #6 latency 统计量：观测窗口；分位选择
- [ ] #7 λ 具体值：eco/balanced/speed 的权重标定
- [ ] #8 norm 方法：归一化方法（模型决策）；候选集范围；退化处理（max=min）
- [ ] #9 求解性质：确定性、复杂度、failover 语义；惰性 vs 全排序
- [ ] #10 验证执行：§4 各项待实现后逐条执行
- [ ] #12 准则扩展：一般形式 score(x; Λ) 扩展到 >2 准则的语义
- [ ] #13 λ 端点理由：排除端点（纯单目标）的形式化理由
- [ ] #14 冷启动：无观测/无价格候选的初值语义（乐观/排除）
- [ ] #15 归因默认：failover 未列状态码（403 等）的默认归因
- [ ] #17 quota 冲突：quota 定价 vs 可用性维度：与 CONTEXT.md 定义冲突，待产品决定

## 6. 引用

- 术语：`CONTEXT.md`
- 相关 ADR
  - `adr/0009-routing.md`（请求面路由决策）、
  - `adr/0008-data-asset.md`（模型配置数据：context、能力位）
- **参考实现**（同构系统，工程验证）：
  - 数据库查询优化器：谓词 + 代价模型 + `ORDER BY LIMIT 1` 同构度最高
  - Kubernetes scheduler：predicates（硬过滤）+ priorities（评分）
  - 主流 AI 网关：LiteLLM / OpenRouter / Cloudflare / Portkey（候选 + 策略 + fallback）
  - 服务网格（Envoy）：健康过滤 + 负载均衡 + retry/failover
