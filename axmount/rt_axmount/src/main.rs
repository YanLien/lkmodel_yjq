#![no_std]
#![no_main]

#[macro_use]
extern crate axlog2;

use core::str::from_utf8;
use core::panic::PanicInfo;
use axfs_vfs::VfsNodeType;
use axfile::fops::{File, OpenOptions};

/// Entry
#[no_mangle]
pub extern "Rust" fn runtime_main(cpu_id: usize, dtb_pa: usize) {
    assert_eq!(cpu_id, 0);

    axlog2::init("debug");
    info!("[rt_axmount]: ... cpuid {}", cpu_id);

    axhal::cpu::init_primary(cpu_id);

    info!("Initialize global memory allocator...");
    axalloc::init();

    info!("Initialize kernel page table...");
    page_table::init();

    fstree::init(cpu_id, dtb_pa);
    let fs = fstree::init_fs();
    let locked_fs = fs.lock();

    match locked_fs.create_dir(None, "/testss/abc", 0, 0, 0o777) {
        Ok(_) => info!("create /testss/abc ok!"),
        Err(e) => error!("create /testss/abc failed {}", e),
    }

    match locked_fs.create_dir(None, "/testss", 0, 0, 0o777) {
        Ok(_) => info!("create /testss ok!"),
        Err(e) => error!("create /testss failed {}", e),
    }

    match locked_fs.create_dir(None, "/testss/abc", 0, 0, 0o777) {
        Ok(_) => info!("create /testss/abc ok!"),
        Err(e) => error!("create /testss/abc failed {}", e),
    }

    let fname = "/testss/abc/new-file.txt";
    info!("test create file {:?}:", fname);
    let contents = "create a new file!\n";

    match locked_fs.create_file(None, fname, VfsNodeType::File, 0, 0, 0o644) {
        Ok(wfile) => {
            info!("create {:?} ok!", fname);
            wfile.write_at(0, contents.as_bytes()).unwrap();
        },
        Err(e) => error!("create {:?} failed {}", fname, e),
    }

    let mut opts = OpenOptions::new();
    opts.read(true);
    let mut rfile = File::open(fname, &opts, &locked_fs, 0, 0).unwrap();
    let mut buf = [0u8; 256];
    match rfile.read(&mut buf) {
        Ok(len) => {
            info!("read test file: \"{:?}\". len {}", from_utf8(&buf[..len]), len);
            assert_eq!(contents.as_bytes(), &buf[..len]);
        },
        Err(e) => error!("read test file failed {}", e),
    };

    assert!(locked_fs.remove_file(None, fname).is_ok());
    assert!(locked_fs.remove_dir(None, "/testss/abc").is_ok());

    info!("[rt_axmount]: ok!");
    axhal::misc::terminate();
}

#[panic_handler]
pub fn panic(info: &PanicInfo) -> ! {
    error!("{}", info);
    arch_boot::panic(info)
}

extern "C" {
    fn _ekernel();
}
