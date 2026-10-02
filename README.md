# A Local AI Gateway

Point your agents once, and Awitch will route every request to the
cheapest or fastest vendor — your keys never leave your machine.

> Agents supported: `claude` / `codex` / `opencode` / `hermes` / `openclaw` / `pi`.

---

## 🚀 Quick Start

```bash
# 1. Add a vendor you have a key for (deepseek, openai, …)
awitch provider add -t deepseek --key <your-key>

# 2. Point agents at Awitch once (writes each agent's native config)
awitch point --all

# 3. Done. Every request auto-routes:
awitch status                   # gateway / pool / health / pin
awitch usage                    # tokens / cost usage
awitch settings mode            # show routing mode (eco / speed / balanced)
awitch provider balance <id>    # balance / coding-plan quota

# Pin a provider manually (skip auto-decision)
awitch pin deepseek --app claude
```

---

## ✨ Features

### 🔌 Provider pool

Awitch ships the common AI vendors (deepseek, openai, …), so adding one is a
one-liner with your key. Anything else — your own gateway, a proxy, a smaller
vendor — you add yourself by its API base URL.

Pricing is each vendor's real pricing — flat, tiered by time of day (peak /
off-peak), or a plan with a free quota — so "cheapest" and your usage totals
are the actual numbers.

```bash
awitch provider list                # list providers
awitch provider add -t <template> --key <key>   # add a provider
awitch provider edit <id>           # change a provider
awitch provider delete <id>         # remove a provider
awitch provider probe <id>          # check its endpoints respond
awitch provider balance <id>        # balance / coding-plan quota
awitch provider template [id]       # write a built-in vendor's file to customize (no id: list them)
```

**Adding a provider**

When Awitch knows the vendor, a one-liner with your key — and Awitch keeps
such a provider current across upgrades (only its key is editable):

```bash
awitch provider add -t deepseek --key <your-key>
```

For a provider Awitch doesn't ship — your own gateway, a proxy, a less common
vendor — give it its API base URL:

```bash
awitch provider add --base-url https://api.example.com/v1 \
    --key <your-key> --protocols openai_chat
```

Want a built-in vendor with your own settings (a different endpoint, extra
paths, no balance endpoint)? Write its definition file, edit it, and add from
the file — the result is fully yours and never changed behind your back:

```bash
awitch provider template deepseek        # writes deepseek.toml to edit
awitch provider add -t ./deepseek.toml --key <your-key>
```

`provider add` assigns each provider an id and prints it — reference the
provider by that id in `edit`, `delete`, `pin`, `balance`, and `probe`.

### 🔄 Migration (from cc-switch)

Coming from cc-switch? Bring your providers in:

```bash
awitch migrate cc-switch --dry-run   # preview what will be imported, write nothing
awitch migrate cc-switch             # import, then check each provider's endpoints
```

### 🚦 Auto-routing

Every request is routed to a provider that is healthy, within budget, and able
to serve it — a pinned provider wins if you set one — then ranked by mode:

- `eco` — cheapest first
- `speed` — fastest first
- `balanced` — a blend of price and speed

```bash
awitch pin deepseek --app claude          # use this provider for claude
awitch pin deepseek --all                 # use it for every app
awitch pin remove deepseek --app claude   # stop using it for claude
awitch pin clear --app claude             # clear claude's pins (auto-routing)
awitch pin clear --all                    # clear every app's pins
awitch settings mode eco                  # switch routing mode
```

### 📊 Accounting

Usage and cost are tracked per provider × model — tokens, cost, latency,
success — so you can see where your spend actually goes.

### 🔧 Gateway & utilities

The gateway runs locally in the background.

```bash
awitch status                           # gateway / pool / health / pin

awitch point undo --all                 # un-point agents (restore originals, drop keys)
awitch point reset --all                # reset pointing that cannot be undone

# Completion scripts are printed to stdout — install by redirecting one into
# your shell's dir. Each shell's `awitch completions <shell> --help` shows the
# exact copy-paste install / uninstall commands (bash, zsh, fish, powershell).
awitch completions bash > ~/.local/share/bash-completion/completions/awitch
```

---

## 🛠️ Development

- **Rust** 1.89+; `cargo build` / `cargo test` / `cargo build --release`.
- Design decisions: `docs/adr/` (status index: `docs/adr/README.md`).
- Domain vocabulary: `CONTEXT.md`.
- Runtime contract: `docs/runtime-contract.md`.
- Roadmap: `roadmap.md`.

## 📜 License

[MIT](LICENSE)
