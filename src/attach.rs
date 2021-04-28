use crate::result::Result;
use log::*;
use nix::unistd::Pid;
use simple_error::try_with;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use vm_device::bus::MmioAddress;
use vm_virtio::device::VirtioDevice;
use vm_virtio::device::WithDriverSelect;
use vm_virtio::Queue;

use crate::device::{Device, DEVICE_MAX_MEM};
use crate::kvm::{self, hypervisor::Hypervisor};
use crate::tracer::wrap_syscall::KvmRunWrapper;

pub struct AttachOptions {
    pub pid: Pid,
    pub backing: PathBuf,
}

pub fn attach(opts: &AttachOptions) -> Result<()> {
    info!("attaching");

    let vm = Arc::new(try_with!(
        kvm::hypervisor::get_hypervisor(opts.pid),
        "cannot get vms for process {}",
        opts.pid
    ));
    vm.stop()?;

    // instanciate blkdev
    let device = try_with!(Device::new(&vm, &opts.backing), "cannot create vm");
    info!("mmio dev attached");

    // start monitoring thread
    let child = blkdev_monitor(&device);

    // run guest until driver has inited
    try_with!(
        run_kvm_wrapped(&vm, &device),
        "device init stage with KvmRunWrapper failed"
    );
    info!("blkdev queue ready.");
    vm.resume()?;

    info!("pause");
    nix::unistd::pause();
    let _err = child.join();
    Ok(())
}

fn blkdev_monitor(device: &Device) -> JoinHandle<()> {
    let blkdev = device.blkdev.clone();
    thread::spawn(move || loop {
        {
            let blkdev = blkdev.lock().unwrap();
            if blkdev.selected_queue().map(|q| q.ready).unwrap() {
                // blkdev queue ready
                break;
            }
            info!("");
            info!("dev type {}", blkdev.device_type());
            info!("dev features b{:b}", blkdev.device_features());
            info!(
                "dev interrupt stat b{:b}",
                blkdev
                    .interrupt_status()
                    .load(std::sync::atomic::Ordering::Relaxed)
            );
            info!("dev status b{:b}", blkdev.device_status());
            info!("dev config gen {}", blkdev.config_generation());
            info!(
                "dev selqueue max size {}",
                blkdev.selected_queue().map(Queue::max_size).unwrap()
            );
            info!(
                "dev selqueue ready {}",
                blkdev.selected_queue().map(|q| q.ready).unwrap()
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(1000));
    })
}

/// returns when blkdev queue is ready
fn run_kvm_wrapped(vm: &Arc<Hypervisor>, device: &Device) -> Result<()> {
    let mut mmio_mgr = device.mmio_mgr.lock().unwrap();

    vm.kvmrun_wrapped(|wrapper: &mut KvmRunWrapper| {
        let mmio_space = {
            let blkdev = device.blkdev.clone();
            let blkdev = &try_with!(blkdev.lock(), "TODO");
            blkdev.mmio_cfg.range
        };

        loop {
            let mut kvm_exit =
                try_with!(wrapper.wait_for_ioctl(), "failed to wait for vmm exit_mmio");
            if let Some(mmio_rw) = &mut kvm_exit {
                let addr = MmioAddress(mmio_rw.addr);
                let from = mmio_space.base();
                // virtio mmio space + virtio device specific space
                let to = from + mmio_space.size() + DEVICE_MAX_MEM;
                if from <= addr && addr < to {
                    // intercept op
                    debug!("mmio access 0x{:x}", addr.0);
                    try_with!(mmio_mgr.handle_mmio_rw(mmio_rw), "failed to handle MmioRw");
                } else {
                    // do nothing, just continue to ingore and pass to hv
                }
                {
                    let blkdev = device.blkdev.clone();
                    let blkdev = &try_with!(blkdev.lock(), "cannot get blkdev lock");
                    if blkdev.selected_queue().map(|q| q.ready).unwrap() {
                        // blkdev queue ready
                        break;
                    }
                }
            }
        }

        Ok(())
    })?;
    Ok(())
}
