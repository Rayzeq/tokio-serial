//! Bindings for serial port I/O and futures
//!
//! This crate provides bindings between `mio_serial`, a mio crate for
//! serial port I/O, and `futures`.  The API is very similar to the
//! bindings in `mio_serial`
//!
#![deny(missing_docs)]
#![warn(rust_2018_idioms)]

// Re-export serialport types and traits from mio_serial
pub use mio_serial::{
    available_ports, new, ClearBuffer, DataBits, Error, ErrorKind, FlowControl, Parity, Result,
    SerialPort, SerialPortBuilder, SerialPortInfo, StopBits,
};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use std::io::{self, Read, Write};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

#[cfg(feature = "codec")]
mod frame;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix::UnixSerialStream as NativeSerialStream;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::WindowsSerialStream as NativeSerialStream;

/// Async serial port I/O
///
/// Reading and writing to a `TcpStream` is usually done using the
/// convenience methods found on the [`tokio::io::AsyncReadExt`] and [`tokio::io::AsyncWriteExt`]
/// traits.
///
/// [`AsyncReadExt`]: trait@tokio::io::AsyncReadExt
/// [`AsyncWriteExt`]: trait@tokio::io::AsyncWriteExt
///
#[derive(Debug)]
pub struct SerialStream {
    inner: NativeSerialStream,
}

impl SerialStream {
    /// Open serial port from a provided path, using the default reactor.
    pub fn open(builder: &SerialPortBuilder) -> crate::Result<Self> {
        let port = mio_serial::SerialStream::open(builder)?;
        let inner = NativeSerialStream::new(port)?;

        Ok(Self { inner })
    }

    /// Create a pair of pseudo serial terminals using the default reactor
    ///
    /// ## Returns
    /// Two connected, unnamed `Serial` objects.
    ///
    /// ## Errors
    /// Attempting any IO or parameter settings on the slave tty after the master
    /// tty is closed will return errors.
    ///
    #[cfg(unix)]
    pub fn pair() -> crate::Result<(Self, Self)> {
        let (primary, secondary) = NativeSerialStream::pair()?;
        let primary = Self { inner: primary };
        let secondary = Self { inner: secondary };

        Ok((primary, secondary))
    }

    /// Sets the exclusivity of the port
    ///
    /// If a port is exclusive, then trying to open the same device path again
    /// will fail.
    ///
    /// See the man pages for the tiocexcl and tiocnxcl ioctl's for more details.
    ///
    /// ## Errors
    ///
    /// * `Io` for any error while setting exclusivity for the port.
    #[cfg(unix)]
    pub fn set_exclusive(&mut self, exclusive: bool) -> crate::Result<()> {
        self.inner.get_mut().set_exclusive(exclusive)
    }

    /// Returns the exclusivity of the port
    ///
    /// If a port is exclusive, then trying to open the same device path again
    /// will fail.
    #[cfg(unix)]
    pub fn exclusive(&self) -> bool {
        self.inner.get_ref().exclusive()
    }

    /// Try to read bytes on the serial port.  On success returns the number of bytes read.
    ///
    /// The function must be called with valid byte array `buf` of sufficient
    /// size to hold the message bytes. If a message is too long to fit in the
    /// supplied buffer, excess bytes may be discarded.
    ///
    /// When there is no pending data, `Err(io::ErrorKind::WouldBlock)` is
    /// returned. This function is usually paired with `readable()`.
    pub fn try_read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }

    /// Wait for the port to become readable.
    ///
    /// This function is usually paired with `try_read()`.
    ///
    /// The function may complete without the socket being readable. This is a
    /// false-positive and attempting a `try_read()` will return with
    /// `io::ErrorKind::WouldBlock`.
    pub async fn readable(&self) -> io::Result<()> {
        self.inner.readable().await
    }

    /// Try to write bytes on the serial port.  On success returns the number of bytes written.
    ///
    /// When the write would block, `Err(io::ErrorKind::WouldBlock)` is
    /// returned. This function is usually paired with `writable()`.
    pub fn try_write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    /// Wait for the port to become writable.
    ///
    /// This function is usually paired with `try_write()`.
    ///
    /// The function may complete without the socket being readable. This is a
    /// false-positive and attempting a `try_write()` will return with
    /// `io::ErrorKind::WouldBlock`.
    pub async fn writable(&self) -> io::Result<()> {
        self.inner.writable().await
    }
}

impl AsyncRead for SerialStream {
    /// Attempts to ready bytes on the serial port.
    ///
    /// Note that on multiple calls to a `poll_*` method in the read direction, only the
    /// `Waker` from the `Context` passed to the most recent call will be scheduled to
    /// receive a wakeup.
    ///
    /// # Return value
    ///
    /// The function returns:
    ///
    /// * `Poll::Pending` if the socket is not ready to read
    /// * `Poll::Ready(Ok(()))` reads data `ReadBuf` if the socket is ready
    /// * `Poll::Ready(Err(e))` if an error is encountered.
    ///
    /// # Errors
    ///
    /// This function may encounter any standard I/O error except `WouldBlock`.
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for SerialStream {
    /// Attempts to send data on the serial port
    ///
    /// Note that on multiple calls to a `poll_*` method in the send direction,
    /// only the `Waker` from the `Context` passed to the most recent call will
    /// be scheduled to receive a wakeup.
    ///
    /// # Return value
    ///
    /// The function returns:
    ///
    /// * `Poll::Pending` if the socket is not available to write
    /// * `Poll::Ready(Ok(n))` `n` is the number of bytes sent
    /// * `Poll::Ready(Err(e))` if an error is encountered.
    ///
    /// # Errors
    ///
    /// This function may encounter any standard I/O error except `WouldBlock`.
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

impl SerialPort for SerialStream {
    #[inline(always)]
    fn name(&self) -> Option<String> {
        self.inner.get_ref().name()
    }

    #[inline(always)]
    fn baud_rate(&self) -> crate::Result<u32> {
        self.inner.get_ref().baud_rate()
    }

    #[inline(always)]
    fn data_bits(&self) -> crate::Result<crate::DataBits> {
        self.inner.get_ref().data_bits()
    }

    #[inline(always)]
    fn flow_control(&self) -> crate::Result<crate::FlowControl> {
        self.inner.get_ref().flow_control()
    }

    #[inline(always)]
    fn parity(&self) -> crate::Result<crate::Parity> {
        self.inner.get_ref().parity()
    }

    #[inline(always)]
    fn stop_bits(&self) -> crate::Result<crate::StopBits> {
        self.inner.get_ref().stop_bits()
    }

    #[inline(always)]
    fn timeout(&self) -> Duration {
        Duration::from_secs(0)
    }

    #[inline(always)]
    fn set_baud_rate(&mut self, baud_rate: u32) -> crate::Result<()> {
        self.inner.get_mut().set_baud_rate(baud_rate)
    }

    #[inline(always)]
    fn set_data_bits(&mut self, data_bits: crate::DataBits) -> crate::Result<()> {
        self.inner.get_mut().set_data_bits(data_bits)
    }

    #[inline(always)]
    fn set_flow_control(&mut self, flow_control: crate::FlowControl) -> crate::Result<()> {
        self.inner.get_mut().set_flow_control(flow_control)
    }

    #[inline(always)]
    fn set_parity(&mut self, parity: crate::Parity) -> crate::Result<()> {
        self.inner.get_mut().set_parity(parity)
    }

    #[inline(always)]
    fn set_stop_bits(&mut self, stop_bits: crate::StopBits) -> crate::Result<()> {
        self.inner.get_mut().set_stop_bits(stop_bits)
    }

    #[inline(always)]
    fn set_timeout(&mut self, _: Duration) -> crate::Result<()> {
        Ok(())
    }

    #[inline(always)]
    fn write_request_to_send(&mut self, level: bool) -> crate::Result<()> {
        self.inner.get_mut().write_request_to_send(level)
    }

    #[inline(always)]
    fn write_data_terminal_ready(&mut self, level: bool) -> crate::Result<()> {
        self.inner.get_mut().write_data_terminal_ready(level)
    }

    #[inline(always)]
    fn read_clear_to_send(&mut self) -> crate::Result<bool> {
        self.inner.get_mut().read_clear_to_send()
    }

    #[inline(always)]
    fn read_data_set_ready(&mut self) -> crate::Result<bool> {
        self.inner.get_mut().read_data_set_ready()
    }

    #[inline(always)]
    fn read_ring_indicator(&mut self) -> crate::Result<bool> {
        self.inner.get_mut().read_ring_indicator()
    }

    #[inline(always)]
    fn read_carrier_detect(&mut self) -> crate::Result<bool> {
        self.inner.get_mut().read_carrier_detect()
    }

    #[inline(always)]
    fn bytes_to_read(&self) -> crate::Result<u32> {
        self.inner.get_ref().bytes_to_read()
    }

    #[inline(always)]
    fn bytes_to_write(&self) -> crate::Result<u32> {
        self.inner.get_ref().bytes_to_write()
    }

    #[inline(always)]
    fn clear(&self, buffer_to_clear: crate::ClearBuffer) -> crate::Result<()> {
        self.inner.get_ref().clear(buffer_to_clear)
    }

    #[inline(always)]
    fn try_clone(&self) -> crate::Result<Box<dyn crate::SerialPort>> {
        Err(crate::Error::new(
            crate::ErrorKind::Io(std::io::ErrorKind::Other),
            "Cannot clone Tokio handles",
        ))
    }

    #[inline(always)]
    fn set_break(&self) -> crate::Result<()> {
        self.inner.get_ref().set_break()
    }

    #[inline(always)]
    fn clear_break(&self) -> crate::Result<()> {
        self.inner.get_ref().clear_break()
    }
}

impl Read for SerialStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Write for SerialStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(unix)]
mod sys {
    use super::SerialStream;
    use std::os::unix::io::{AsRawFd, RawFd};

    impl AsRawFd for SerialStream {
        fn as_raw_fd(&self) -> RawFd {
            self.inner.get_ref().as_raw_fd()
        }
    }
}

/// An extension trait for serialport::SerialPortBuilder
///
/// This trait adds two methods to SerialPortBuilder:
///
/// - open_async
///
/// These methods mirror the `open` and `open_native` methods of SerialPortBuilder
pub trait SerialPortBuilderExt {
    /// Open a platform-specific interface to the port with the specified settings
    fn open_async(self) -> Result<SerialStream>;
}

impl SerialPortBuilderExt for SerialPortBuilder {
    /// Open a platform-specific interface to the port with the specified settings
    fn open_async(self) -> Result<SerialStream> {
        SerialStream::open(&self)
    }
}
