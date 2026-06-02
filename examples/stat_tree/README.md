# stat_tree

Display Varnish statistics as a hierarchical tree, using dots as section separators (e.g. `MAIN.cache_hit` appears under `MAIN`).

```
$ stat_tree -I 'VBE.*' -v
VBE
└── boot
    └── default
        ├── bereq_hdrbytes:                  543  bytes    Request header bytes
        ├── beresp_bodybytes:               3885  bytes    Response body bytes
        ├── beresp_hdrbytes:                 526  bytes    Response header bytes
        ├── happy:            0xffffffffffffffff  bitmap   Happy health probes
        └── req:                               3  integer  Backend requests sent
```

By default, metrics with a value of `0` are hidden. Use `-a` to show all.

## Flags

| Flag | Type     | Default      | Description                                               |
|------|----------|--------------|-----------------------------------------------------------|
| `-a` | bool     | false        | Show all metrics, including zero-value ones               |
| `-I` | glob     |              | Include matching fields (VSC-level, may be repeated)      |
| `-X` | glob     |              | Exclude matching fields (VSC-level, may be repeated)      |
| `-n` | string   |              | Varnish instance name (working directory)                 |
| `-t` | seconds  | 5            | Attach timeout                                            |
| `-v` | bool     | false        | Show format and short description columns                 |

`-I` and `-X` are passed directly to VSC and filter which metrics are tracked at the library level. When both are used, all `-I` patterns are processed before `-X`.

## Examples

```bash
# Show all non-zero stats for the default instance
stat_tree

# Show all stats including zeros, with descriptions
stat_tree -a -v

# Include only VBE metrics, verbose, named instance
stat_tree -I 'VBE.*' -v -n myinstance
```
