use libc::{c_int, c_long, c_ulong, c_void, off_t, pid_t, size_t, ssize_t, SYS_munmap};
use libc::{SYS_getpid, SYS_ioctl, SYS_mmap};
use nix::sys::signal::Signal;
use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::Pid;
use simple_error::bail;
use simple_error::try_with;
use std::os::unix::prelude::RawFd;

use crate::cpu::{self, Regs};
use crate::ptrace;
use crate::result::Result;

pub struct Process {
    process_idx: usize,
    saved_regs: Regs,
    saved_text: c_long,
    threads: Vec<ptrace::Thread>,
}

pub fn attach(pid: Pid) -> Result<Process> {
    let (threads, process_idx) = ptrace::attach_all_threads(pid)?;

    let saved_regs = try_with!(
        threads[process_idx].getregs(),
        "cannot get registers for main process ({})",
        threads[process_idx].tid
    );
    let ip = saved_regs.ip();
    let saved_text = try_with!(
        threads[process_idx].read(ip as *mut c_void),
        "cannot get text for main process"
    );
    try_with!(
        unsafe { threads[process_idx].write(ip as *mut c_void, cpu::SYSCALL_TEXT as *mut c_void) },
        "cannot patch syscall instruction"
    );

    Ok(Process {
        process_idx,
        saved_regs,
        saved_text,
        threads,
    })
}

macro_rules! syscall_args {
    ($regs:expr, $nr:expr) => {
        ($regs).prepare_syscall(&[$nr, 0, 0, 0, 0, 0, 0])
    };

    ($regs:expr, $nr:expr, $a1:expr) => {
        ($regs).prepare_syscall(&[$nr, $a1 as c_ulong, 0, 0, 0, 0, 0])
    };

    ($regs:expr, $nr:expr, $a1:expr, $a2:expr) => {
        ($regs).prepare_syscall(&[$nr, $a1 as c_ulong, $a2 as c_ulong, 0, 0, 0, 0])
    };

    ($regs:expr, $nr:expr, $a1:expr, $a2:expr, $a3:expr) => {
        $regs.prepare_syscall(&[$nr, $a1 as c_ulong, $a2 as c_ulong, $a3 as c_ulong, 0, 0, 0])
    };

    ($regs:expr, $nr:expr, $a1:expr, $a2:expr, $a3:expr, $a4:expr) => {
        $regs.prepare_syscall(&[
            $nr,
            $a1 as c_ulong,
            $a2 as c_ulong,
            $a3 as c_ulong,
            $a4 as c_ulong,
            0,
            0,
        ])
    };

    ($regs:expr, $nr:expr, $a1:expr, $a2:expr, $a3:expr, $a4:expr, $a5:expr) => {
        $regs.prepare_syscall(&[
            $nr,
            $a1 as c_ulong,
            $a2 as c_ulong,
            $a3 as c_ulong,
            $a4 as c_ulong,
            $a5 as c_ulong,
            0,
        ])
    };

    ($regs:expr, $nr:expr, $a1:expr, $a2:expr, $a3:expr, $a4:expr, $a5:expr, $a6:expr) => {
        $regs.prepare_syscall(&[
            $nr,
            $a1 as c_ulong,
            $a2 as c_ulong,
            $a3 as c_ulong,
            $a4 as c_ulong,
            $a5 as c_ulong,
            $a6 as c_ulong,
        ])
    };
}

impl Process {
    pub fn ioctl(&self, fd: RawFd, request: c_ulong, arg: c_ulong) -> Result<c_int> {
        let args = syscall_args!(
            self.saved_regs,
            SYS_ioctl as c_ulong,
            fd as c_ulong,
            request,
            arg
        );

        self.syscall(&args).map(|v| v as c_int)
    }

    #[allow(dead_code)]
    pub fn getpid(&self) -> Result<pid_t> {
        let args = syscall_args!(self.saved_regs, SYS_getpid as c_ulong);

        self.syscall(&args).map(|v| v as c_int)
    }

    pub fn mmap(
        &self,
        addr: *mut c_void,
        length: size_t,
        prot: c_int,
        flags: c_int,
        fd: RawFd,
        offset: off_t,
    ) -> Result<*mut c_void> {
        let args = syscall_args!(
            self.saved_regs,
            SYS_mmap as c_ulong,
            addr,
            length,
            prot,
            flags,
            fd,
            offset
        );

        self.syscall(&args).map(|v| v as *mut c_void)
    }

    pub fn munmap(&self, addr: *mut c_void, length: libc::size_t) -> Result<()> {
        let args = syscall_args!(self.saved_regs, SYS_munmap as c_ulong, addr, length);

        self.syscall(&args).map(drop)
    }

    pub fn socket(&self, domain: c_int, ty: c_int, protocol: c_int) -> Result<c_int> {
        let args = syscall_args!(
            self.saved_regs,
            libc::SYS_socket as c_ulong,
            domain,
            ty,
            protocol
        );

        self.syscall(&args).map(|v| v as c_int)
    }

    pub fn close(&self, fd: c_int) -> Result<c_int> {
        let args = syscall_args!(self.saved_regs, libc::SYS_close as c_ulong, fd);

        self.syscall(&args).map(|v| v as c_int)
    }

    pub fn bind(
        &self,
        socket: c_int,
        address: *const libc::sockaddr,
        address_len: libc::socklen_t,
    ) -> Result<c_int> {
        let args = syscall_args!(
            self.saved_regs,
            libc::SYS_bind as c_ulong,
            socket,
            address,
            address_len
        );

        self.syscall(&args).map(|v| v as c_int)
    }

    pub fn connect(
        &self,
        socket: c_int,
        address: *const libc::sockaddr,
        len: libc::socklen_t,
    ) -> Result<c_int> {
        let args = syscall_args!(
            self.saved_regs,
            libc::SYS_connect as c_ulong,
            socket,
            address,
            len
        );

        self.syscall(&args).map(|v| v as c_int)
    }

    pub fn recvmsg(&self, fd: c_int, msg: *mut libc::msghdr, flags: c_int) -> Result<ssize_t> {
        let args = syscall_args!(
            self.saved_regs,
            libc::SYS_recvmsg as c_ulong,
            fd,
            msg,
            flags
        );

        self.syscall(&args).map(|v| v as ssize_t)
    }

    pub fn userfaultfd(&self, flags: c_int) -> Result<c_int> {
        let args = syscall_args!(self.saved_regs, libc::SYS_userfaultfd as c_ulong, flags);

        self.syscall(&args).map(|v| v as c_int)
    }

    fn syscall(&self, regs: &Regs) -> Result<isize> {
        try_with!(
            self.main_thread().setregs(regs),
            "cannot set system call args"
        );
        loop {
            // FIXME: on arm we would need PTRACE_SET_SYSCALL
            // stops before sycall
            try_with!(self.main_thread().syscall(), "cannot run syscall in thread");

            let mut status = try_with!(waitpid(self.main_thread().tid, None), "waitpid failed");

            if let WaitStatus::Stopped(_, Signal::SIGTRAP) = status {
                // traps after sycall
                try_with!(self.main_thread().syscall(), "cannot run syscall in thread");
                status = try_with!(waitpid(self.main_thread().tid, None), "waitpid failed");
            }

            match status {
                WaitStatus::PtraceEvent(_, _, _) => {
                    bail!("got unexpected ptrace event")
                }
                WaitStatus::PtraceSyscall(_) => {
                    bail!("got unexpected ptrace syscall event")
                }
                WaitStatus::StillAlive => {
                    bail!("got unexpected still-alive waitpid() event")
                }
                WaitStatus::Continued(_) => {} // noop
                WaitStatus::Stopped(_, Signal::SIGTRAP) => {
                    let result_regs =
                        try_with!(self.main_thread().getregs(), "cannot syscall results");
                    assert!(self.saved_regs.ip() == result_regs.ip() - cpu::SYSCALL_SIZE as u64);
                    return Ok(result_regs.syscall_ret() as isize);
                }
                WaitStatus::Stopped(_, signal) => {
                    bail!("process was stopped by by signal: {}", signal);
                }
                WaitStatus::Exited(_, status) => bail!("process exited with: {}", status),
                WaitStatus::Signaled(_, signal, _) => {
                    bail!("process was stopped by signal: {}", signal)
                }
            }
        }
    }

    /// Depricated. TODO remove
    pub fn await_syscall(&self) -> Result<()> {
        // TODO spawn worker threads that do the same
        //for thread in &self.threads {
        //thread.syscall()?;
        //}

        self.main_thread().syscall()?;
        loop {
            let status = try_with!(waitpid(self.main_thread().tid, None), "cannot waitpid");

            match status {
                WaitStatus::PtraceEvent(_, _, _) => {
                    bail!("got unexpected ptrace event")
                }
                WaitStatus::PtraceSyscall(_) => {
                    bail!("got unexpected ptrace syscall event")
                }
                WaitStatus::StillAlive => {
                    bail!("got unexpected still-alive waitpid() event")
                }
                WaitStatus::Continued(_) => {
                    println!("WaitStatus::Continued");
                } // noop
                //WaitStatus::Stopped(_, Signal::SIGTRAP) => {
                //let regs =
                //try_with!(self.main_thread().getregs(), "cannot syscall results");
                //println!("syscall: eax {:x} ebx {:x}", regs.rax, regs.rbx);

                //return Ok(());
                //}
                WaitStatus::Stopped(pid, signal) => {
                    println!("process {} was stopped by by signal: {}", pid, signal);
                    let regs = try_with!(self.main_thread().getregs(), "cannot syscall results");
                    println!(
                        "syscall: eax {:x} ebx {:x} cs {:x}",
                        regs.rax, regs.rbx, regs.cs
                    );
                    let siginfo = try_with!(
                        nix::sys::ptrace::getsiginfo(self.main_thread().tid),
                        "cannot getsiginfo"
                    );
                    if (siginfo.si_code == libc::SIGTRAP)
                        || (siginfo.si_code == (libc::SIGTRAP | 0x80))
                    {
                        println!("siginfo.si_code true: 0x{:x}", siginfo.si_code);
                        return Ok(());
                    } else {
                        println!("siginfo.si_code false: 0x{:x}", siginfo.si_code);
                        //try_with!(nix::sys::ptrace::syscall(self.main_thread().tid, None), "cannot ptrace::syscall");
                    }
                    //bail!("process was stopped by by signal: {}", signal);
                    //self.main_thread().cont(Some(signal))?;
                    //return self.await_syscall();
                }
                WaitStatus::Exited(_, status) => bail!("process exited with: {}", status),
                WaitStatus::Signaled(_, signal, _) => {
                    bail!("process was stopped by signal: {}", signal)
                }
            }
        }

        Ok(())
    }

    fn main_thread(&self) -> &ptrace::Thread {
        &self.threads[self.process_idx]
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = unsafe {
            self.main_thread().write(
                self.saved_regs.ip() as *mut c_void,
                self.saved_text as *mut c_void,
            )
        };
        let _ = self.main_thread().setregs(&self.saved_regs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nix::{fcntl::OFlag, unistd::pipe2};
    use std::fs::File;
    use std::io::Write;
    use std::os::unix::io::FromRawFd;
    use std::path::Path;
    use std::process::Command;
    use std::process::Stdio;
    use tempfile::tempdir;

    fn compile_executable(source: &str, target: &Path) {
        let cc = std::env::var("CC").unwrap_or_else(|_| String::from("cc"));
        let args = &[
            "-xc",
            "-",
            "-g",
            "-Wall",
            "-o",
            target.to_str().unwrap(),
            "-pthread",
        ];
        println!("$ {} {}", cc, args.join(" "));
        let mut child = Command::new(cc)
            .args(args)
            .stdin(Stdio::piped())
            .spawn()
            .expect("cannot compile program");
        {
            let stdin = child.stdin.as_mut().expect("cannot get child stdin");
            stdin
                .write_all(source.as_bytes())
                .expect("cannot write stdin");
        }
        assert!(child.wait().expect("process failed").success());
    }

    #[test]
    fn test_syscall_inject() {
        let dir = tempdir().expect("cannot create tempdir");
        let binary = dir.path().join("main");
        compile_executable(
            r#"
#include <unistd.h>
#include <stdio.h>
int main() {
  int a; a = read(0, &a, sizeof(a));
  puts("OK");
  return 0;
}
"#,
            &binary,
        );
        let (readfd, writefd) = pipe2(OFlag::O_CLOEXEC).expect("cannot create pipe");
        let read_end = unsafe { Stdio::from_raw_fd(readfd) };
        let write_end = unsafe { File::from_raw_fd(writefd) };
        let child = Command::new(binary)
            .stdin(read_end)
            .stdout(Stdio::piped())
            .spawn()
            .expect("test program failed");
        let pid = Pid::from_raw(child.id() as i32);
        {
            let proc = attach(pid).expect("cannot attach with ptrace");
            assert_eq!(proc.getpid().expect("getpid failed"), pid.as_raw());
        }
        drop(write_end);
        let output = child
            .wait_with_output()
            .expect("could not read stdout")
            .stdout;
        assert_eq!(output, b"OK\n");
    }
}
