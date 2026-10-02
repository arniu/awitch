# Provider protocol survey — data

> The per-vendor data behind `provider-protocol-survey.md`'s derived rules: how
> each probed vendor serves each protocol family, plus model-list and
> balance/quota endpoints. The survey holds the analysis (rules, balance
> verification, methodology); this file holds the data.
> **Scope**: tracks the three protocol families (anthropic / openai chat /
> openai responses); Gemini/Bedrock per-vendor data is not surveyed — the
> Gemini access modes below are survey context, not one of the three families.
> Probed 2026-08; the five `†` balance/quota endpoints re-verified 2026-09
> (survey §5); SiliconFlow CN + intl re-probed 2026-09-17.
> `✓` = probed; `(docs)` = official docs; `‡` = auth-gated under empty
> credentials (endpoint existence inferred from 401 — verify before onboarding);
> `†` = second-hand from cc-switch, to be verified.

## Vendor registry (API-probed)

### DeepSeek — `api.deepseek.com` ‡

- **Anthropic**: `/anthropic` ✓ (docs ✓)
- **Chat**: `/v1/chat/completions` ✓
- **Responses**: `/v1/responses` ✓
- **Model list**: `/v1/models` ✓
- **Balance**: `/user/balance` (docs)

### Kimi/Moonshot — `api.moonshot.cn`

- **Anthropic**: `/anthropic` ✓
- **Chat**: `/v1/chat/completions` ✓
- **Responses**: `/v1/responses` ✓
- **Model list**: `/v1/models` ✓
- **Balance**: —

### Kimi For Coding — `api.kimi.com`

- **Anthropic**: `/coding/`
- **Chat**: `/coding/v1/chat/completions` ✓
- **Responses**: —
- **Model list**: `/coding/v1/models` ✓
- **Balance**: `/coding/v1/usages` ✓

### MiniMax — `api.minimaxi.com`

- **Anthropic**: `/anthropic` ✓
- **Chat**: `/v1/chat/completions` ✓
- **Responses**: `/v1/responses` ✓
- **Model list**: `/v1/models` ✓
- **Balance**: `…/v1/api/openplatform/coding_plan/remains` (probed)

### Zhipu GLM — `open.bigmodel.cn`

- **Anthropic**: `/api/anthropic` ✓
- **Chat**: `/api/coding/paas/v4/chat/completions` ✓
- **Responses**: `/…/paas/v4/responses` ✓
- **Model list**: `/…/paas/v4/models` ✓
- **Balance**: `/api/monitor/usage/quota/limit` †

### Novita AI — `api.novita.ai`

- **Anthropic**: `/anthropic` ✓ (docs ✓)
- **Chat**: `/openai/v1/chat/completions` ✓
- **Responses**: `/openai/v1/responses` ✓
- **Model list**: `/openai/v1/models` **public 200**
- **Balance**: `/v3/user/balance` ✓

### Xiaomi MiMo — `api.xiaomimimo.com`

- **Anthropic**: `/anthropic` ✓ (official ✓)
- **Chat**: `/v1/chat/completions` ✓
- **Responses**: `/v1/responses` ✓
- **Model list**: `/v1/models` ✓
- **Balance**: —

### Longcat — `api.longcat.chat`

- **Anthropic**: `/anthropic` ✓
- **Chat**: `/openai/v1/chat/completions` ✓
- **Responses**: `/openai/v1/responses` ✓
- **Model list**: `/openai/v1/models` ✓
- **Balance**: —

### StepFun — `api.stepfun.com` ‡

- **Anthropic**: `/step_plan` ✓
- **Chat**: `/step_plan/v1/chat/completions` ✓
- **Responses**: `/step_plan/v1/responses` ✓
- **Model list**: `/step_plan/v1/models` ✓
- **Balance**: `/v1/accounts` (docs)

### Volcano Ark — `ark.cn-beijing.volces.com` ‡

- **Anthropic**: `/api/coding` (Agentplan) / `/api/compatible` (DouBaoSeed)
- **Chat**: `/api/coding/v3` ✓, `/api/v3` ✓
- **Responses**: same ✓
- **Model list**: —
- **Balance**: control-plane OpenAPI (AK/SK) (docs)

### Alibaba DashScope — `{WorkspaceId}.cn-beijing.maas.aliyuncs.com` (regional)

- **Anthropic**: `/apps/anthropic` ✓
- **Chat**: `/compatible-mode/v1/chat/completions` ✓ (docs ✓)
- **Responses**: —
- **Model list**: `/compatible-mode/v1/models` ✓
- **Balance**: —

### Baidu Qianfan — `qianfan.baidubce.com`

- **Anthropic**: `/anthropic` ✓
- **Chat**: —
- **Responses**: —
- **Model list**: `/v1/models` **public 200**
- **Balance**: —

### Tencent TokenHub — `tokenhub.tencentmaas.com`

- **Anthropic**: —
- **Chat**: —
- **Responses**: `/v1/responses` ✓
- **Model list**: `/v1/models` ✓
- **Balance**: —

### Nvidia — `integrate.api.nvidia.com`

- **Anthropic**: —
- **Chat**: —
- **Responses**: `/v1/responses` ✓
- **Model list**: `/v1/models` **public 200**
- **Balance**: —

### ModelScope — `api-inference.modelscope.cn`

- **Anthropic**: `/v1/messages` ✓ (root)
- **Chat**: `/v1/chat/completions` ✓
- **Responses**: —
- **Model list**: `/v1/models` **public 200**
- **Balance**: —

### SiliconFlow — `api.siliconflow.cn` ‡

- **Anthropic**: `/v1/messages` ✓ (root)
- **Chat**: `/v1/chat/completions` ✓
- **Responses**: **✗ (404)**
- **Model list**: `/v1/models` ✓
- **Balance**: `/v1/user/info` (docs)

### SiliconFlow International — `api.siliconflow.com` ‡

- **Anthropic**: `/v1/messages` ✓ (root)
- **Chat**: `/v1/chat/completions` ✓
- **Responses**: **✗ (404)**
- **Model list**: `/v1/models` ✓
- **Balance**: `/v1/user/info` (docs; USD)

### OpenRouter — `openrouter.ai`

- **Anthropic**: `/api/v1/messages` ✓
- **Chat**: `/api/v1/chat/completions` ✓
- **Responses**: `/api/v1/responses` ✓
- **Model list**: `/api/v1/models` **public 200**
- **Balance**: `/api/v1/credits`

### GitHub Copilot — `api.githubcopilot.com`

- **Anthropic**: `/v1/messages` ✓
- **Chat**: `/chat/completions` ✓
- **Responses**: `/v1/responses` ✓
- **Model list**: —
- **Balance**: Copilot quota

### xAI/Grok — `api.x.ai`

- **Anthropic**: unreachable
- **Chat**: unreachable
- **Responses**: unreachable
- **Model list**: unreachable
- **Balance**: —

## Gemini access modes

Survey context — awitch does not serve the Gemini protocol. GATEWAY semantics
(`GOOGLE_GEMINI_BASE_URL`, `x-goog-api-key`) — survey appendix.

| Mode                               | Endpoint                                                                  | Vendors                        |
| ---------------------------------- | ------------------------------------------------------------------------- | ------------------------------ |
| Gemini native                      | `generativelanguage.googleapis.com` (unreachable, official API)           | Gemini Native                  |
| Gemini OAuth                       | — (CLI login state)                                                       | Google Official                |
| GATEWAY (Gemini-protocol endpoint) | `GOOGLE_GEMINI_BASE_URL` → third-party gateway                            | per-vendor verification needed |
| Gemini-style `/v1beta`             | `api.etok.ai/v1beta/models` (403 ✓), `subrouter.ai/v1beta/models` (401 ✓) | ETok.ai / SubRouter            |
| Vertex bypass                      | `api.qnaigc.com/bypass/vertex`                                            | Qiniu                          |
| `/api/gemini`                      | `api.aicodemirror.ai/api/gemini/models` (401 ✓)                           | AICodeMirror                   |
