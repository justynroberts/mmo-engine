//! Batched UDP I/O: `recvmmsg` and `sendmmsg` move hundreds of datagrams per
//! syscall, which is what makes tens of thousands of packets a second cheap on
//! one core. Linux only. IPv4 only (the spike binds 0.0.0.0).

use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::os::fd::AsRawFd;

pub const RBATCH: usize = 512;
pub const SBATCH: usize = 512;
/// Inputs are 16 bytes; anything larger than this slot is truncated and will
/// fail to decode (which is the right outcome for junk).
pub const RSLOT: usize = 64;

pub fn bind(port: u16, rcvbuf: i32, sndbuf: i32) -> std::io::Result<UdpSocket> {
    let sock = UdpSocket::bind(("0.0.0.0", port))?;
    sock.set_nonblocking(true)?;
    let fd = sock.as_raw_fd();
    set_int(fd, libc::SO_RCVBUF, rcvbuf)?;
    set_int(fd, libc::SO_SNDBUF, sndbuf)?;
    Ok(sock)
}

pub fn effective_buffers(sock: &UdpSocket) -> (i32, i32) {
    (get_int(sock.as_raw_fd(), libc::SO_RCVBUF), get_int(sock.as_raw_fd(), libc::SO_SNDBUF))
}

fn set_int(fd: i32, opt: i32, v: i32) -> std::io::Result<()> {
    let r = unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            opt,
            &v as *const i32 as *const libc::c_void,
            std::mem::size_of::<i32>() as libc::socklen_t,
        )
    };
    if r != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn get_int(fd: i32, opt: i32) -> i32 {
    let mut v: i32 = 0;
    let mut len = std::mem::size_of::<i32>() as libc::socklen_t;
    unsafe {
        libc::getsockopt(fd, libc::SOL_SOCKET, opt, &mut v as *mut i32 as *mut libc::c_void, &mut len);
    }
    v
}

pub fn pin_to_core(core: usize) -> bool {
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        libc::CPU_SET(core, &mut set);
        libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set) == 0
    }
}

#[inline]
fn to_sockaddr(a: &SocketAddrV4) -> libc::sockaddr_in {
    libc::sockaddr_in {
        sin_family: libc::AF_INET as libc::sa_family_t,
        sin_port: a.port().to_be(),
        sin_addr: libc::in_addr { s_addr: u32::from_ne_bytes(a.ip().octets()) },
        sin_zero: [0; 8],
    }
}

#[inline]
fn from_sockaddr(s: &libc::sockaddr_in) -> SocketAddrV4 {
    SocketAddrV4::new(Ipv4Addr::from(s.sin_addr.s_addr.to_ne_bytes()), u16::from_be(s.sin_port))
}

pub struct Rx {
    fd: i32,
    buf: Vec<u8>,
    iov: Vec<libc::iovec>,
    addr: Vec<libc::sockaddr_in>,
    msg: Vec<libc::mmsghdr>,
    n: usize,
}

impl Rx {
    pub fn new(fd: i32) -> Rx {
        let mut rx = Rx {
            fd,
            buf: vec![0u8; RBATCH * RSLOT],
            iov: vec![unsafe { std::mem::zeroed() }; RBATCH],
            addr: vec![unsafe { std::mem::zeroed() }; RBATCH],
            msg: vec![unsafe { std::mem::zeroed() }; RBATCH],
            n: 0,
        };
        // The vectors never reallocate, so these self-references stay valid.
        for i in 0..RBATCH {
            rx.iov[i].iov_base = unsafe { rx.buf.as_mut_ptr().add(i * RSLOT) } as *mut libc::c_void;
            rx.iov[i].iov_len = RSLOT;
            rx.msg[i].msg_hdr.msg_name = &mut rx.addr[i] as *mut libc::sockaddr_in as *mut libc::c_void;
            rx.msg[i].msg_hdr.msg_iov = &mut rx.iov[i] as *mut libc::iovec;
            rx.msg[i].msg_hdr.msg_iovlen = 1;
        }
        rx
    }

    /// Receive up to one batch without blocking. Returns the number of datagrams.
    pub fn recv(&mut self) -> usize {
        for m in self.msg.iter_mut() {
            m.msg_hdr.msg_namelen = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
            m.msg_len = 0;
        }
        let r = unsafe {
            libc::recvmmsg(
                self.fd,
                self.msg.as_mut_ptr(),
                RBATCH as u32,
                libc::MSG_DONTWAIT,
                std::ptr::null_mut(),
            )
        };
        self.n = if r > 0 { r as usize } else { 0 };
        self.n
    }

    pub fn packet(&self, i: usize) -> (SocketAddrV4, &[u8]) {
        let len = (self.msg[i].msg_len as usize).min(RSLOT);
        (from_sockaddr(&self.addr[i]), &self.buf[i * RSLOT..i * RSLOT + len])
    }
}

pub struct Tx {
    fd: i32,
    data: Vec<u8>,
    queued: Vec<(u32, u16, SocketAddrV4)>,
    iov: Vec<libc::iovec>,
    addr: Vec<libc::sockaddr_in>,
    msg: Vec<libc::mmsghdr>,
    pub sent: u64,
    pub bytes: u64,
    pub dropped: u64,
}

impl Tx {
    pub fn new(fd: i32) -> Tx {
        let mut tx = Tx {
            fd,
            data: Vec::with_capacity(SBATCH * 4 * 1200),
            queued: Vec::with_capacity(SBATCH * 4),
            iov: vec![unsafe { std::mem::zeroed() }; SBATCH],
            addr: vec![unsafe { std::mem::zeroed() }; SBATCH],
            msg: vec![unsafe { std::mem::zeroed() }; SBATCH],
            sent: 0,
            bytes: 0,
            dropped: 0,
        };
        for i in 0..SBATCH {
            tx.msg[i].msg_hdr.msg_name = &mut tx.addr[i] as *mut libc::sockaddr_in as *mut libc::c_void;
            tx.msg[i].msg_hdr.msg_namelen = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
            tx.msg[i].msg_hdr.msg_iov = &mut tx.iov[i] as *mut libc::iovec;
            tx.msg[i].msg_hdr.msg_iovlen = 1;
        }
        tx
    }

    pub fn queue(&mut self, to: SocketAddrV4, payload: &[u8]) {
        self.queued.push((self.data.len() as u32, payload.len() as u16, to));
        self.data.extend_from_slice(payload);
        if self.queued.len() >= SBATCH * 4 {
            self.flush();
        }
    }

    pub fn flush(&mut self) {
        let n = self.queued.len();
        let mut off = 0;
        while off < n {
            let m = (n - off).min(SBATCH);
            for k in 0..m {
                let (o, l, a) = self.queued[off + k];
                self.iov[k].iov_base = unsafe { self.data.as_ptr().add(o as usize) } as *mut libc::c_void;
                self.iov[k].iov_len = l as usize;
                self.addr[k] = to_sockaddr(&a);
            }
            let r = unsafe { libc::sendmmsg(self.fd, self.msg.as_mut_ptr(), m as u32, 0) };
            if r <= 0 {
                // Socket buffer full or an error: drop the rest of this flush.
                // UDP gives no delivery guarantee and the delta scheme is built for loss.
                self.dropped += (n - off) as u64;
                break;
            }
            let r = r as usize;
            for k in 0..r {
                self.bytes += self.msg[k].msg_len as u64;
            }
            self.sent += r as u64;
            off += r;
        }
        self.data.clear();
        self.queued.clear();
    }
}
