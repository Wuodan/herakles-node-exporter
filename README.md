# Herakles Node Exporter

[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)
[![Prometheus](https://img.shields.io/badge/prometheus-compatible-red.svg)](https://prometheus.io)

A Prometheus exporter for Linux system metrics that aggregates per-process resource usage into named process groups, exposing clean group-level time series without per-PID cardinality.

---

## Why Herakles?

Most process-aware Prometheus exporters expose per-PID label dimensions. This causes two well-understood problems: label cardinality explodes as processes restart and accumulate new PIDs, and the resulting time series are too fine-grained to be actionable in alert rules. An alert that fires on `process{pid="12345"}` is operationally useless because the PID changes every restart and the label set varies across hosts.

Herakles solves this by classifying every running process into a named (`group`, `subgroup`) pair at scrape time. The metrics at `/metrics` carry only these two stable label dimensions — never a PID, never a raw command name. `herakles_group_memory_rss_bytes{group="db",subgroup="postgres"}` means the same thing on every host and survives any number of postgres restarts without any stale series accumulation. This makes it safe to write recording rules and multi-host federation queries over group metrics without cardinality concerns.

The human operator, however, needs the opposite: when an alert fires on `herakles_group_memory_rss_bytes`, they want to know _which_ postgres process is responsible. For this, `/details` and `/html/details` intentionally expose high-cardinality data — PIDs, USS per process, CPU percentages — that would be unsafe in Prometheus but are perfectly appropriate in a forensic endpoint that is read by humans or automation on demand, never scraped continuously. This separation is architectural: the Prometheus scrape path and the operator inspection path are different endpoints with different contracts.

The cache model follows from the same reasoning. `/metrics` serves data from an in-memory cache that is refreshed on-demand at most every 5 seconds (`CACHE_UPDATE_THROTTLE_SECS`). This means a Prometheus scrape never blocks on `/proc` I/O: the scrape handler reads from cache, while a background tokio task asynchronously updates it. The cost is that metrics may lag by up to one cache TTL — an acceptable trade-off for a monitoring system where staleness on the order of seconds is irrelevant.

The `group`/`subgroup` classification is implemented as a static lookup table compiled from `data/subgroups.toml`. Process names that match no entry fall into `{group="other", subgroup="unknown"}`. Custom rules can be added to the TOML without modifying source code.

---

## The Endpoint Architecture

Herakles exposes two distinct categories of HTTP endpoint: machine-readable endpoints designed for Prometheus and automation, and human-readable endpoints designed for operator inspection.

### `/metrics` — Prometheus scrape target

`GET /metrics` returns the full metric set in Prometheus text exposition format. This is the only endpoint that Prometheus should scrape. It contains no per-PID labels; all process-level data has been aggregated into `(group, subgroup)` pairs before encoding. On each request, the handler checks whether the cache is stale (older than 5 seconds) and, if so, spawns a background task to refresh it, then immediately returns the current cached data. Scrape latency is therefore bounded by lock acquisition time, not `/proc` scan time.

The intended usage pattern: configure Prometheus to scrape `http://<host>:9215/metrics`. When an alert fires — for example, `herakles_group_memory_rss_bytes{group="db",subgroup="postgres"} > 8e9` — open `/html/details?subgroup=postgres` on the affected host to find which individual postgres process is responsible.

### `/details` and `/html/details` — Forensic process view

`GET /details` (plain text) and `GET /html/details` (HTML) expose per-process data from the in-memory cache. Both endpoints accept a `?subgroup=<name>` query parameter to filter by subgroup. The details handler implements temporal zone classification: processes younger than 5 minutes are in the `Live` phase, 5–60 minutes in `Stabilization`, and older than 60 minutes in `Historical`. For each phase, the handler computes anomaly severity based on deviation from a rolling baseline, surfacing processes whose memory or CPU consumption is growing unexpectedly.

This data is intentionally not in `/metrics` because it contains PIDs and per-process metrics that are high-cardinality and change with every restart. The details endpoints are for human operators and automated runbooks, not continuous Prometheus scraping.

**Workflow**: alert fires on group metric → open `/html/details?subgroup=<name>` to identify the responsible process → examine PID, USS, CPU, and I/O rates → act.

### Other endpoints

| Method | Path | Audience | Description |
|--------|------|----------|-------------|
| GET | `/` | Human | HTML landing page with all endpoint links and exporter uptime |
| GET | `/health` | Both | Plain-text health check; returns HTTP 200 if cache is valid, 503 otherwise |
| GET | `/config` | Human/Automation | Current effective configuration in plain text |
| GET | `/subgroups` | Human/Automation | All loaded (group, subgroup) pairs with their process name patterns |
| GET | `/doc` | Human | Inline plain-text documentation |
| GET | `/docs` | Human | HTML documentation |
| GET | `/html` | Human | HTML index |
| GET | `/html/subgroups` | Human | HTML view of subgroup classification table |
| GET | `/html/health` | Human | HTML view of health and buffer statistics |
| GET | `/html/config` | Human | HTML view of current configuration |
| GET | `/html/docs` | Human | HTML documentation |

---

## Process Classification

Every process that appears in `/proc` is classified into a `(group, subgroup)` pair by matching its executable name against the entries in `data/subgroups.toml`. The matching logic checks `matches` (process name prefixes) and `cmdline_matches` (command-line substrings). A process that matches no entry is assigned `{group="other", subgroup="unknown"}`.

The built-in groups and their subgroups are:

| Group | Subgroups |
|-------|-----------|
| `backup` | bacula, commvault, cohesity, netbackup, networker, rubrik, spectrum_protect, tsm, veeam |
| `cache` | memcached, redis, varnish |
| `cicd` | ansible, chef, gitlab, jenkins, openstack, puppet, saltstack, terraform |
| `container` | containerd, crio, docker, kubelet, podman |
| `db` | cassandra, clickhouse, cockroachdb, couchbase, couchdb, db2, influxdb, mongodb, mssql, mysql, oracle, percona, postgres, rethinkdb, timescaledb, yugabyte |
| `erp` | peoplesoft, sap |
| `logging` | elasticsearch, filebeat, fluentd, graylog, kibana, log_collectors, logstash, splunk |
| `messaging` | activemq, kafka, nats, nsq, pulsar, rabbitmq, zeromq |
| `monitoring` | alertmanager, blackbox, grafana, icinga_nagios, node_exporter, percona, prometheus, snmp, telegraf, thanos, victoriametrics, zabbix |
| `network` | bind, dhcp, haproxy, keepalived, lvs, ntp, proxy_squid, vpn |
| `runtime` | go, java, nodejs, php, python, ruby |
| `security` | audit_tools, freeipa, kerberos_client, keycloak, ldap_client, osquery, selinux_apparmor, snort, sssd, suricata, vault, wazuh, zeek |
| `storage` | ceph, drbd, gluster, iscsi, lustre, minio, nfs, samba |
| `system` | audit, cron, firewall, kernel, postfix, rsyslog, sendmail, ssh, systemd |
| `virtualization` | libvirt, qemu, vbox |
| `web` | apache, caddy, glassfish, jetty, nginx, springboot, tomcat, weblogic, websphere |
| `other` | unknown (fallback for unclassified processes) |

### Custom subgroups

The classification table is defined in `data/subgroups.toml`. Each entry follows this structure:

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

- `group` — coarse category, appears as the `group` label in Prometheus
- `subgroup` — specific service name, appears as the `subgroup` label
- `matches` — list of process name prefixes (matched against `/proc/<pid>/comm`)
- `cmdline_matches` — list of substrings matched against the full command line (useful for JVM processes where `comm` is always `java`)

To control which groups are active at runtime, use `search_mode`, `search_groups`, and `search_subgroups` in the configuration file.

---

## Metrics Reference

### Exporter Self-Metrics

| Metric | Type | Description | Labels |
|--------|------|-------------|--------|
| `herakles_exporter_scrape_duration_seconds` | Gauge | Time spent serving /metrics request (reading from cache) | — |
| `herakles_exporter_processes_total` | Gauge | Number of processes currently exported by herakles-node-exporter | — |
| `herakles_exporter_cache_update_duration_seconds` | Gauge | Time spent updating the process metrics cache in background | — |
| `herakles_exporter_cache_update_success` | Gauge | Whether the last cache update was successful (1) or failed (0) | — |
| `herakles_exporter_cache_updating` | Gauge | Whether cache update is currently in progress (1) or idle (0) | — |

### Process Group Metrics

| Metric | Type | Description | Labels |
|--------|------|-------------|--------|
| `herakles_group_cpu_usage_ratio` | Gauge | CPU usage ratio per group and subgroup (0.0–1.0) | `group`, `subgroup` |
| `herakles_group_cpu_seconds_total` | Counter | Total CPU time in seconds per group, subgroup, and mode | `group`, `subgroup`, `mode` |
| `herakles_group_memory_rss_bytes` | Gauge | Sum of RSS bytes per group and subgroup | `group`, `subgroup` |
| `herakles_group_memory_pss_bytes` | Gauge | Sum of PSS bytes per group and subgroup | `group`, `subgroup` |
| `herakles_group_memory_swap_bytes` | Gauge | Sum of swap bytes per group and subgroup | `group`, `subgroup` |
| `herakles_group_blkio_read_bytes_total` | Counter | Total bytes read per group and subgroup | `group`, `subgroup` |
| `herakles_group_blkio_write_bytes_total` | Counter | Total bytes written per group and subgroup | `group`, `subgroup` |
| `herakles_group_blkio_read_syscalls_total` | Counter | Total read syscalls per group and subgroup | `group`, `subgroup` |
| `herakles_group_blkio_write_syscalls_total` | Counter | Total write syscalls per group and subgroup | `group`, `subgroup` |
| `herakles_group_net_rx_bytes_total` | Counter | Total bytes received per group and subgroup (eBPF) | `group`, `subgroup` |
| `herakles_group_net_tx_bytes_total` | Counter | Total bytes transmitted per group and subgroup (eBPF) | `group`, `subgroup` |
| `herakles_group_net_connections_total` | Gauge | Total network connections per group, subgroup, and protocol | `group`, `subgroup`, `proto` |

### System Memory Metrics

| Metric | Type | Description | Labels |
|--------|------|-------------|--------|
| `herakles_system_memory_total_bytes` | Gauge | Total system memory in bytes | — |
| `herakles_system_memory_available_bytes` | Gauge | Available system memory in bytes | — |
| `herakles_system_memory_used_ratio` | Gauge | System memory used ratio (0.0–1.0) | — |
| `herakles_system_memory_cached_bytes` | Gauge | Page cache memory in bytes | — |
| `herakles_system_memory_buffers_bytes` | Gauge | Buffer cache memory in bytes | — |
| `herakles_system_swap_used_ratio` | Gauge | System swap memory used ratio (0.0–1.0) | — |
| `herakles_system_memory_psi_wait_seconds_total` | Counter | Total memory pressure stall time in seconds | — |

### System CPU Metrics

| Metric | Type | Description | Labels |
|--------|------|-------------|--------|
| `herakles_system_cpu_usage_ratio` | Gauge | System CPU usage ratio (0.0–1.0) | — |
| `herakles_system_cpu_idle_ratio` | Gauge | System CPU idle ratio (0.0–1.0) | — |
| `herakles_system_cpu_iowait_ratio` | Gauge | System CPU iowait ratio (0.0–1.0) | — |
| `herakles_system_cpu_steal_ratio` | Gauge | System CPU steal ratio (0.0–1.0) | — |
| `herakles_system_cpu_load_1` | Gauge | System load average over 1 minute | — |
| `herakles_system_cpu_load_5` | Gauge | System load average over 5 minutes | — |
| `herakles_system_cpu_load_15` | Gauge | System load average over 15 minutes | — |
| `herakles_system_cpu_psi_wait_seconds_total` | Counter | Total CPU pressure stall time in seconds | — |

### Disk I/O Metrics

| Metric | Type | Description | Labels |
|--------|------|-------------|--------|
| `herakles_system_disk_read_bytes_total` | Counter | Total bytes read from disk device | `device` |
| `herakles_system_disk_write_bytes_total` | Counter | Total bytes written to disk device | `device` |
| `herakles_system_disk_io_time_seconds_total` | Counter | Total time spent doing I/Os in seconds | `device` |
| `herakles_system_disk_queue_depth` | Gauge | Number of I/O operations currently in progress for disk device | `device` |
| `herakles_system_disk_psi_wait_seconds_total` | Counter | Total I/O pressure stall time in seconds | — |

### Filesystem Metrics

| Metric | Type | Description | Labels |
|--------|------|-------------|--------|
| `herakles_system_filesystem_avail_bytes` | Gauge | Filesystem space available to non-root users in bytes | `device`, `mountpoint`, `fstype` |
| `herakles_system_filesystem_size_bytes` | Gauge | Filesystem total size in bytes | `device`, `mountpoint`, `fstype` |
| `herakles_system_filesystem_files` | Gauge | Filesystem total file nodes | `device`, `mountpoint`, `fstype` |
| `herakles_system_filesystem_files_free` | Gauge | Filesystem free file nodes | `device`, `mountpoint`, `fstype` |

### Network Metrics

| Metric | Type | Description | Labels |
|--------|------|-------------|--------|
| `herakles_system_net_rx_bytes_total` | Counter | Total bytes received per network interface | `iface` |
| `herakles_system_net_tx_bytes_total` | Counter | Total bytes transmitted per network interface | `iface` |
| `herakles_system_net_rx_errors_total` | Counter | Total receive errors per network interface | `iface` |
| `herakles_system_net_tx_errors_total` | Counter | Total transmit errors per network interface | `iface` |
| `herakles_system_net_drops_total` | Counter | Total dropped packets per network interface and direction | `iface`, `direction` |

### TCP Connection State Metrics

These metrics are populated by eBPF when the `ebpf` feature is compiled in and `enable_tcp_tracking` is `true`. The metrics are always registered in the Prometheus registry but only updated by the eBPF subsystem.

| Metric | Type | Description | Labels |
|--------|------|-------------|--------|
| `herakles_system_tcp_connections_established` | Gauge | Number of TCP connections in ESTABLISHED state | — |
| `herakles_system_tcp_connections_syn_sent` | Gauge | Number of TCP connections in SYN_SENT state | — |
| `herakles_system_tcp_connections_syn_recv` | Gauge | Number of TCP connections in SYN_RECV state | — |
| `herakles_system_tcp_connections_fin_wait1` | Gauge | Number of TCP connections in FIN_WAIT1 state | — |
| `herakles_system_tcp_connections_fin_wait2` | Gauge | Number of TCP connections in FIN_WAIT2 state | — |
| `herakles_system_tcp_connections_time_wait` | Gauge | Number of TCP connections in TIME_WAIT state | — |
| `herakles_system_tcp_connections_close` | Gauge | Number of TCP connections in CLOSE state | — |
| `herakles_system_tcp_connections_close_wait` | Gauge | Number of TCP connections in CLOSE_WAIT state | — |
| `herakles_system_tcp_connections_last_ack` | Gauge | Number of TCP connections in LAST_ACK state | — |
| `herakles_system_tcp_connections_listen` | Gauge | Number of TCP connections in LISTEN state | — |
| `herakles_system_tcp_connections_closing` | Gauge | Number of TCP connections in CLOSING state | — |

### Hardware & Host Metrics

| Metric | Type | Description | Labels |
|--------|------|-------------|--------|
| `herakles_system_cpu_temp_celsius` | Gauge | CPU/sensor temperature in Celsius | `sensor` |
| `herakles_system_uptime_seconds` | Gauge | System uptime in seconds | — |
| `herakles_system_boot_time_seconds` | Gauge | System boot time as Unix timestamp | — |
| `herakles_system_uname_info` | Gauge | System information from uname | `sysname`, `release`, `version`, `machine` |
| `herakles_system_context_switches_total` | Counter | Total number of context switches | — |
| `herakles_system_forks_total` | Counter | Total number of forks since boot | — |
| `herakles_system_open_fds` | Gauge | Number of file descriptors system-wide | `state` |
| `herakles_system_entropy_bits` | Gauge | Available entropy in bits | — |

### eBPF Metrics

These metrics track the health of the eBPF subsystem itself. They are always registered in the Prometheus registry but only updated when the `ebpf` Cargo feature is compiled in and eBPF initialization succeeds at runtime.

| Metric | Type | Description | Labels |
|--------|------|-------------|--------|
| `herakles_ebpf_events_processed_total` | Counter | Total number of eBPF events processed | — |
| `herakles_ebpf_events_dropped_total` | Counter | Total number of eBPF events dropped | — |
| `herakles_ebpf_maps_count` | Gauge | Number of eBPF programs currently loaded | — |
| `herakles_ebpf_cpu_seconds_total` | Counter | Total CPU time used by eBPF programs in seconds | — |

---

## Installation

### Standard build (eBPF enabled by default)

The `ebpf` feature is the default. Building with eBPF requires `clang`, `bpftool`, and a kernel with BTF support (`/sys/kernel/btf/vmlinux`).

```bash
# Release build with eBPF (default)
make release

# Or directly with cargo
cargo build --release

# Binary is placed in binary/herakles-node-exporter
```

### Build without eBPF

```bash
make build CARGOFLAGS='--no-default-features'
# Or
cargo build --release --no-default-features
```

### System-wide installation

The `install` subcommand copies the binary to the system path and optionally enables a systemd service:

```bash
sudo ./herakles-node-exporter install
# Skip systemd service setup:
sudo ./herakles-node-exporter install --no-service
# Force reinstall over existing installation:
sudo ./herakles-node-exporter install --force
```

To uninstall:

```bash
sudo ./herakles-node-exporter uninstall
```

### Docker

The Dockerfile expects a pre-built statically linked musl binary named `herakles-node-exporter` in the build context (produced by a CI pipeline). It uses `alpine:3.19` as the base image and runs as the `herakles` user (uid=1000).

```bash
# Build (requires pre-built binary in current directory)
docker build -t herakles-node-exporter:latest .

# Run — /proc must be bind-mounted for full host monitoring
docker run -d \
  --name herakles-node-exporter \
  --pid=host \
  -v /proc:/proc:ro \
  -p 9215:9215 \
  herakles-node-exporter:latest
```

> **Note:** The container image runs as the `herakles` user. Without `--pid=host` and a `/proc` bind-mount, only processes visible to that user will be monitored. See the Docker Compose section for a complete example.

---

## Configuration

Configuration is loaded from the first file found in this search order, then merged with CLI flags (CLI takes precedence):

1. `/etc/herakles/node-exporter.yaml`
2. `/etc/herakles/node-exporter.yml`
3. `/etc/herakles/node-exporter.json`
4. `./herakles-node-exporter.yaml`
5. `./herakles-node-exporter.yml`
6. `./herakles-node-exporter.json`

YAML, JSON, and TOML formats are all supported. Use `--config <path>` to specify an explicit path, or `--no-config` to ignore all config files.

### Minimal configuration

```yaml
port: 9215
bind: "0.0.0.0"
cache_ttl: 30
```

### Production configuration

```yaml
# Server
port: 9215
bind: "0.0.0.0"
cache_ttl: 30

# Process filtering
min_uss_kb: 0
parallelism: 4
max_processes: 2000

# Metrics
enable_rss: true
enable_pss: true
enable_uss: true
enable_cpu: true

# Group filtering (optional — include only these groups)
# search_mode: "include"
# search_groups: ["db", "web", "cache"]

# Disable "other" group entirely
disable_others: false
top_n_subgroup: 3
top_n_others: 10
details_top_n: 5

# Feature flags
enable_health: true
enable_telemetry: true
enable_default_collectors: true
enable_pprof: false

# Collectors
enable_filesystem_collector: true
enable_thermal_collector: true
enable_psi_collector: true

# eBPF
enable_ebpf: true
enable_ebpf_network: true
enable_ebpf_disk: true
enable_tcp_tracking: true

# Ringbuffer (for /details historical data)
ringsize:
  max_memory_mb: 15
  interval_seconds: 30
  min_entries_per_subgroup: 10
  max_entries_per_subgroup: 120

# TLS (optional)
enable_tls: false
# tls_cert_path: "/etc/herakles/cert.pem"
# tls_key_path: "/etc/herakles/key.pem"

# Logging
log_level: "info"
enable_file_logging: false
```

### Configuration fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `port` | `u16` | `9215` | HTTP listen port |
| `bind` | `string` | `"0.0.0.0"` | Bind address |
| `cache_ttl` | `u64` | `30` | Cache TTL in seconds |
| `min_uss_kb` | `u64` | `0` | Minimum USS in KB to include process |
| `include_names` | `[string]` | — | Include only processes with these names |
| `exclude_names` | `[string]` | — | Exclude processes with these names |
| `parallelism` | `usize` | auto | Rayon thread pool size (0 = auto) |
| `max_processes` | `usize` | unlimited | Maximum processes to scan |
| `io_buffer_kb` | `usize` | `256` | Buffer size for generic /proc reads |
| `smaps_buffer_kb` | `usize` | `512` | Buffer size for /proc/<pid>/smaps |
| `smaps_rollup_buffer_kb` | `usize` | `256` | Buffer size for /proc/<pid>/smaps_rollup |
| `enable_health` | `bool` | `true` | Enable /health endpoint |
| `enable_telemetry` | `bool` | `true` | Enable herakles_exporter_* self-metrics |
| `enable_default_collectors` | `bool` | `true` | Enable generic system collectors |
| `enable_pprof` | `bool` | `false` | Enable /debug/pprof endpoints |
| `log_level` | `string` | `"info"` | Log level (off/error/warn/info/debug/trace) |
| `enable_file_logging` | `bool` | `false` | Enable file logging |
| `log_file` | `path` | — | Log file path |
| `search_mode` | `string` | — | `"include"` or `"exclude"` for group filtering |
| `search_groups` | `[string]` | — | Group names to include/exclude |
| `search_subgroups` | `[string]` | — | Subgroup names to include/exclude |
| `disable_others` | `bool` | `false` | Completely ignore `other`/`unknown` processes |
| `top_n_subgroup` | `usize` | `3` | Top-N processes per subgroup (for /details) |
| `top_n_others` | `usize` | `10` | Top-N processes for the `other` group |
| `details_top_n` | `usize` | `5` | Top-N processes displayed in /details |
| `enable_rss` | `bool` | `true` | Collect RSS memory metrics |
| `enable_pss` | `bool` | `true` | Collect PSS memory metrics |
| `enable_uss` | `bool` | `true` | Collect USS memory metrics |
| `enable_cpu` | `bool` | `true` | Collect CPU metrics |
| `test_data_file` | `path` | — | Use synthetic JSON data instead of /proc |
| `enable_tls` | `bool` | `false` | Enable HTTPS |
| `tls_cert_path` | `string` | — | Path to TLS certificate (PEM) |
| `tls_key_path` | `string` | — | Path to TLS private key (PEM) |
| `enable_ebpf` | `bool` | `true` | Enable eBPF subsystem |
| `enable_ebpf_network` | `bool` | `true` | Enable eBPF network I/O tracking |
| `enable_ebpf_disk` | `bool` | `true` | Enable eBPF disk I/O tracking |
| `enable_filesystem_collector` | `bool` | `true` | Enable filesystem metrics collector |
| `enable_thermal_collector` | `bool` | `true` | Enable thermal/temperature metrics collector |
| `enable_psi_collector` | `bool` | `true` | Enable PSI (pressure stall) metrics collector |
| `ringbuffer.max_memory_mb` | `usize` | `15` | Maximum total ringbuffer memory in MB |
| `ringbuffer.interval_seconds` | `u64` | `30` | Ringbuffer sampling interval in seconds |
| `ringbuffer.min_entries_per_subgroup` | `usize` | `10` | Minimum history entries per subgroup |
| `ringbuffer.max_entries_per_subgroup` | `usize` | `120` | Maximum history entries per subgroup |

---

## eBPF Requirements

The `ebpf` feature is compiled in by default (`default = ["ebpf"]` in `Cargo.toml`). It provides:

- Per-process network I/O aggregation → `herakles_group_net_rx_bytes_total`, `herakles_group_net_tx_bytes_total`
- Per-process block I/O aggregation → `herakles_group_blkio_*`
- TCP connection state tracking → `herakles_system_tcp_connections_*`
- eBPF performance self-metrics → `herakles_ebpf_*`

### Kernel requirements

- Kernel ≥ 4.18 (as required by `--enable-ebpf` flag description in `src/cli.rs`)
- BTF (BPF Type Format) enabled: `/sys/kernel/btf/vmlinux` must exist
- Required capabilities: `CAP_BPF`, `CAP_PERFMON` (or run as root)

### Build-time dependencies (when compiling with `ebpf` feature)

| Dependency | Purpose |
|------------|---------|
| `clang` | Compiles `src/ebpf/bpf/process_io.bpf.c` to BPF object code |
| `bpftool` | Generates `vmlinux.h` from `/sys/kernel/btf/vmlinux` |
| `libbpf-rs = "0.24"` | Rust bindings for libbpf (pulled by Cargo) |
| `libbpf-sys = "1.4"` | C libbpf library (pulled by Cargo) |

### Graceful degradation

eBPF initialization failure is non-fatal. If `EbpfManager::new()` returns an error at startup, the exporter logs a warning (`⚠️ Failed to initialize eBPF: … - running without eBPF metrics`), increments the internal `ebpf_init_failures` counter, and continues running. All non-eBPF metrics remain fully functional. Check `/health` to see whether eBPF initialized successfully.

### Troubleshooting

```bash
# Check BTF availability
ls -la /sys/kernel/btf/vmlinux

# Check required capabilities
capsh --print | grep -E 'cap_bpf|cap_perfmon'

# Check clang and bpftool
clang --version
bpftool version

# Validate runtime requirements
herakles-node-exporter check-requirements --ebpf

# Check eBPF status via health endpoint
curl http://localhost:9215/health
```

---

## Example PromQL Queries

```promql
# RSS memory by subgroup — all postgres processes across all hosts
herakles_group_memory_rss_bytes{subgroup="postgres"}

# PSS memory by group — deduplicates shared memory contributions
herakles_group_memory_pss_bytes{group="db"}

# Disk read throughput per group (bytes/sec, 5-min rate)
rate(herakles_group_blkio_read_bytes_total[5m])

# Disk write throughput per block device
rate(herakles_system_disk_write_bytes_total[5m])

# Filesystem usage percentage per mount point
1 - (herakles_system_filesystem_avail_bytes / herakles_system_filesystem_size_bytes)

# Network receive throughput per interface (bytes/sec)
rate(herakles_system_net_rx_bytes_total[5m])

# Network transmit throughput per process group (eBPF)
rate(herakles_group_net_tx_bytes_total[5m])

# CPU PSI stall rate — fraction of time stalled waiting for CPU
rate(herakles_system_cpu_psi_wait_seconds_total[5m])

# Memory PSI stall rate
rate(herakles_system_memory_psi_wait_seconds_total[5m])

# I/O PSI stall rate
rate(herakles_system_disk_psi_wait_seconds_total[5m])

# Alert: system memory pressure above 90%
herakles_system_memory_used_ratio > 0.9
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
  help                Print this message or the help of the given subcommand(s)
```

### Flags

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--port <PORT>` | `-p` | — | HTTP listen port |
| `--bind <BIND>` | — | — | Bind to specific interface/IP |
| `--log-level <LEVEL>` | — | `info` | Log level: off, error, warn, info, debug, trace |
| `--config <FILE>` | `-c` | — | Config file (YAML/JSON/TOML) |
| `--no-config` | — | false | Disable all config file loading |
| `--show-config` | — | false | Print effective merged config and exit |
| `--show-user-config` | — | false | Print only the loaded user config file + full path and exit |
| `--config-format <FMT>` | — | `yaml` | Output format for --show-config*: yaml, json, toml |
| `--check-config` | — | false | Validate config and exit (return code 1 on error) |
| `--debug` | — | false | Enable /debug/pprof endpoints |
| `--cache-ttl <SECS>` | — | — | Cache metrics for N seconds |
| `--disable-health` | — | false | Disable /health endpoint + health metrics |
| `--disable-telemetry` | — | false | Disable internal exporter_* metrics |
| `--disable-default-collectors` | — | false | Disable generic collectors |
| `--io-buffer-kb <KB>` | — | — | Override IO buffer size (KB) for generic /proc readers |
| `--smaps-buffer-kb <KB>` | — | — | Override buffer size (KB) for /proc/<pid>/smaps |
| `--smaps-rollup-buffer-kb <KB>` | — | — | Override buffer size (KB) for /proc/<pid>/smaps_rollup |
| `--min-uss-kb <KB>` | — | — | Minimum USS in KB to include process |
| `--include-names <NAMES>` | — | — | Include only processes matching these names (comma-separated) |
| `--exclude-names <NAMES>` | — | — | Exclude processes matching these names (comma-separated) |
| `--parallelism <N>` | — | — | Parallel processing threads (0 = auto) |
| `--max-processes <N>` | — | — | Maximum number of processes to scan |
| `--top-n-subgroup <N>` | — | — | Top-N processes to export per subgroup (override config) |
| `--top-n-others <N>` | — | — | Top-N processes to export for "other" group (override config) |
| `--test-data-file <FILE>` | `-t` | — | Path to JSON test data file (uses synthetic data instead of /proc) |
| `--enable-tls` | — | false | Enable TLS/SSL for HTTPS |
| `--tls-cert <FILE>` | — | — | Path to TLS certificate file (PEM format) |
| `--tls-key <FILE>` | — | — | Path to TLS private key file (PEM format) |
| `--enable-ebpf` | — | false | Enable eBPF-based I/O tracking (requires kernel ≥ 4.18, BTF, CAP_BPF/CAP_PERFMON) |
| `--enable-ebpf-network` | — | false | Enable eBPF-based network I/O tracking |
| `--disable-ebpf-network` | — | false | Disable eBPF-based network I/O tracking (conflicts with --enable-ebpf-network) |
| `--enable-ebpf-disk` | — | false | Enable eBPF-based disk I/O tracking |
| `--disable-ebpf-disk` | — | false | Disable eBPF-based disk I/O tracking (conflicts with --enable-ebpf-disk) |
| `--enable-tcp-tracking` | — | false | Enable TCP connection state tracking via eBPF |
| `--disable-tcp-tracking` | — | false | Disable TCP connection state tracking via eBPF (conflicts with --enable-tcp-tracking) |

---

## Systemd Service

```ini
[Unit]
Description=Herakles Node Exporter
Documentation=https://github.com/cansp-dev/herakles-node-exporter
After=network.target

[Service]
Type=simple
# Root is required to read /proc/<pid>/smaps_rollup for processes owned by other users.
# After eBPF initialization, the process will attempt to drop privileges to the
# 'herakles' system user if it exists. If 'herakles' user is not present, the
# process continues as root (recommended for full system monitoring).
User=root
ExecStart=/usr/local/bin/herakles-node-exporter
Restart=on-failure
RestartSec=5s

# Security hardening (compatible with /proc access)
ProtectSystem=strict
ReadOnlyPaths=/proc
PrivateTmp=true
NoNewPrivileges=false

[Install]
WantedBy=multi-user.target
```

> **Why root?** Reading `/proc/<pid>/smaps_rollup` for processes owned by other users requires root privileges. The exporter reads this file to obtain accurate USS (Unique Set Size) figures. After eBPF programs are loaded and pinned, the process attempts to drop to the `herakles` system user if it exists — see `drop_privileges()` in `src/main.rs`. If the `herakles` user does not exist, the process continues as root, which is the recommended production configuration for full multi-user system monitoring.

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

Licensed under either of

- MIT License ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)

at your option.

---

## Author

Michael Moll <exporter@herakles.now> — Herakles
