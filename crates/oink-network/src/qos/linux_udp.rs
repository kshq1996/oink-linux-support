//! Linux QUIC probe socket: report the ICMP unreachability that Linux keeps in
//! `SO_ERROR` instead of the receive queue.
use quinn::{AsyncUdpSocket, UdpPoller, udp};
use std::{
    future::Future,
    io,
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, ready},
    time::Duration,
};
use tokio::io::Interest;

/// Linux reports an ICMP port-unreachable on a connected UDP socket as a
/// pending socket error. The kernel raises `EPOLLERR` without `EPOLLIN`, so the
/// socket never becomes readable: a readiness-driven receive never runs and the
/// failure stays invisible until `SO_ERROR` is read. Nothing else reports it.
const ERROR_CHECK_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug)]
pub(super) struct Socket {
    io: tokio::net::UdpSocket,
    state: udp::UdpSocketState,
    /// Deadline of the next `SO_ERROR` check.
    error_check: Mutex<Option<Pin<Box<tokio::time::Sleep>>>>,
}
impl Socket {
    /// The dedicated manual lookup owns a connected socket, so an ICMP failure
    /// belongs to exactly that lookup and has to end it.
    pub(super) fn wrap_probe(socket: std::net::UdpSocket) -> io::Result<Arc<dyn AsyncUdpSocket>> {
        let state = udp::UdpSocketState::new((&socket).into())?;
        Ok(Arc::new(Self {
            io: tokio::net::UdpSocket::from_std(socket)?,
            state,
            error_check: Mutex::new(None),
        }))
    }
    /// Read and clear the pending socket error, if the kernel holds one.
    fn take_pending_error(&self) -> Option<io::Error> {
        self.io.take_error().ok().flatten()
    }
    /// Keep the pending socket error observable. The kernel never marks this
    /// socket readable for it, so no receive readiness wakes this task.
    fn arm_error_check(&self, cx: &mut Context<'_>) {
        let mut slot = self.error_check.lock().expect("error check");
        let check = slot.get_or_insert_with(|| Box::pin(tokio::time::sleep(ERROR_CHECK_INTERVAL)));
        if check.as_mut().poll(cx).is_ready() {
            check.as_mut().reset(tokio::time::Instant::now() + ERROR_CHECK_INTERVAL);
            // The new deadline needs its own wake-up.
            let _ = check.as_mut().poll(cx);
        }
    }
}
type Writable = Pin<Box<dyn Future<Output = io::Result<()>> + Send + Sync>>;
struct Poller {
    socket: Arc<Socket>,
    wait: Option<Writable>,
}
impl std::fmt::Debug for Poller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinuxUdpPoller").finish()
    }
}
impl UdpPoller for Poller {
    fn poll_writable(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.wait.is_none() {
            let socket = self.socket.clone();
            self.wait = Some(Box::pin(async move { socket.io.writable().await }));
        }
        let result = ready!(self.wait.as_mut().expect("initialized").as_mut().poll(cx));
        self.wait = None;
        Poll::Ready(result)
    }
}
impl AsyncUdpSocket for Socket {
    fn create_io_poller(self: Arc<Self>) -> Pin<Box<dyn UdpPoller>> {
        Box::pin(Poller {
            socket: self,
            wait: None,
        })
    }
    fn try_send(&self, transmit: &udp::Transmit<'_>) -> io::Result<()> {
        // The probe socket is connected to its single peer, so a plain send
        // keeps the kernel's ICMP report attached to this lookup.
        self.io.try_send(transmit.contents).map(|_| ())
    }
    fn poll_recv(
        &self,
        cx: &mut Context<'_>,
        bufs: &mut [io::IoSliceMut<'_>],
        meta: &mut [udp::RecvMeta],
    ) -> Poll<io::Result<usize>> {
        // The pending socket error is Linux's only report of an ICMP
        // unreachable. Surface it as the receive error it is.
        if let Some(error) = self.take_pending_error() {
            return Poll::Ready(Err(error));
        }
        self.arm_error_check(cx);
        loop {
            ready!(self.io.poll_recv_ready(cx))?;
            match self
                .io
                .try_io(Interest::READABLE, || self.state.recv((&self.io).into(), bufs, meta))
            {
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                result => return Poll::Ready(result),
            }
        }
    }
    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.io.local_addr()
    }
    fn may_fragment(&self) -> bool {
        self.state.may_fragment()
    }
    fn max_transmit_segments(&self) -> usize {
        // One datagram per transmit: the connected send carries no GSO.
        1
    }
    fn max_receive_segments(&self) -> usize {
        self.state.gro_segments()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A closed UDP port answers the first datagram with an ICMP
    /// port-unreachable. Linux keeps that answer in `SO_ERROR` and never makes
    /// the socket readable, so the refusal has to be reported explicitly.
    #[tokio::test]
    async fn a_closed_port_reports_its_icmp_refusal() {
        let vacant = std::net::UdpSocket::bind(("127.0.0.1", 0)).unwrap();
        let address = vacant.local_addr().unwrap();
        drop(vacant);
        let raw = std::net::UdpSocket::bind(("127.0.0.1", 0)).unwrap();
        raw.connect(address).unwrap();
        // The datagram draws the ICMP port-unreachable that Linux keeps in
        // SO_ERROR.
        raw.send(b"probe").unwrap();
        raw.set_nonblocking(true).unwrap();
        let socket = Socket::wrap_probe(raw).unwrap();
        let mut buffer = [0u8; 64];
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            std::future::poll_fn(|cx| {
                let mut bufs = [io::IoSliceMut::new(&mut buffer)];
                let mut meta = [udp::RecvMeta::default()];
                socket.poll_recv(cx, &mut bufs, &mut meta)
            }),
        )
        .await;
        let error = result
            .expect("the refusal must arrive")
            .expect_err("the refusal must be reported as an error");
        assert_eq!(error.kind(), io::ErrorKind::ConnectionRefused);
    }
}
