# Runtime contract

The stable conventions a running Awitch honors.

## Ports

| Port  | Role          | Auth                                |
| ----- | ------------- | ----------------------------------- |
| 10689 | data plane    | per-app token (bearer credential)   |
| 10690 | control plane | `control.token` (strong, 0600 file) |

Setting resolution (per setting: data host/port, control host/port):

```
AWITCH_* env          ──→ override
config.toml [server]  ──→ set value
missing field         ──→ default (127.0.0.1 / 10689 / 10690)
unknown field/section ──→ error (typos never run silently on defaults)
```

> **NOTE**:
>
> - `parseInt('awitch', 36) % 65536` => `60689` => `10689`
> - `60689` falls in the ephemeral port range (macOS / Windows 49152–65535; Linux 32768–60999)

## Files

`AWITCH_CONFIG_DIR` overrides all paths; otherwise `.awitch/` under `dirs::home_dir()`

```
~/.awitch/                    # AWITCH_CONFIG_DIR
├── config.toml               # machine-local settings
├── data.db                   # SQLite (single writer, WAL)
├── server.pid                # process ID (conventional pidfile)
├── server.lock               # single-instance flock (held while the gateway runs)
├── control.token             # control API token (0600, generated at first start)
├── logs/                     # rotation logs: `awitch.<YYYY-MM-DD>.log`
└── point/                    # pointing records + per-app locks (dir 0700)
    ├── <app>.toml            # what `point` replaced, for `point undo` (0600)
    └── <app>.lock            # per-app flock held across a point/undo/reset flow
```

## config.toml

Startup seed only:

```toml
[server]
host = "127.0.0.1"          # data bind/dial host
port = 10689                # data port (agent requests)
control_host = "127.0.0.1"  # control bind/dial host
control_port = 10690        # control port (CLI commands)
```
