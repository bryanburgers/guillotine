use crate::runtime::RuntimeContext;
use pin_project::pin_project;
use std::future::Future;
use std::io::ErrorKind;
use std::net::SocketAddr;

/// A wrapper around [`std::net::TcpListener`] that enables _futures_.
pub struct TcpListener(std::net::TcpListener);

impl TcpListener {
    /// Create a new listener
    ///
    /// This will set the listener to be non-blocking.
    pub fn new(listener: std::net::TcpListener) -> Result<Self, std::io::Error> {
        listener.set_nonblocking(true)?;
        Ok(Self(listener))
    }

    /// Get access to the wrapped TcpListener
    pub fn inner(&self) -> &std::net::TcpListener {
        &self.0
    }

    /// Get mutable access to the wrapped TcpListener
    pub fn inner_mut(&mut self) -> &mut std::net::TcpListener {
        &mut self.0
    }

    /// Wait until a new connection is available and accept that connection
    pub async fn accept(&self) -> Result<(TcpStream, SocketAddr), std::io::Error> {
        Accept {
            listener: &self,
            state: RegisteredState::Unregistered,
        }
        .await
    }
}

/// A wrapper around [`std::net::TcpStream`] that enables _futures_.
pub struct TcpStream(std::net::TcpStream);

impl TcpStream {
    /// Create a new stream
    ///
    /// This will set the listener to be non-blocking.
    pub fn new(stream: std::net::TcpStream) -> Result<Self, std::io::Error> {
        stream.set_nonblocking(true)?;
        Ok(Self(stream))
    }

    /// Get access to the wrapped TcpStream
    pub fn inner(&self) -> &std::net::TcpStream {
        &self.0
    }

    /// Get mutable access to the wrapped TcpStream
    pub fn inner_mut(&mut self) -> &mut std::net::TcpStream {
        &mut self.0
    }

    /// Read bytes from the stream, as a future
    pub async fn read<'a, 'b>(&'a mut self, buf: &'b mut [u8]) -> Result<usize, std::io::Error> {
        Read {
            stream: self,
            buf,
            state: RegisteredState::Unregistered,
        }
        .await
    }

    /// Write bytes to the stream, as a future
    pub async fn write<'a, 'b>(&'a mut self, buf: &'b [u8]) -> Result<usize, std::io::Error> {
        Write {
            stream: self,
            buf,
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

/// The future that runs [`TcpListener::accept`]
#[pin_project]
struct Accept<'a> {
    listener: &'a TcpListener,
    state: RegisteredState,
}

impl<'a> Future for Accept<'a> {
    type Output = Result<(TcpStream, SocketAddr), std::io::Error>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let projected = self.project();

        // Call `.accept` on the inner listener. Since the listener is set to non-blocking, this
        // should return immediately.
        let result = projected.listener.0.accept();
        match result {
            // Success! Return the accepted stream
            Ok((stream, addr)) => match TcpStream::new(stream) {
                Ok(stream) => std::task::Poll::Ready(Ok((stream, addr))),
                Err(err) => std::task::Poll::Ready(Err(err)),
            },
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                // Not ready yet. If we haven't registered the file descriptor with the runtime,
                // do it now.
                if *projected.state == RegisteredState::Unregistered {
                    let context = RuntimeContext::current();
                    context.register_file_descriptor(&projected.listener.0);
                    *projected.state = RegisteredState::Registered;
                }
                std::task::Poll::Pending
            }
            Err(err) => std::task::Poll::Ready(Err(err)),
        }
    }
}

/// The future that runs [`TcpStream::read`]
#[pin_project]
struct Read<'a, 'b> {
    stream: &'a mut TcpStream,
    buf: &'b mut [u8],
    state: RegisteredState,
}

impl<'a, 'b> Future for Read<'a, 'b> {
    type Output = Result<usize, std::io::Error>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        use std::io::Read;

        let projected = self.project();

        // Call `.read` on the inner stream. Since the stream is set to non-blocking, this should
        // return immediately.
        let result = projected.stream.0.read(*projected.buf);
        match result {
            // Successs! Return the number of bytes read
            Ok(ok) => std::task::Poll::Ready(Ok(ok)),
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                // Not ready yet. If we haven't registered the file descriptor with the runtime,
                // do it now.
                if *projected.state == RegisteredState::Unregistered {
                    let context = RuntimeContext::current();
                    context.register_file_descriptor(&projected.stream.0);
                    *projected.state = RegisteredState::Registered;
                }
                std::task::Poll::Pending
            }
            Err(err) => std::task::Poll::Ready(Err(err)),
        }
    }
}

/// The future that runs [`TcpStream::write`]
#[pin_project]
struct Write<'a, 'b> {
    stream: &'a mut TcpStream,
    buf: &'b [u8],
    state: RegisteredState,
}

impl<'a, 'b> Future for Write<'a, 'b> {
    type Output = Result<usize, std::io::Error>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        use std::io::Write;

        let projected = self.project();

        // Call `.write` on the inner stream. Since the stream is set to non-blocking, this should
        // return immediately.
        let result = projected.stream.0.write(*projected.buf);
        match result {
            // Successs! Return the number of bytes written
            Ok(ok) => std::task::Poll::Ready(Ok(ok)),
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                // Not ready yet. If we haven't registered the file descriptor with the runtime,
                // do it now.
                if *projected.state == RegisteredState::Unregistered {
                    let context = RuntimeContext::current();
                    context.register_file_descriptor(&projected.stream.0);
                    *projected.state = RegisteredState::Registered;
                }
                std::task::Poll::Pending
            }
            Err(err) => std::task::Poll::Ready(Err(err)),
        }
    }
}
