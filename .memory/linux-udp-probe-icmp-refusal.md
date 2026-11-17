---
name: linux-udp-probe-icmp-refusal
description: Why a closed UDP port never fails a Quinn probe on Linux and how SO_ERROR is the only way to see the refusal
type: project
---

Facts learned while porting the address probe to Linux (2026-11-17):

- Linux reports an ICMP port-unreachable on a **connected** UDP socket as a
  pending socket error (`SO_ERROR`), and the kernel raises `EPOLLERR` **without**
  `EPOLLIN`. Measured with raw epoll in the project image: mask `0xc` =
  `EPOLLERR | EPOLLOUT`, `EPOLLIN` clear.
- `mio` 1.2.3 maps that to `Event::is_readable()` = `EPOLLIN | EPOLLPRI` only
  (`mio/src/sys/unix/selector/epoll.rs`), so tokio never marks the socket
  readable, never runs the receive, and the error stays invisible. Quinn's
  default Tokio adapter therefore swallows the refusal, and a manual address
  lookup to a closed port burns its whole five second budget and reports
  `Timeout` instead of `Refused`.
- `getsockopt(SO_ERROR)` returns the refusal (`ECONNREFUSED`) and clears it;
  after that a plain `recv()` returns `EAGAIN`, so the error has to be read
  through `SO_ERROR` first.
- macOS (kqueue reports `EVFILT_READ` with `EV_ERROR`) and Windows (the own
  `windows_udp` adapter) surface the same failure as a receive error, so the
  gap was Linux-only.
- `sendto` on a connected Linux UDP socket with the *same* address succeeds;
  with a different address it does not fail with `EISCONN` like Darwin does.

**Why:** the failure looks like a five second timeout and a flaky probe, not
like a missing error path, and the whole readiness chain has to be read to
find it.
**How to apply:** keep the platform adapter in
`crates/oink-network/src/qos/linux_udp.rs`: it reads `SO_ERROR` on every
receive and arms a 50 ms re-check timer, because nothing else wakes a socket
that only carries `EPOLLERR`. Verify with
`cargo test -p oink-network --lib link::runtime::tests::manual_query_closed_udp_port_returns_a_terminal_probe_error`.
See also [[linux-platform-identity]].
