# Installation Guide

This guide covers all installation methods for the Herakles Process Memory Exporter.

Installation places the binary at `/opt/herakles/bin/`, configuration at `/etc/herakles/`, and the systemd service at
`/etc/systemd/system/herakles-node-exporter.service`.

## Prerequisites

- **Linux**: Kernel 4.14+ recommended (for `smaps_rollup` support)
- **Permissions**: Read access to `/proc` filesystem (root)

## Uninstall

Run this to uninstall `herakles-node-exporter`:

```bash
sudo herakles-node-exporter uninstall
```

## Method 1: Release Installer

### One-Line Installer

```bash
curl -fsSL https://github.com/herakles-now/herakles-node-exporter/releases/latest/download/install.sh | sudo sh
```

Specific version:

```bash
curl -fsSL https://github.com/herakles-now/herakles-node-exporter/releases/latest/download/install.sh | \
  sudo sh -s -- --version <version>
```

### Manual Binary Install

Download the matching binary from the release page.

```bash
(
    export RELEASE=latest
    export ARCH=x86_64 # or aarch64
    export C_LIBRARY=gnu # or musl
    curl -fL -o herakles-node-exporter \
      "https://github.com/Wuodan/herakles-node-exporter/releases/$RELEASE/download/herakles-node-exporter-$ARCH-linux-$C_LIBRARY"
)
chmod +x herakles-node-exporter
sudo install -m 0755 herakles-node-exporter /opt/herakles/bin/herakles-node-exporter
./herakles-node-exporter --version
```

#### Manual System-Wide Installation

Install system-wide using the downloaded binary.

```bash
# Install binary + systemd service
sudo herakles-node-exporter install

# Install without the systemd service
sudo herakles-node-exporter install --no-service

# Force reinstall over existing installation
sudo herakles-node-exporter install --force

# Uninstall
sudo herakles-node-exporter uninstall
```

## Method 2: From Source

The `ebpf` feature is enabled by default. Building with eBPF requires:

- a working `clang`/`llvm` toolchain
- `libelf` development headers
- a kernel with BTF support (`/sys/kernel/btf/vmlinux`)

### Release Build

```bash
# Clone the repository
git clone https://github.com/cansp-dev/herakles-node-exporter.git
cd herakles-node-exporter

# Install build dependencies (Debian/Ubuntu)
sudo apt-get install -y clang libelf-dev

# Build optimized release binary
cargo build --release

# Verify binary
target/release/herakles-node-exporter --version

# Install system-wide with systemd service
sudo target/release/herakles-node-exporter install
```

### Release Build Without eBPF

```bash
# Release build without eBPF (smaller binary, no clang/BTF dependency)
make release CARGOFLAGS='--no-default-features'
```
`
### Development Build

```bash
# Build with debug symbols
cargo build

# Run directly from source
cargo run -- --help

# Run with specific options
cargo run -- -p 9215 --log-level debug
```

## Method 3: Docker Compose

> Due to technical restrictions by the Linux kernel and Docker it makes no sense (better wording please) to run
> `herakles-now-exporter` in containers. It cannot read useful metrics for either the container or the host there.

### Basic Setup

Run `herakles-node-exporter` on the host on port `9215`.

### Full Stack With Prometheus & Grafana

Due to technical restrictions by the Linux kernel and Docker it makes no sense (better wording please) to run
`herakles-now-exporter` in containers. It cannot read useful metrics for either the container or the host there.

But it can work very well with other containers. A Grafana dashboard backed by Prometheus running in docker-compose
can be started as described below.

### Run `herakles-now-exporter` On The Host

Run `herakles-now-exporter` on the host on port `9215`.

### Start Docker Compose

```bash
# Clone the repository
git clone https://github.com/cansp-dev/herakles-node-exporter.git
cd herakles-node-exporter

# Run docker compose with docker-compose.yml
docker compose up -d
# Or use the older docker-compose command with:
# docker-compose up -d
```

### View Grafana Dashboard

1. Open [http://localhost:3000](http://localhost:3000)
2. Login with user `admin` and password `admin`

### View Prometheus Console

Open [http://localhost:9090/targets](http://localhost:9090/targets)

## Systemd Service

The installation automatically sets up a `systemd` service, starts it and prints relevant information.

> [install.sh](https://github.com/herakles-now/herakles-node-exporter/blob/main/scripts/install.sh) runs
> `herakles-node-exporter install` which sets up the service.

```bash
# Check service status
sudo systemctl status herakles-node-exporter
```

## Post-Installation

### 1. Verify System Check

```bash
herakles-node-exporter check --all
```

Expected output:
```
🔍 Herakles Process Memory Exporter - System Check
===================================================

📁 Checking /proc filesystem...
   ✅ /proc filesystem accessible
   ✅ Can read 5 process entries

💾 Checking memory metrics accessibility...
   ✅ smaps_rollup available (fast path)
   ✅ Memory parsing successful: RSS=50MB, PSS=45MB, USS=40MB

⚙️  Checking configuration...
   ✅ Configuration is valid

📊 Checking subgroups configuration...
   ✅ 140 subgroups loaded

📋 Summary:
   ✅ All checks passed - system is ready
```

### 2. Verify Configuration

```bash
# Show effective configuration
herakles-node-exporter --show-config

# Validate configuration
herakles-node-exporter --check-config
```

### 3. Test Metrics Collection

```bash
# Start exporter in foreground
herakles-node-exporter --log-level debug

# In another terminal, fetch metrics
curl http://localhost:9215/metrics | head -50

# Check health endpoint
curl http://localhost:9215/health
```

## Troubleshooting Installation

### Permission Denied

```bash
# Error: Permission denied reading /proc/*/smaps
# Solution: Run with appropriate capabilities
sudo setcap cap_dac_read_search+ep /opt/herakles/bin/herakles-node-exporter
```

### Port Already In Use

```bash
# Check what's using port 9215
sudo lsof -i :9215

# Use a different port
herakles-node-exporter -p 9216
```

### Rust Build Errors

```bash
# Ensure Rust is up to date
rustup update stable

# Clean and rebuild
cargo clean
cargo build --release
```

### Missing smaps_rollup

```bash
# Check kernel version (4.14+ required for smaps_rollup)
uname -r

# The exporter will fall back to smaps if smaps_rollup is unavailable
# Performance may be reduced on older kernels
```

### Running As Non-Root User

Reading `/proc/<pid>/smaps_rollup` for processes owned by other users requires root privileges. This file provides
accurate USS (Unique Set Size) figures. Without root, USS data for root-owned processes is unavailable and those
processes are silently excluded from group memory metrics.

For complete multi-user system monitoring, the exporter is intended to run as `root`. The built-in installer sets up
the systemd service as `User=root` / `Group=root` so the process retains the `/proc` access and eBPF privileges it
needs.

Check effective user before debugging missing processes:

```bash
ps aux | grep herakles-node-exporter
# Should show: root ... herakles-node-exporter
```

## Next Steps

- [Configure the exporter](Configuration.md)
- [Set up Prometheus integration](Prometheus-Integration.md)
- [Understand the metrics](Metrics-Overview.md)

## 🔗 Project & Support

Project: <https://github.com/cansp-dev/herakles-node-exporter> — More info: <https://www.herakles.now> — Support: <exporter@herakles.now>
