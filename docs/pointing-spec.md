# pointing 规格

> 以算法分析方法形式化 pointing：从问题定义出发识别模型（WAL 状态机），
> 确定正交完备的状态空间，把操作定义为状态空间上的转移函数——正确性由构造保证
> （WAL 写序、正交状态、门控、幂等）。任意时刻中断均不破坏用户数据，状态可判定、可恢复。
>
> 术语首次出现用全称（`CONTEXT.md` 词条），正文引用用简称。

## 1. 问题定义

对**外部资源**（agent 配置）执行**可逆变更**：操作在任意时刻中断（崩溃、
断电、杀进程），用户数据不丢失、状态可判定、可恢复。

## 2. 模型：WAL 状态机

问题要求"可逆变更 + 任意中断可判定/可恢复"，这组约束强制了模型的以下结构。

- **可逆 ⇒ 独立记录**。变更作用于外部资源，被改文档（C）不保证记录历史数据，
  因此变更必须有记录——**指向记录**（pointing record，R）。
- **两文件无事务 ⇒ 写序固定为 WAL**。写入之间无原子性，任意中断点都要可判定、可恢复，故记录先于文档。
- **R 的内容由还原与校验的需要决定**：
  - **K**（键集）：apply 触碰的受管键集合——还原/剥离的范围
  - **E**（new）：apply 将写的值——文档状态与还原门控的参照量
  - **O**（old）：被替换的旧值——还原的依据

## 3. 状态空间

状态空间是积空间 C × R：C 与 R 对应两个独立文件；值集互斥、穷举，空间正交完备。

- **C**（文档）∈ { absent, present(c) }：C 的内容永远是用户数据。
- **R**（指向记录）∈ { absent, corrupt, complete(O, E, K) }：
  - `absent`——文件不存在
  - `corrupt`——数据损坏，或校验未通过
  - `complete(O, E, K)`——携带指向载荷（O/E/K）：存在 ∧ 可解析 ∧ 校验通过 ∧ K
    覆盖所有触碰键
- **错误通道**：任一文件读不到（权限 / IO 等）→ **读取错误**，非状态，
  直接**报错停止**，不进入判定。

## 4. 操作

操作是面向用户的接口，在状态空间 S = C × R（§3）上定义为转移函数 δ: S ⇀ S
（部分函数）。四个操作都作用于单个 app（对象）；接口只列对象之外的输入输出。

### 可观察状态

可观察状态是状态空间的投影，为判定（check 的输出）提供依据，不引入新状态。
投影从 C、R 派生：

- **文档状态** ∈ { new, divergent } = (C == E)，仅当 C = present 且 R = complete
  （E 可得）时有定义：
  - **new** = 文档 == 记录承诺的新值（E）
  - **divergent** = 文档 ≠ E（外部修改或中断残留，文件上不可区分）
- 纯观察、不混合判定；输入只有本地文件（C、R）与受管键集合，不读网关、不问控制面。

### check

检查当前状态。

- **接口**：输出判定（verdict）+ target。target 从 C 按受管键字段读回（url/key），
  与 R 无关：只要 C 可读且装有对应字段即给出，否则 None——reset 后的残留即
  借此暴露（退化原则：残留明示不静默）。
- **转移函数**：恒等——δ(c, r) = (c, r)，∀(c, r) ∈ C × R。不改变状态，只输出判定。

**判定（verdict）**由可观察状态合成，是状态 → 推荐操作的反向映射：

1. 读取错误 → **报错停止**（用户修复后重试）
2. R = absent → **未开始（点）**
3. R = corrupt → **不可恢复（剥离）**
4. R = complete：
   - C = absent → **不可恢复（文档缺失）**
   - 文档状态 = divergent → **需处理（undo-first）**
   - 文档状态 = new → **正常**

**范围**：可观察状态只回答"app 的本地文件能推出什么"，不判动作。与世界正确性的比较
（如网关当前端口）是外部问题，不在本模型内。

### reset

清理损坏的指向记录（R = corrupt 时）。

- **接口**：输出 已清理 / 无损坏记录。
- **转移函数**：定义域 { (c, corrupt) }；δ(c, corrupt) = (c, absent)——只清 R，不碰 C。
- **步骤**：读 R → corrupt 则删记录。幂等：重跑时 R = absent → 无损坏记录。
- **范围**：纯 R 操作，不读配置、不判 URL、不剥离受管键。配置剥离不是 reset 的职责
  （还原配置由 undo 按记录完成）；损坏记录清理后若配置仍有残留，由 check 的
  target 读回暴露，用户重新 point 覆盖或手动清理。

退化原则：只清理 awitch 自己的记录，绝不误删用户配置；残留明示不静默。

### point

指向网关。

- **接口**：输入 target（URL + token），输出完成 / 错误。
- **转移函数**：定义域 { (present(c), absent) }；δ(present(c), absent) =
  (present(E), complete(O, E, K))，其中 (O, E, K) = plan(c)。
- **步骤**：plan(C) → 写记录（R）→ apply(C)——按 WAL 先写记录、后写文档，
  把文档改写为指向网关的可逆变更。
- **可中断**：写 R 前 / apply 前 / apply 中途。
- **幂等**：中断后重跑采用 undo-first——先按记录还原 C_old（门控），再重新
  plan/apply 到新目标（不重放旧 C_new）。

### undo

撤销指向、恢复原样。

- **接口**：输出已撤销 / 无可撤销。
- **转移函数**：定义域 { (present(c), complete(O, E, K)) }；门控 current[k] ∈
  {O[k], E[k]}；δ(present(c), complete) = (present(O), absent)。
- **步骤**：门控校验（current 未被外部修改）→ 按记录的 O 逐键还原 C_old → 清
  记录——point 的逆操作；既作为重跑 point 的内部恢复步骤，也可作为独立操作调用。
- **可中断**：还原中途。
- **幂等**：门控下，已还原键跳过、未还原键继续。

### 状态转移完备性（无死锁）

| 状态 (C, R)           | point  | undo     | reset      | 出口         |
| --------------------- | ------ | -------- | ---------- | ------------ |
| A (absent, absent)    | ✓ → D  | 无可撤销 | 无损坏记录 | point        |
| B (present, absent)   | ✓ → D  | 无可撤销 | 无损坏记录 | point        |
| C (absent, complete)  | ✓      | ✓ → A    | 无损坏记录 | undo / point |
| D (present, complete) | ✓      | ✓ → B    | 无损坏记录 | undo / point |
| E (absent, corrupt)   | ✗ 报错 | ✗ 报错   | ✓ 清 R → A | reset        |
| F (present, corrupt)  | ✗ 报错 | ✗ 报错   | ✓ 清 R → B | reset        |

前 4 个状态由 point/undo 覆盖；E/F（corrupt）由 reset 兜底。

## 5. 假设清单

1. plan 是纯函数（同一 C 产生确定 (O, E)）——§4
2. 故障持久性语义（进程级中断 vs 断电）——§1
3. 并发建模：同一 app 的变更全程互斥（串行）——§4
4. 受管键集合可完整枚举（含指针键），剥离无残留——§4

## 6. 参考资料

- **WAL、undo 日志、恢复状态、幂等恢复** —— Mohan et al., "ARIES: A Transaction Recovery Method Supporting Fine-Granularity Locking and Partial Rollbacks Using Write-Ahead Logging", ACM TODS 17(1), 1992
- **WAL/恢复的系统性参考** —— Gray & Reuter, _Transaction Processing: Concepts and Techniques_, Morgan Kaufmann, 1993
- **CONTEXT.md** — 领域词汇表
