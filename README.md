# TCP SO_LINGER(0) Memory Leak Reproducer

## Bug Summary

When a TCP server closes sockets with `SO_LINGER` set to `l_onoff=1, l_linger=0`, the kernel sends a RST and transitions directly from ESTABLISHED → CLOSE. However, the write buffer memory associated with those sockets is not properly reclaimed. The memory accumulates past `tcp_mem` limits with no owning sockets, eventually triggering the OOM killer.

Setting `SO_LINGER` to `l_onoff=1, l_linger=1` does **not** leak — the connection goes through FIN_WAIT1 → FIN_WAIT2 → CLOSE and all memory is freed properly.

## Reproduction Results

### Kernel 6.18.33 (latest 6.18.y LTS)

- **Kernel**: `6.18.33-62.116.amzn2023.x86_64`
- **Server**: `m5zn.6xlarge` (24 vCPU, 96 GB RAM)
- **Client**: `m5zn.6xlarge`
- **Config**: [kernel-config-6.18.33](./kernel-config-6.18.33)

#### Baseline (50,000 connections established, one client SIGSTOP'd)

```
TCP: inuse 50007 orphan 0 tw 1 alloc 50008 mem 13276
Mem:  94504 total,   719 used, 93292 free
tcp_mem: 1130601  1507469  2261202  (pages)
```

#### After ~7 minutes

```
TCP: inuse 29642 orphan 0 tw 6 alloc 29649 mem 1299193
Mem:  94504 total, 16410 used, 77598 free
```

**Result**: TCP mem grew from 13,276 to 1,299,193 pages (~5.1 GB tracked by the counter) while `inuse` sockets dropped from 50,007 to 29,642. System memory used grew from 719 MB to 16.4 GB. The `tcp_mem` upper limit is 2,261,202 pages (~8.8 GB). Memory continues to grow unbounded if the test runs longer.

### Kernel 5.15.168 (AL2)

Originally reported and reproduced by the customer on `5.15.168-114.166.amzn2.x86_64`. The same behavior is observed — memory grows past 80 GB and triggers the OOM killer.

## Setup

### Requirements

- **Server**: `m5zn.6xlarge` (or similar with ≥64 GB RAM to observe the leak before OOM)
- **Client**: `m5zn.6xlarge` (instance type likely doesn't matter)
- Both machines on the same VPC/subnet for low latency

### Server Configuration

```bash
sudo sysctl -w net.core.wmem_max="16777216"
```

### Client Configuration

```bash
sudo sysctl -w net.ipv4.tcp_rmem="4096 8192 16384"
sudo sysctl -w net.ipv4.ip_local_port_range="10000 65535"
```

### Build (Rust)

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Tested with `rustc 1.82.0 (f6e511eec 2024-10-15)`.

### Running

1. On the server: `cd server && cargo run --release`
2. On the client: update `ADDRESS` in `client/src/main.rs` with the server's private IP
3. On the client: `cd client && cargo run --release` — run this **twice** (50k total connections)
4. Wait for the server to log `Active Connection Count: 50000`
5. `SIGSTOP` one of the client processes: `kill -STOP <pid>`
6. Monitor: `watch -n5 'cat /proc/net/sockstat; echo ---; free -m'`

The server's write buffers fill up for the stopped client's connections. When `send()` returns `EAGAIN`, the server closes with `SO_LINGER(1,0)`. The sockets disappear from `inuse` but the memory is never freed.
