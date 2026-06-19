# Herakles Node Exporter

[![Rust](https://img.shields.io/badge/rust-stable-orange)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)
[![Prometheus](https://img.shields.io/badge/prometheus-compatible-red)](https://prometheus.io/)

A Prometheus exporter for Linux system metrics that aggregates per-process resource usage into named process groups —
exposing stable, cardinality-safe time series at `/metrics` and full per-process forensic detail at `/html/details`.

---

## What it does

Herakles scrapes `/proc` on every request, classifies each running process into a `(group, subgroup)` pair using a
static lookup table, and exposes two fundamentally different views of that data:

- **`/metrics`** — Prometheus scrape endpoint. All process data is aggregated into `(group, subgroup)` pairs before
  encoding. No PID labels, no process name labels. Safe to scrape continuously at any interval.
- **`/html/details`** — Operator inspection endpoint. Full per-process breakdown with PIDs, USS, CPU%, I/O rates, and
  temporal anomaly classification. Intentionally not scraped by Prometheus.

The separation is architectural and deliberate. See [Why this architecture?](#why-this-architecture) for the reasoning.

---

## Quick Start

```bash
curl -fsSL https://github.com/herakles-now/herakles-node-exporter/releases/latest/download/install.sh | sh

sudo herakles-node-exporter install

curl -f http://localhost:9215/health
```

Manual installation instructions are in [wiki/Installation.md](wiki/Installation.md).

Prometheus scrape config:

```yaml
scrape_configs:
  - job_name: 'herakles'
    static_configs:
      - targets: ['localhost:9215']
    scrape_interval: 60s
    scrape_timeout: 30s
```

---

## Why this architecture?

Most process-aware exporters expose per-PID label dimensions. This causes two well-understood problems: label
cardinality explodes as processes restart and accumulate new PIDs, and the resulting time series are too fine-grained
to be actionable in alert rules. An alert that fires on `process{pid="12345"}` is operationally useless — the PID
changes every restart and the label set varies across hosts.

Herakles solves this by classifying every running process into a named `(group, subgroup)` pair at scrape time.
`herakles_group_memory_rss_bytes{group="db",subgroup="postgres"}` means the same thing on every host and survives any
number of postgres restarts without stale series accumulation. This makes it safe to write recording rules and
multi-host federation queries over group metrics without cardinality concerns.

The human operator, however, needs the opposite: when an alert fires on `herakles_group_memory_rss_bytes`, they want
to know *which* postgres process is responsible. For this, `/html/details` intentionally exposes high-cardinality data
— PIDs, USS per process, CPU percentages — that would be unsafe in Prometheus but are perfectly appropriate for a
forensic endpoint read by humans or automation on demand, never scraped continuously.

**The cache model follows from the same reasoning.** `/metrics` serves data from an in-memory cache refreshed at most
every 5 seconds (`CACHE_UPDATE_THROTTLE_SECS`). A Prometheus scrape never blocks on `/proc` I/O: the scrape handler
reads from cache while a background tokio task asynchronously updates it. Staleness on the order of seconds is
irrelevant for a monitoring system.

---

## The Endpoints

Every capability in Herakles is exposed twice: once as plain text or Prometheus format for machines and automation,
and once as HTML for human operators. The split is intentional — the same underlying data, rendered appropriately for
each consumer.

### `/metrics` — Prometheus scrape target

Returns the full metric set in Prometheus text exposition format. No per-PID labels anywhere. All process-level data
has been aggregated into `(group, subgroup)` pairs before encoding. Scrape latency is bounded by lock acquisition
time, not `/proc` scan time.

This is the only endpoint Prometheus should scrape. When an alert fires — e.g.
`herakles_group_memory_rss_bytes{group="db",subgroup="postgres"} > 8e9` — open `/html/details?subgroup=postgres` on
the affected host to identify the responsible process.

### Process detail — `/details` and `/html/details`

Full per-process breakdown from the in-memory cache. Both variants accept a `?subgroup=<n>` query parameter to filter
to a specific subgroup. `/details` returns plain text suitable for `curl` and scripts; `/html/details` returns a
sortable, filterable HTML table for the browser.

Beyond a simple process list, both endpoints implement **temporal zone classification**: each process is assigned to
one of three phases based on age.

| Phase | Age | What it means |
|---|---|---|
| Live | < 5 min | Recently started; no baseline established yet |
| Stabilization | 5–60 min | Settling; compared against short-term baseline |
| Historical | > 60 min | Stable; anomalies here are genuinely unexpected |

Within each phase, anomaly severity is computed from deviation against a rolling baseline stored in the ring buffer. A
postgres process in the Historical phase consuming 3× its normal RSS is flagged. A freshly started process consuming
the same amount is not.

**This is where you go after an alert fires.** `curl http://host:9215/details?subgroup=postgres` from a runbook, or
open `/html/details?subgroup=postgres` in a browser.

### Health — `/health` and `/html/health`

`/health` returns plain text with HTTP 200 if the cache is valid, 503 otherwise. Suitable for load balancer health
checks and monitoring probes.

`/html/health` renders the same data as HTML with additional detail: cache age, last update duration, buffer fill
levels for `/proc` I/O buffers (`smaps`, `smaps_rollup`, generic I/O), and eBPF subsystem status — whether eBPF
initialized successfully, events processed, and events dropped. This is the right place to look when diagnosing
unexpected metric gaps or eBPF initialization failures.

```bash
curl http://localhost:9215/health
# ok — cache age 3s, processes 347, ebpf ok
```

### Configuration — `/config` and `/html/config`

`/config` returns the effective merged configuration as plain text — the result of merging the config file with any
CLI overrides. Use this to verify that the right config file was loaded and that CLI flags took effect. Equivalent to
running `herakles-node-exporter --show-config` against the live process.

`/html/config` renders the same data as HTML with field descriptions inline.

```bash
curl http://localhost:9215/config
```

### Subgroups — `/subgroups` and `/html/subgroups`

`/subgroups` returns all loaded `(group, subgroup)` pairs with their process name patterns and command-line match
rules as plain text. Use this to verify that custom entries in `subgroups.toml` were parsed correctly, or to check
which pattern a specific process name would match.

`/html/subgroups` renders the same data as a searchable HTML table, useful for browsing the full 140+ entry
classification table.

```bash
curl http://localhost:9215/subgroups | grep postgres
# db  postgres  matches: [postgres, pg_dump, pg_restore, ...]
```

### Documentation — `/doc` and `/html/docs`

`/doc` returns a plain-text inline manual: all endpoints, all metrics, configuration fields, and example PromQL
queries. Intended as a self-contained reference accessible without network access to external documentation — a
manpage served over HTTP.

```bash
curl http://localhost:9215/doc | less
```

`/html/docs` and `/docs` render the same content as HTML with navigation.

### Landing page — `/` and `/html`

HTML landing page listing all available endpoints with links, exporter version, uptime, and current cache status. The
entry point for any operator who opens the exporter in a browser for the first time.

### Complete endpoint reference

| Path | Text | HTML | Description |
|---|---|---|---|
| `/metrics` | ✓ | — | Prometheus scrape target — no PID labels |
| `/details` | ✓ | — | Per-process forensic view with temporal classification |
| `/html/details` | — | ✓ | Per-process forensic view, sortable HTML table |
| `/health` | ✓ | — | Health check; 200 ok / 503 degraded |
| `/html/health` | — | ✓ | Health + buffer fill levels + eBPF status |
| `/config` | ✓ | — | Effective merged configuration |
| `/html/config` | — | ✓ | Effective configuration with field descriptions |
| `/subgroups` | ✓ | — | Loaded classification rules with match patterns |
| `/html/subgroups` | — | ✓ | Classification table, searchable |
| `/doc` | ✓ | — | Inline manual — metrics, config, PromQL (manpage over HTTP) |
| `/html/docs` | — | ✓ | Same as `/doc`, rendered as HTML |
| `/docs` | — | ✓ | Alias for `/html/docs` |
| `/` | — | ✓ | Landing page with endpoint index and uptime |
| `/html` | — | ✓ | Alias for `/` |

---

## Process Classification

Every process in `/proc` is classified into a `(group, subgroup)` pair by matching its executable name against
`data/subgroups.toml`. The matching logic checks `matches` (process name prefixes against `/proc/<pid>/comm`) and
`cmdline_matches` (substrings against the full command line, useful for JVM processes where `comm` is always `java`).
A process matching no entry is assigned `{group="other", subgroup="unknown"}`.

### Built-in groups

| Group | Subgroups |
|---|---|
| backup | bacula, commvault, cohesity, netbackup, networker, rubrik, spectrum_protect, tsm, veeam |
| cache | memcached, redis, varnish |
| cicd | ansible, chef, gitlab, jenkins, openstack, puppet, saltstack, terraform |
| container | containerd, crio, docker, kubelet, podman |
| db | cassandra, clickhouse, cockroachdb, couchbase, couchdb, db2, influxdb, mongodb, mssql, mysql, oracle, percona, postgres, rethinkdb, timescaledb, yugabyte |
| erp | peoplesoft, sap |
| logging | elasticsearch, filebeat, fluentd, graylog, kibana, log_collectors, logstash, splunk |
| messaging | activemq, kafka, nats, nsq, pulsar, rabbitmq, zeromq |
| monitoring | alertmanager, blackbox, grafana, icinga_nagios, node_exporter, percona, prometheus, snmp, telegraf, thanos, victoriametrics, zabbix |
| network | bind, dhcp, haproxy, keepalived, lvs, ntp, proxy_squid, vpn |
| runtime | go, java, nodejs, php, python, ruby |
| security | audit_tools, freeipa, kerberos_client, keycloak, ldap_client, osquery, selinux_apparmor, snort, sssd, suricata, vault, wazuh, zeek |
| storage | ceph, drbd, gluster, iscsi, lustre, minio, nfs, samba |
| system | audit, cron, firewall, kernel, postfix, rsyslog, sendmail, ssh, systemd |
| virtualization | libvirt, qemu, vbox |
| web | apache, caddy, glassfish, jetty, nginx, springboot, tomcat, weblogic, websphere |
| other | unknown (fallback for all unclassified processes) |

### Custom subgroups

Add entries to `data/subgroups.toml` without modifying source code:

```toml
subgroups = [
  { group = "db", subgroup = "mysql", matches = [
    "mysqld",
    "mariadbd",
  ] },
  { group = "web", subgroup = "tomcat", matches = [
    "tomcat",
  ], cmdline_matches = [
    "org.apache.catalina.startup.Bootstrap",
    "catalina.sh",
  ] },
]
```

`cmdline_matches` is the right tool for JVM-based services where `/proc/<pid>/comm` is always `java` — match on the
main class or startup script instead.

File search order: `./subgroups.toml` → `/etc/herakles-node-exporter/subgroups.toml`.

List loaded subgroups at runtime:

```bash
herakles-node-exporter subgroups
herakles-node-exporter subgroups --group db --verbose
```

---

## Metrics Reference

### Exporter Self-Metrics

| Metric | Type | Description |
|---|---|---|
| `herakles_exporter_scrape_duration_seconds` | Gauge | Time spent serving `/metrics` (reading from cache) |
| `herakles_exporter_processes` | Gauge | Number of processes currently exported |
| `herakles_exporter_cache_update_duration_seconds` | Gauge | Time spent on last background cache update |
| `herakles_exporter_cache_update_success` | Gauge | 1 if last cache update succeeded, 0 if failed |
| `herakles_exporter_cache_updating` | Gauge | 1 if cache update is in progress, 0 if idle |

### Process Group Metrics

These are the primary metrics for alerting and dashboards. All aggregated at `(group, subgroup)` level — no PID labels.

| Metric | Type | Description | Labels |
|---|---|---|---|
| `herakles_group_memory_rss_bytes` | Gauge | Sum of RSS bytes | group, subgroup |
| `herakles_group_memory_pss_bytes` | Gauge | Sum of PSS bytes (deduplicates shared memory) | group, subgroup |
| `herakles_group_memory_swap_bytes` | Gauge | Sum of swap bytes | group, subgroup |
| `herakles_group_cpu_usage_ratio` | Gauge | CPU usage ratio (0.0–1.0) | group, subgroup |
| `herakles_group_cpu_seconds` | Gauge | CPU time by mode | group, subgroup, mode |
| `herakles_group_blkio_read_bytes` | Gauge | Bytes read | group, subgroup |
| `herakles_group_blkio_write_bytes` | Gauge | Bytes written | group, subgroup |
| `herakles_group_blkio_read_syscalls` | Gauge | Read syscalls | group, subgroup |
| `herakles_group_blkio_write_syscalls` | Gauge | Write syscalls | group, subgroup |
| `herakles_group_net_rx_bytes` | Gauge | Bytes received (eBPF) | group, subgroup |
| `herakles_group_net_tx_bytes` | Gauge | Bytes transmitted (eBPF) | group, subgroup |
| `herakles_group_net_connections` | Gauge | Network connections by protocol (eBPF) | group, subgroup, proto |

### System Memory Metrics

| Metric | Type | Description |
|---|---|---|
| `herakles_system_memory_total_bytes` | Gauge | Total system memory |
| `herakles_system_memory_available_bytes` | Gauge | Available system memory |
| `herakles_system_memory_used_ratio` | Gauge | Memory used ratio (0.0–1.0) |
| `herakles_system_memory_cached_bytes` | Gauge | Page cache memory |
| `herakles_system_memory_buffers_bytes` | Gauge | Buffer cache memory |
| `herakles_system_swap_used_ratio` | Gauge | Swap used ratio (0.0–1.0) |
| `herakles_system_memory_psi_wait_seconds` | Gauge | Memory pressure stall time |

### System CPU Metrics

| Metric | Type | Description | Labels |
|---|---|---|---|
| `herakles_system_cpu_usage_ratio` | Gauge | CPU usage ratio (0.0–1.0) | — |
| `herakles_system_cpu_idle_ratio` | Gauge | CPU idle ratio (0.0–1.0) | — |
| `herakles_system_cpu_iowait_ratio` | Gauge | CPU iowait ratio (0.0–1.0) | — |
| `herakles_system_cpu_steal_ratio` | Gauge | CPU steal ratio (0.0–1.0) | — |
| `herakles_system_cpu_load_1` | Gauge | Load average (1 min) | — |
| `herakles_system_cpu_load_5` | Gauge | Load average (5 min) | — |
| `herakles_system_cpu_load_15` | Gauge | Load average (15 min) | — |
| `herakles_system_cpu_psi_wait_seconds` | Gauge | CPU pressure stall time | — |

### Disk I/O Metrics

| Metric | Type | Description | Labels |
|---|---|---|---|
| `herakles_system_disk_read_bytes` | Gauge | Bytes read | device |
| `herakles_system_disk_write_bytes` | Gauge | Bytes written | device |
| `herakles_system_disk_io_time_seconds` | Gauge | Time doing I/Os | device |
| `herakles_system_disk_queue_depth` | Gauge | I/O operations currently in progress | device |
| `herakles_system_disk_psi_wait_seconds` | Gauge | I/O pressure stall time | — |

### Filesystem Metrics

| Metric | Type | Description | Labels |
|---|---|---|---|
| `herakles_system_filesystem_size_bytes` | Gauge | Total filesystem size | device, mountpoint, fstype |
| `herakles_system_filesystem_avail_bytes` | Gauge | Space available to non-root users | device, mountpoint, fstype |
| `herakles_system_filesystem_files` | Gauge | Total inodes | device, mountpoint, fstype |
| `herakles_system_filesystem_files_free` | Gauge | Free inodes | device, mountpoint, fstype |

### Network Metrics

| Metric | Type | Description | Labels |
|---|---|---|---|
| `herakles_system_net_rx_bytes` | Gauge | Bytes received | iface |
| `herakles_system_net_tx_bytes` | Gauge | Bytes transmitted | iface |
| `herakles_system_net_rx_errors` | Gauge | Receive errors | iface |
| `herakles_system_net_tx_errors` | Gauge | Transmit errors | iface |
| `herakles_system_net_drops` | Gauge | Dropped packets | iface, direction |

### TCP Connection State Metrics

Always registered in the Prometheus registry. Only updated when `ebpf` is compiled in and `enable_tcp_tracking` is true.

| Metric | Type | Description |
|---|---|---|
| `herakles_system_tcp_connections_established` | Gauge | ESTABLISHED |
| `herakles_system_tcp_connections_syn_sent` | Gauge | SYN_SENT |
| `herakles_system_tcp_connections_syn_recv` | Gauge | SYN_RECV |
| `herakles_system_tcp_connections_fin_wait1` | Gauge | FIN_WAIT1 |
| `herakles_system_tcp_connections_fin_wait2` | Gauge | FIN_WAIT2 |
| `herakles_system_tcp_connections_time_wait` | Gauge | TIME_WAIT |
| `herakles_system_tcp_connections_close` | Gauge | CLOSE |
| `herakles_system_tcp_connections_close_wait` | Gauge | CLOSE_WAIT |
| `herakles_system_tcp_connections_last_ack` | Gauge | LAST_ACK |
| `herakles_system_tcp_connections_listen` | Gauge | LISTEN |
| `herakles_system_tcp_connections_closing` | Gauge | CLOSING |

### Hardware & Host Metrics

| Metric | Type | Description | Labels |
|---|---|---|---|
| `herakles_system_cpu_temp_celsius` | Gauge | Temperature in Celsius | sensor |
| `herakles_system_uptime_seconds` | Gauge | System uptime | — |
| `herakles_system_boot_time_seconds` | Gauge | Boot time (Unix timestamp) | — |
| `herakles_system_uname_info` | Gauge | Kernel/arch info (always 1) | sysname, release, version, machine |
| `herakles_system_context_switches` | Gauge | Context switches | — |
| `herakles_system_forks` | Gauge | Forks since boot | — |
| `herakles_system_open_fds` | Gauge | Open file descriptors system-wide | state |
| `herakles_system_entropy_bytes` | Gauge | Available entropy in bytes | — |

### eBPF Subsystem Metrics

Always registered. Only updated when the `ebpf` feature is compiled in and eBPF initialization succeeds at runtime.

| Metric | Type | Description |
|---|---|---|
| `herakles_ebpf_events_processed_total` | Counter | Total eBPF events processed |
| `herakles_ebpf_events_dropped_total` | Counter | Total eBPF events dropped |
| `herakles_ebpf_maps_count` | Gauge | Number of eBPF programs currently loaded |
| `herakles_ebpf_cpu_seconds_total` | Counter | Total CPU time used by eBPF programs |

---

## Installation

See [wiki/Installation.md](wiki/Installation.md).

---

## Configuration

Configuration is loaded from the first file found in this order, then merged with CLI flags (CLI takes precedence):

1. `--config <path>` if specified
2. `/etc/herakles-node-exporter/config.yaml` (also `.yml`, `.json`)
3. `./herakles-node-exporter.yaml` (also `.yml`, `.json`)

Use `--no-config` to ignore all config files. Use `--show-config` to print the effective merged configuration.

### Minimal

```yaml
port: 9215
bind: "0.0.0.0"
cache_ttl: 30
```

### Production

```yaml
port: 9215
bind: "0.0.0.0"
cache_ttl: 30

# Process filtering
min_uss_kb: 0
parallelism: 4
max_processes: 2000

# Metrics collection
enable_rss: true
enable_pss: true
enable_uss: true
enable_cpu: true

# Group filtering — uncomment to restrict which groups are active
# search_mode: "include"
# search_groups: ["db", "web", "cache"]

# "other" group handling
disable_others: false
top_n_subgroup: 3      # Top-N processes shown in /details per subgroup
top_n_others: 10       # Top-N processes shown in /details for "other"
details_top_n: 5       # Total Top-N shown in /details view

# Collectors
enable_filesystem_collector: true
enable_thermal_collector: true
enable_psi_collector: true

# Ring buffer — controls /details historical data depth
ringsize:
  max_memory_mb: 15
  interval_seconds: 30
  min_entries_per_subgroup: 10
  max_entries_per_subgroup: 120

# eBPF
enable_ebpf: true
enable_ebpf_network: true
enable_ebpf_disk: true
enable_tcp_tracking: true

# TLS (disabled by default)
enable_tls: false
# tls_cert_path: "/etc/herakles-node-exporter/cert.pem"
# tls_key_path: "/etc/herakles-node-exporter/key.pem"

log_level: "info"
```

### Configuration reference

| Field | Type | Default | Description |
|---|---|---|---|
| `port` | u16 | 9215 | HTTP listen port |
| `bind` | string | "0.0.0.0" | Bind address |
| `cache_ttl` | u64 | 30 | Cache TTL in seconds |
| `min_uss_kb` | u64 | 0 | Minimum USS in KB to include a process |
| `include_names` | [string] | — | Include only processes with these names |
| `exclude_names` | [string] | — | Exclude processes with these names |
| `parallelism` | usize | auto | Rayon thread pool size (0 = auto) |
| `max_processes` | usize | unlimited | Maximum processes to scan |
| `io_buffer_kb` | usize | 256 | Buffer size for generic `/proc` reads |
| `smaps_buffer_kb` | usize | 512 | Buffer size for `/proc/<pid>/smaps` |
| `smaps_rollup_buffer_kb` | usize | 256 | Buffer size for `/proc/<pid>/smaps_rollup` |
| `enable_rss` | bool | true | Collect RSS memory |
| `enable_pss` | bool | true | Collect PSS memory |
| `enable_uss` | bool | true | Collect USS memory |
| `enable_cpu` | bool | true | Collect CPU metrics |
| `search_mode` | string | — | "include" or "exclude" for group filtering |
| `search_groups` | [string] | — | Group names to include/exclude |
| `search_subgroups` | [string] | — | Subgroup names to include/exclude |
| `disable_others` | bool | false | Ignore all other/unknown processes entirely |
| `top_n_subgroup` | usize | 3 | Top-N processes per subgroup in `/details` |
| `top_n_others` | usize | 10 | Top-N processes for the "other" group in `/details` |
| `details_top_n` | usize | 5 | Top-N shown in `/details` view |
| `enable_health` | bool | true | Enable `/health` endpoint |
| `enable_telemetry` | bool | true | Enable `herakles_exporter_*` self-metrics |
| `enable_default_collectors` | bool | true | Enable generic system collectors |
| `enable_pprof` | bool | false | Enable `/debug/pprof` endpoints |
| `enable_filesystem_collector` | bool | true | Filesystem metrics |
| `enable_thermal_collector` | bool | true | Temperature metrics |
| `enable_psi_collector` | bool | true | PSI pressure stall metrics |
| `log_level` | string | "info" | off / error / warn / info / debug / trace |
| `enable_file_logging` | bool | false | Enable file logging |
| `log_file` | path | — | Log file path |
| `test_data_file` | path | — | Use synthetic JSON data instead of `/proc` |
| `enable_tls` | bool | false | Enable HTTPS |
| `tls_cert_path` | string | — | TLS certificate (PEM) |
| `tls_key_path` | string | — | TLS private key (PEM) |
| `enable_ebpf` | bool | true | Enable eBPF subsystem |
| `enable_ebpf_network` | bool | true | eBPF network I/O tracking |
| `enable_ebpf_disk` | bool | true | eBPF disk I/O tracking |
| `enable_tcp_tracking` | bool | true | TCP connection state tracking via eBPF |
| `ringsize.max_memory_mb` | usize | 15 | Maximum ring buffer memory for `/details` history |
| `ringsize.interval_seconds` | u64 | 30 | Ring buffer sampling interval |
| `ringsize.min_entries_per_subgroup` | usize | 10 | Minimum history entries per subgroup |
| `ringsize.max_entries_per_subgroup` | usize | 120 | Maximum history entries per subgroup |

---

## eBPF

The `ebpf` feature is compiled in by default and provides:

- Per-process network I/O → `herakles_group_net_rx/tx_bytes_total`
- Per-process block I/O → `herakles_group_blkio_*`
- TCP connection state tracking → `herakles_system_tcp_connections_*`
- eBPF self-monitoring → `herakles_ebpf_*`

### Requirements

| Requirement | Detail |
|---|---|
| Linux kernel | ≥ 4.18 with BTF enabled |
| BTF | `/sys/kernel/btf/vmlinux` must exist |
| Capabilities | `CAP_BPF` + `CAP_PERFMON`, or root |
| Build: clang | ≥ 10 |
| Build: bpftool | any recent version |
| Build: libbpf | pulled automatically by Cargo |

### Graceful degradation

eBPF initialization failure is non-fatal. If `EbpfManager::new()` returns an error, the exporter logs a warning and
continues. All non-eBPF metrics remain fully functional. Check `/health` to see whether eBPF initialized successfully.

```
INFO  ✅ eBPF programs loaded and attached successfully
WARN  ⚠️  Failed to initialize eBPF: [reason] - running without eBPF metrics
```

### Troubleshooting

```bash
# BTF availability
ls -la /sys/kernel/btf/vmlinux

# Kernel version
uname -r

# Capabilities
capsh --print | grep -E 'cap_bpf|cap_perfmon'

# Build tools
clang --version && bpftool version

# Runtime requirement check
herakles-node-exporter check-requirements --ebpf

# eBPF status at runtime
curl http://localhost:9215/health
```

---

## Example PromQL Queries

```promql
# RSS memory for all postgres processes across all hosts
herakles_group_memory_rss_bytes{subgroup="postgres"}

# PSS memory by group — deduplicates shared memory contributions
herakles_group_memory_pss_bytes{group="db"}

# Disk read throughput per group (bytes/sec)
rate(herakles_group_blkio_read_bytes[5m])

# Disk write throughput per block device
rate(herakles_system_disk_write_bytes[5m])

# Filesystem usage per mountpoint (0.0–1.0)
1 - (herakles_system_filesystem_avail_bytes / herakles_system_filesystem_size_bytes)

# Filesystems below 10% free space
(herakles_system_filesystem_avail_bytes / herakles_system_filesystem_size_bytes) < 0.1

# Network receive throughput per interface
rate(herakles_system_net_rx_bytes[5m])

# Network transmit throughput per process group (requires eBPF)
rate(herakles_group_net_tx_bytes[5m])

# CPU pressure stall rate — fraction of time stalled on CPU
rate(herakles_system_cpu_psi_wait_seconds[5m])

# Memory pressure stall rate
rate(herakles_system_memory_psi_wait_seconds[5m])

# I/O pressure stall rate
rate(herakles_system_disk_psi_wait_seconds[5m])

# Alert: system memory pressure above 90%
herakles_system_memory_used_ratio > 0.9

# Alert: db group memory growing faster than 100MB/min
rate(herakles_group_memory_rss_bytes{group="db"}[5m]) * 60 > 1e8
```

---

## CLI Reference

```
Usage: herakles-node-exporter [OPTIONS] [COMMAND]

Commands:
  check               Validate configuration and system requirements
  config              Generate configuration files
  test                Test metrics collection
  subgroups           List available process subgroups
  generate-testdata   Generate synthetic test data JSON file
  install             Install system-wide with systemd service
  uninstall           Uninstall system-wide installation
  check-requirements  Check runtime requirements and permissions
```

| Flag | Short | Default | Description |
|---|---|---|---|
| `--port <PORT>` | `-p` | — | HTTP listen port |
| `--bind <BIND>` | — | — | Bind address |
| `--log-level <LEVEL>` | — | info | off / error / warn / info / debug / trace |
| `--config <FILE>` | `-c` | — | Config file (YAML/JSON/TOML) |
| `--no-config` | — | false | Ignore all config files |
| `--show-config` | — | false | Print effective merged config and exit |
| `--show-user-config` | — | false | Print loaded user config file and path, then exit |
| `--config-format <FMT>` | — | yaml | Output format for `--show-config*`: yaml, json, toml |
| `--check-config` | — | false | Validate config and exit (rc=1 on error) |
| `--debug` | — | false | Enable `/debug/pprof` endpoints |
| `--cache-ttl <SECS>` | — | — | Override cache TTL |
| `--disable-health` | — | false | Disable `/health` endpoint |
| `--disable-telemetry` | — | false | Disable `herakles_exporter_*` self-metrics |
| `--disable-default-collectors` | — | false | Disable generic system collectors |
| `--io-buffer-kb <KB>` | — | — | Buffer size for generic `/proc` reads |
| `--smaps-buffer-kb <KB>` | — | — | Buffer size for `/proc/<pid>/smaps` |
| `--smaps-rollup-buffer-kb <KB>` | — | — | Buffer size for `/proc/<pid>/smaps_rollup` |
| `--min-uss-kb <KB>` | — | — | Minimum USS in KB to include a process |
| `--include-names <NAMES>` | — | — | Include only these process names (comma-separated) |
| `--exclude-names <NAMES>` | — | — | Exclude these process names (comma-separated) |
| `--parallelism <N>` | — | — | Parallel processing threads (0 = auto) |
| `--max-processes <N>` | — | — | Maximum processes to scan |
| `--top-n-subgroup <N>` | — | — | Top-N processes per subgroup in `/details` |
| `--top-n-others <N>` | — | — | Top-N for the "other" group in `/details` |
| `--test-data-file <FILE>` | `-t` | — | Synthetic JSON test data instead of `/proc` |
| `--enable-tls` | — | false | Enable HTTPS |
| `--tls-cert <FILE>` | — | — | TLS certificate (PEM) |
| `--tls-key <FILE>` | — | — | TLS private key (PEM) |
| `--enable-ebpf` | — | false | Enable eBPF (kernel ≥ 4.18, BTF, CAP_BPF/CAP_PERFMON) |
| `--enable-ebpf-network` | — | false | Enable eBPF network I/O tracking |
| `--disable-ebpf-network` | — | false | Disable eBPF network I/O tracking |
| `--enable-ebpf-disk` | — | false | Enable eBPF disk I/O tracking |
| `--disable-ebpf-disk` | — | false | Disable eBPF disk I/O tracking |
| `--enable-tcp-tracking` | — | false | Enable TCP state tracking via eBPF |
| `--disable-tcp-tracking` | — | false | Disable TCP state tracking via eBPF |

---

## Running as Root

Reading `/proc/<pid>/smaps_rollup` for processes owned by other users requires root privileges. This file provides
accurate USS (Unique Set Size) figures. Without root, USS data for root-owned processes is unavailable and those
processes are silently excluded from group memory metrics.

After eBPF programs are loaded and pinned, the process attempts to drop to the `herakles` system user if it exists
(`drop_privileges()` in `src/main.rs`). If the `herakles` user does not exist, the process continues as root — which
is the recommended production configuration for complete multi-user system monitoring.

Check effective user before debugging missing processes:

```bash
ps aux | grep herakles-node-exporter
# Should show: root ... herakles-node-exporter
```

---

## Systemd Service

```ini
[Unit]
Description=Herakles Node Exporter
Documentation=https://github.com/cansp-dev/herakles-node-exporter
After=network.target

[Service]
Type=simple
User=root
ExecStart=/usr/local/bin/herakles-node-exporter
Restart=on-failure
RestartSec=5s
ProtectSystem=strict
ReadOnlyPaths=/proc
PrivateTmp=true
NoNewPrivileges=false

[Install]
WantedBy=multi-user.target
```

---

## Docker Compose

```yaml
services:
  herakles-node-exporter:
    image: herakles-node-exporter:latest
    container_name: herakles-node-exporter
    pid: host
    volumes:
      - /proc:/proc:ro
    ports:
      - "9215:9215"
    restart: unless-stopped
    healthcheck:
      test: ["CMD", "wget", "-q", "-O", "/dev/null", "http://localhost:9215/health"]
      interval: 30s
      timeout: 5s
      retries: 3
      start_period: 5s
```

---

## License

Licensed under the [Apache 2.0](LICENSE) license.

## Author

Michael Moll — [exporter@herakles.now](mailto:exporter@herakles.now)
