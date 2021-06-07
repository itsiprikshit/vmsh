use std::env;
use std::fs;
use std::os::unix::fs::symlink;
use std::process::Command;

use build_utils::{copy_out, log, run, stage_dir};

fn fallback_kernel_dir() -> String {
    let proc = Command::new("uname")
        .arg("-r")
        .output()
        .expect("uname command failed");
    if !proc.status.success() {
        match proc.status.code() {
            Some(code) => panic!("uname exited with status code: {}", code),
            None => panic!("uname terminated by signal"),
        }
    }
    let kernel_version = String::from_utf8(proc.stdout).expect("cannot decode uname output");
    format!("/lib/modules/{}/build", kernel_version.trim_end())
}

fn main() {
    if env::var("VMSH_SKIP_KERNEL_BUILD").unwrap_or_else(|_| String::from("0")) == "1" {
        return;
    }

    // Tell Cargo that if the given file changes, to rerun this build script.
    let srcs = [
        "build.rs",
        "module.c",
        "Makefile",
        "src/lib.rs",
        "src/printk.rs",
    ];
    for src in &srcs {
        // In theory this breaks paths on windows, but so does the linux build system.
        println!("cargo:rerun-if-changed=src/stage1/{}", src);
    }

    // Re-run build if kernel dir changes
    println!("rerun-if-env-changed=KERNELDIR");

    let kernel_dir = env::var("KERNELDIR").unwrap_or_else(|_| fallback_kernel_dir());

    let stage1_dir = stage_dir("stage1");

    log!("cd {} && cargo build --release", stage1_dir.display(),);

    run("cargo", |command| {
        command
            .arg("build")
            .arg("--release")
            .current_dir(&stage1_dir)
    });

    let libstage1_object = stage1_dir
        .join("target")
        .join("release")
        .join("libstage1.a");
    let libstage1_symlink = stage1_dir.join("libstage1.o");
    log!(
        "ln -sf {} {}",
        libstage1_object.display(),
        libstage1_symlink.display()
    );
    let _ = fs::remove_file(&libstage1_symlink);
    symlink(&libstage1_object, &libstage1_symlink).unwrap_or_else(|_| {
        panic!(
            "failed to symlink {} to {}",
            libstage1_object.display(),
            libstage1_symlink.display()
        )
    });

    log!(
        "make -C {} M={} RUST_DIR={}",
        kernel_dir,
        stage1_dir.display(),
        stage1_dir.display()
    );
    run("make", |command| {
        command
            .arg("-C")
            .arg(&kernel_dir)
            .arg(format!("M={}", stage1_dir.display()))
            .arg(format!("RUST_DIR={}", stage1_dir.display()))
    });

    copy_out(&stage1_dir.join("stage1.ko"));
}
