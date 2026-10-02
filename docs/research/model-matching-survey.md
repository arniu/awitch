# Model matching survey

> Purpose: calibrate ADR-0009 (the model-matching part of routing); serve as
> implementation reference for the model-matching topic. Research date
> 2026-08; all claims from primary sources (official docs), fetched 2026-08.

## Objects and sources

| Product               | Type                               | Primary source                                                                              |
| --------------------- | ---------------------------------- | ------------------------------------------------------------------------------------------- |
| LiteLLM               | Open-source AI proxy (self-hosted) | https://docs.litellm.ai/docs/routing                                                        |
| OpenRouter            | Aggregated routing service         | https://openrouter.ai/docs/guides/routing/provider-selection / model-fallbacks / routers/\* |
| Cloudflare AI Gateway | Managed gateway (obs/control)      | https://developers.cloudflare.com/ai-gateway/ (index / usage / configuration/fallbacks)     |
| Portkey               | Managed gateway (enterprise)       | https://docs.portkey.ai/docs/product/ai-gateway/conditional-routing / model-catalog         |

## How each gateway matches

### LiteLLM — `model_name` → `model_list` (declared groups + strategy pick)

- Multiple deployments in `model_list` share one `model_name` (comment:
  "model alias"). The request `model` field = `model_name`; it **literal-matches**
  every same-named deployment (across Azure regions / OpenAI / any provider).
- Routing strategy picks one deployment: weighted pick (default), rate-limit
  aware (rpm/tpm), latency-based, least-busy, **lowest-cost**, usage-based
  (Redis-tracked).
- Reliability: cooldowns, fallbacks, timeouts, retries, health-check-driven
  routing.
- **Takeaway: match = request name hits a config table → candidate endpoint
  group → strategy picks one.** Matching knowledge is explicitly declared by
  the admin.

### OpenRouter — model slug → provider endpoints (platform-maintained + price/health weighted)

- Model names are normalized platform slugs (e.g. `anthropic/claude-sonnet-4.5`);
  one slug maps to multiple provider endpoints (same model, multiple hosts).
- Default routing = price-weighted load balancing: drop providers with a major
  outage in the last 30 s, then weight by the **inverse square of price**
  (source: OpenRouter routing guide); the rest form the fallback chain.
- Overrides: `provider.order/only/ignore`, `sort` (price/throughput/latency),
  variants (`:extended` / `:thinking` / `:nitro` / `:exacto`).
- Aliases: `~author/family-latest` resolves to the newest concrete model in a
  family, tracking releases automatically.
- Capability routing (beta): pareto router selects by coding score (source:
  OpenRouter routers docs); a product feature, not the default path.
- **Takeaway: match = slug lookup in a platform catalog → candidate endpoints →
  price/health-weighted pick.** Knowledge is platform-maintained (slug catalog =
  name → capability + endpoint + price).

### Cloudflare AI Gateway — explicit target + explicit fallback chain

- Unified OpenAI-compatible endpoint forwards to the provider/model named in
  the request; or provider-native passthrough.
- Fallbacks: a config array tried in order; failure/timeout advances to the
  next; the `cf-aig-step` header reports which step succeeded.
- Dynamic routing (JSON config): routing flows by condition/quota/fallback.
- **Takeaway: match = name specifies the target + an explicit ordered fallback
  list.** No capability derivation.

### Portkey — conditional rules → target (policy routing)

- Three strategies: `conditional` (if-then rules on metadata/params/url →
  target), `fallback`, `loadbalance`.
- E.g. `params.model = "fastest"` → gpt-4o-mini; `temperature > 0.7` → a more
  creative model.
- **Takeaway: match = rule engine (literal/conditions) → explicit target.**
  Knowledge = user-configured rules.

## What model matching does

Across all four, model matching is **the first step of routing: resolving the
model field into a forwarding target** — one concrete endpoint or an ordered
candidate set. Its purposes: ① endpoint abstraction (the app knows one name);
② reliability (health filtering, fallback chains, cooldowns); ③ cost/performance
optimization (price weighting, throughput/latency sorting); ④ policy control
(conditional routing, quotas); ⑤ observability.

## Two matching philosophies

|                      | Declarative (mainstream: LiteLLM/OpenRouter/CF/Portkey) | Capability-derived (Awitch)                   |
| -------------------- | ------------------------------------------------------- | --------------------------------------------- |
| Name → what          | → an **endpoint or endpoint group** in config/catalog   | → capability requirements → pool match        |
| Where knowledge sits | admin config table / platform slug catalog              | user config + grammar                         |
| Precondition         | names normalized; the app knows what it wants           | name carries mixed identity/capability intent |

## Calibration points for ADR-0009

1. **The decode layer is Awitch-specific**: no mainstream gateway has
   "name → capability → automatic pool matching" — names are normalized
   upstream (LiteLLM declarations, slug catalogs), so the gateway only does
   "name → endpoint" lookup plus endpoint optimization. Awitch agent names
   carry mixed identity/capability intent, hence the decode layer — no
   existing pattern to copy, which is what grounds the ADR-0009 model-matching
   design (intent by write-source + two passes).
2. **Matching knowledge is explicit data** (all four, without exception) —
   consistent with ADR-0009's design intent: explicit stored data, no built-in
   name→capability decoding. The difference is the mainstream stores "name →
   endpoint" while Awitch's design stores "name → capability" — that registry
   is deferred in code (identity-only matching, 2026-08) pending a
   model-capability data source.
3. **OpenRouter's slug catalog is a platform knowledge base** (name →
   capability + endpoint + price), isomorphic to Awitch's provider pool
   (model → capability + price) — implementation reference: the pool carries
   capability data; slug normalization is exactly the gap the decode layer
   fills.
