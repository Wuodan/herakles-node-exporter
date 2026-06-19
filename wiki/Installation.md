# Installation Guide

This guide covers all installation methods for the Herakles Process Memory Exporter.

## Prerequisites

- **Linux**: Kernel 4.14+ recommended (for `smaps_rollup` support)
- **Permissions**: Read access to `/proc` filesystem

## Release Binary Install

```bash
curl -fsSL https://github.com/herakles-now/herakles-node-exporter/releases/latest/download/install.sh | sh
sudo herakles-node-exporter install
curl -f http://localhost:9215/health
```

Specific version:

```bash
curl -fsSL https://github.com/herakles-now/herakles-node-exporter/releases/latest/download/install.sh | \
  sh -s -- --version v0.1.1-alpha6
```

## Manual Binary Install

Download the release binary for your platform and install it into your `PATH`.

```bash
curl -fL -o herakles-node-exporter \
  https://github.com/herakles-now/herakles-node-exporter/releases/latest/download/herakles-node-exporter-x86_64-linux-gnu
chmod +x herakles-node-exporter
sudo install -m 0755 herakles-node-exporter /usr/local/bin/herakles-node-exporter
herakles-node-exporter --version
```

## System-Wide Installation

The built-in installer copies the binary, writes the default config, installs the systemd unit, starts the service,
and applies the eBPF sysctl defaults.

```bash
sudo herakles-node-exporter install
```

Useful flags:

```bash
sudo herakles-node-exporter install --no-service
sudo herakles-node-exporter install --force
sudo herakles-node-exporter uninstall
```

## System Check

Run after installation:

```bash
herakles-node-exporter check --all
```

## From Source

### Prerequisites

- **Rust**: 1.70+ (for building from source)
  - `cargo`, `clang`, `llvm`, `bpftool`, and kernel BTF data if you build with eBPF enabled

### Build And Install

```bash
# Clone the repository
git clone https://github.com/herakles-now/herakles-node-exporter.git
cd herakles-node-exporter

# Build optimized release binary
cargo build --release

# Install system-wide
sudo install -m 0755 target/release/herakles-node-exporter /usr/local/bin/herakles-node-exporter
```

## Docker

Copy the `musl` binaries from the 
[latest release](https://github.com/herakles-now/herakles-node-exporter/releases/latest) to `./herakles-node-exporter`
or build them from source.

### Build `musl` Binaries

On an **amd64** host, use:

```bash
cargo build --release --target x86_64-unknown-linux-musl
cp target/x86_64-unknown-linux-musl/release/herakles-node-exporter ./herakles-node-exporter
```

On an **arm64** host, use:

```bash
cargo build --release --target aarch64-unknown-linux-musl
cp target/aarch64-unknown-linux-musl/release/herakles-node-exporter ./herakles-node-exporter
```

### Build And Run Docker Image

```bash
docker build -t herakles-node-exporter:latest .
docker run -d \
  --name herakles-node-exporter \
  --pid=host \
  -v /proc:/proc:ro \
  -p 9215:9215 \
  herakles-node-exporter:latest
```

## Method 5: Docker Compose

### Basic Setup

```yaml
# docker-compose.yml
version: '3.8'

services:
  herakles-exporter:
    image: herakles-node-exporter:latest
    build: .
    container_name: herakles-exporter
    ports:
      - "9215:9215"
    volumes:
      - /proc:/host/proc:ro
    restart: unless-stopped
```

### Full Stack with Prometheus & Grafana

```yaml
# docker-compose.yml
version: '3.8'

services:
  herakles-exporter:
    image: herakles-node-exporter:latest
    build: .
    container_name: herakles-exporter
    ports:
      - "9215:9215"
    volumes:
      - /proc:/host/proc:ro
      - ./config.yaml:/etc/herakles/config.yaml:ro
    command: ["-c", "/etc/herakles/config.yaml"]
    restart: unless-stopped

  prometheus:
    image: prom/prometheus:latest
    container_name: prometheus
    ports:
      - "9090:9090"
    volumes:
      - ./prometheus.yml:/etc/prometheus/prometheus.yml:ro
      - prometheus-data:/prometheus
    command:
      - '--config.file=/etc/prometheus/prometheus.yml'
      - '--storage.tsdb.path=/prometheus'
    depends_on:
      - herakles-exporter
    restart: unless-stopped

  grafana:
    image: grafana/grafana:latest
    container_name: grafana
    ports:
      - "3000:3000"
    volumes:
      - grafana-data:/var/lib/grafana
    environment:
      - GF_SECURITY_ADMIN_PASSWORD=admin
    depends_on:
      - prometheus
    restart: unless-stopped

volumes:
  prometheus-data:
  grafana-data:
```

## Systemd Service Setup

### Create Service File

```bash
# Create service file
sudo tee /etc/systemd/system/herakles-node-exporter.service << 'EOF'
[Unit]
Description=Herakles Process Memory Exporter
Documentation=https://github.com/herakles-now/herakles-node-exporter
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=prometheus
Group=prometheus
ExecStart=/usr/local/bin/herakles-node-exporter -c /etc/herakles/config.yaml
Restart=always
RestartSec=5
TimeoutStopSec=30

# Security hardening
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
ReadOnlyPaths=/
ReadWritePaths=/var/log

# Capability to read /proc
CapabilityBoundingSet=CAP_DAC_READ_SEARCH
AmbientCapabilities=CAP_DAC_READ_SEARCH

[Install]
WantedBy=multi-user.target
EOF
```

### Enable and Start Service

```bash
# Create dedicated user
sudo useradd -r -s /sbin/nologin prometheus

# Create config directory
sudo mkdir -p /etc/herakles
sudo chown prometheus:prometheus /etc/herakles

# Create minimal config
sudo tee /etc/herakles/config.yaml << 'EOF'
port: 9215
bind: "0.0.0.0"
cache_ttl: 30
log_level: "info"
EOF

# Reload systemd
sudo systemctl daemon-reload

# Enable and start service
sudo systemctl enable herakles-node-exporter
sudo systemctl start herakles-node-exporter

# Check status
sudo systemctl status herakles-node-exporter
```

## Post-Installation

### 1. Verify System Check

```bash
herakles-node-exporter check --all
```

Expected output:

```text
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
sudo setcap cap_dac_read_search+ep /usr/local/bin/herakles-node-exporter
```

### Port Already in Use

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

## Next Steps

- [Configure the exporter](Configuration.md)
- [Set up Prometheus integration](Prometheus-Integration.md)
- [Understand the metrics](Metrics-Overview.md)

## 🔗 Project & Support

Project: <https://github.com/herakles-now/herakles-node-exporter> — More info: <https://www.herakles.now> — Support: <exporter@herakles.now>
