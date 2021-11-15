use crate::runtime::RuntimeContext;
use pin_project::pin_project;
use std::future::Future;
use std::io::ErrorKind;
use std::net::SocketAddr;

/// A wrapper around [`std::net::UdpSocket`] that enables _futures_.
pub struct UdpSocket(std::net::UdpSocket);

impl UdpSocket {
    /// Create a new socket
    ///
    /// This will set the socket to be non-blocking.
    pub fn new(socket: std::net::UdpSocket) -> Result<Self, std::io::Error> {
        socket.set_nonblocking(true)?;
        Ok(Self(socket))
    }

    /// Get access to the wrapped UdpSocket
    pub fn inner(&self) -> &std::net::UdpSocket {
        &self.0
    }

    /// Get mutable access to the wrapped UdpSocket
    pub fn inner_mut(&mut self) -> &mut std::net::UdpSocket {
        &mut self.0
    }

    /// Receive a packet from the socket, as a _future_.
    pub async fn recv<'a, 'b>(&'a self, buf: &'b mut [u8]) -> Result<usize, std::io::Error> {
        Recv {
            socket: &self,
            buf,
            state: RegisteredState::Unregistered,
        }
        .await
    }

    /// Receive a packet from the socket, as a _future_.
    pub async fn recv_from<'a, 'b>(
        &'a self,
        buf: &'b mut [u8],
    ) -> Result<(usize, SocketAddr), std::io::Error> {
        RecvFrom {
            socket: &self,
            buf,
            state: RegisteredState::Unregistered,
        }
        .await
    }

    /// Send a packet on the socket, as a _future_.
    pub async fn send_to<'a, 'b>(
        &'a self,
        buf: &'b [u8],
        addr: SocketAddr,
    ) -> Result<usize, std::io::Error> {
        SendTo {
            socket: &self,
            buf,
            addr,
            state: RegisteredState::Unregistered,
        }
        .await
    }
}

/// Track whether the file descriptor has been registered with the runtime or not
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum RegisteredState {
    Unregistered,
    Registered,
}

/// The future that runs [`UdpSocket::recv`]
#[pin_project]
struct Recv<'a, 'b> {
    socket: &'a UdpSocket,
    buf: &'b mut [u8],
    state: RegisteredState,
}

impl<'a, 'b> Future for Recv<'a, 'b> {
    type Output = Result<usize, std::io::Error>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let projected = self.project();

        // Call `.recv` on the inner socket. Since the socket is set to non-blocking, this
        // should return immediately.
        let result = projected.socket.0.recv(*projected.buf);
        match result {
            // Success! Return the number of bytes read
            Ok(ok) => std::task::Poll::Ready(Ok(ok)),
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                // Not ready yet. If we haven't registered the file descriptor with the runtime,
                // do it now.
                if *projected.state == RegisteredState::Unregistered {
                    let context = RuntimeContext::current();
                    context.register_file_descriptor(&projected.socket.0);
                    *projected.state = RegisteredState::Registered;
                }
                std::task::Poll::Pending
            }
            Err(err) => std::task::Poll::Ready(Err(err)),
        }
    }
}

/// The future that runs [`UdpSocket::recv_from`]
#[pin_project]
struct RecvFrom<'a, 'b> {
    socket: &'a UdpSocket,
    buf: &'b mut [u8],
    state: RegisteredState,
}

impl<'a, 'b> Future for RecvFrom<'a, 'b> {
    type Output = Result<(usize, SocketAddr), std::io::Error>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let projected = self.project();

        // Call `.recv_from` on the inner socket. Since the listener is set to non-blocking, this
        // should return immediately.
        let result = projected.socket.0.recv_from(*projected.buf);
        match result {
            // Success! Return the information
            Ok(ok) => std::task::Poll::Ready(Ok(ok)),
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                // Not ready yet. If we haven't registered the file descriptor with the runtime,
                // do it now.
                if *projected.state == RegisteredState::Unregistered {
                    let context = RuntimeContext::current();
                    context.register_file_descriptor(&projected.socket.0);
                    *projected.state = RegisteredState::Registered;
                }
                std::task::Poll::Pending
            }
            Err(err) => std::task::Poll::Ready(Err(err)),
        }
    }
}

/// The future that runs [`UdpSocket::send_to`]
#[pin_project]
struct SendTo<'a, 'b> {
    socket: &'a UdpSocket,
    buf: &'b [u8],
    addr: SocketAddr,
    state: RegisteredState,
}

impl<'a, 'b> Future for SendTo<'a, 'b> {
    type Output = Result<usize, std::io::Error>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let projected = self.project();

        // Call `.send_to` on the inner listener. Since the socket is set to non-blocking, this
        // should return immediately.
        let result = projected.socket.0.send_to(*projected.buf, *projected.addr);
        match result {
            // Success! Return the number of bytes written
            Ok(ok) => std::task::Poll::Ready(Ok(ok)),
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                // Not ready yet. If we haven't registered the file descriptor with the runtime,
                // do it now.
                if *projected.state == RegisteredState::Unregistered {
                    let context = RuntimeContext::current();
                    context.register_file_descriptor(&projected.socket.0);
                    *projected.state = RegisteredState::Registered;
                }
                std::task::Poll::Pending
            }
            Err(err) => std::task::Poll::Ready(Err(err)),
        }
    }
}
