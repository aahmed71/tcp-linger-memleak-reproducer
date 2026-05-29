# TCP SO_LINGER(0) Memory Leak Reproducer

## Bug Summary

When a TCP server closes sockets with `SO_LINGER` set to `l_onoff=1, l_linger=0`, the kernel sends a RST and transitions directly from ESTABLISHED → CLOSE. However, the write buffer memory associated with those sockets is not properly reclaimed. The memory accumulates past `tcp_mem` limits with no owning sockets, eventually triggering the OOM killer.

Setting `SO_LINGER` to `l_onoff=1, l_linger=1` does **not** leak — the connection goes through FIN_WAIT1 → FIN_WAIT2 → CLOSE and all memory is freed properly.

## Reproduction Results

### Kernel 6.18.33 (latest 6.18.y LTS)

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

## Setup

Custom instance configuration (only for the **Server** instance):  

```
sudo sysctl -w net.core.wmem_max="16777216"
```

Custom instance configuration (only for the **Client** instance):  
This lowers the TCP read buffers to quite small amount, which will help get them full very fast
once we `SIGSTOP` the client process.

```
sudo sysctl -w net.ipv4.tcp_rmem="4096 8192 16384"
sudo sysctl -w net.ipv4.ip_local_port_range="10000 65535"
```

Getting the necessary rust version up and running:

```
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Originally tested on `rustc 1.82.0 (f6e511eec 2024-10-15)`

Getting server side up and running:

1. review the code
2. `cargo run --release`
3. optionally switch `ADDRESS` to a more desired port

Getting client side up and running:

1. review the code
2. update the `ADDRESS` with your server ip:port
3. `cargo run --release`
4. start the client **twice** (with default configuration this will result in chewing through 50k ports)

Wait for both clients to open all their connections, the server side will log out the active
connection count, once **50k** is reached, the clients are ready.  
At this stage, send `SIGSTOP` to one of the clients (the other client needs to remain running).  
Feel free to just open up htop, select the PID and F9 => 19.

What you should end up seeing is the kernel tcp memory pressure mechanism kicking in,
which will start to drop connections on the server side.  
These connections (with the current socket options) appear to end up in some form of invisible limbo,
consuming kernel memory over time.
