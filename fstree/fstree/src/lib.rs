//! File system management implementation
//!
//! This module provides filesystem management functionality, including:
//! - File and directory operations
//! - Path manipulation and resolution
//! - Permission and ownership control
//! - Process filesystem context management
//!
//! # Core Components
//! - [`FsStruct`]: Main filesystem context structure for a process
//! - File and directory manipulation functions
//! - Path resolution and canonicalization
//!

#![no_std]

#[macro_use]
extern crate log;
extern crate alloc;

use axerrno::{ax_err, AxError, AxResult};
use alloc::{string::String, sync::Arc};
use axfs_vfs::{VfsNodeRef, VfsNodeType};
use spinpreempt::SpinLock;
use axfs_vfs::RootDirectory;
use axtype::O_NOFOLLOW;
use lazy_init::LazyInit;
use alloc::vec::Vec;

/// Represents the filesystem context for a process
pub struct FsStruct {
    /// Number of processes sharing this filesystem context
    pub users: i32,
    /// Indicates if the process is currently executing
    pub in_exec: bool,
    /// Current working directory path
    curr_path: String,
    /// Reference to current working directory node
    curr_dir: Option<VfsNodeRef>,
    /// Reference to root directory
    root_dir: Option<Arc<RootDirectory>>,
    /// File creation mask
    umask: u32,
}

impl FsStruct {
    // Creates a new filesystem context with default values
    pub fn new() -> Self {
        Self {
            users: 1,
            in_exec: false,
            curr_path: String::from("/"),
            curr_dir: None,
            root_dir: None,
            umask: 0,
        }
    }

    /// Initializes the filesystem context with a root directory
    pub fn init(&mut self, root_dir: Arc<RootDirectory>) {
        self.root_dir = Some(root_dir);
        self.curr_dir = Some(self.root_dir.as_ref().unwrap().clone());
        self.curr_path = "/".into();
    }

    /// Sets the file creation mask
    pub fn set_umask(&mut self, mode: u32) {
        self.umask = mode;
    }

    /// Copies filesystem context from another process
    pub fn copy_fs_struct(&mut self, fs: Arc<SpinLock<FsStruct>>) {
        let locked_fs = &fs.lock();
        self.root_dir = locked_fs.root_dir.as_ref().map(|root_dir| root_dir.clone());
        self.curr_dir = locked_fs.curr_dir.as_ref().map(|curr_dir| curr_dir.clone());
        self.curr_path = locked_fs.curr_path.clone();
    }

    /// Copies filesystem context from another process
    fn parent_node_of(&self, dir: Option<&VfsNodeRef>, path: &str) -> VfsNodeRef {
        if path.starts_with('/') {
            assert!(self.root_dir.is_some());
            self.root_dir.clone().unwrap()
        } else {
            dir.cloned().unwrap_or_else(|| self.curr_dir.clone().unwrap())
        }
    }

    /// Looks up a file or directory in the filesystem
    pub fn lookup(&self, dir: Option<&VfsNodeRef>, path: &str, flags: i32) -> AxResult<VfsNodeRef> {
        if path.is_empty() {
            return ax_err!(NotFound);
        }
        let (node, _) = self.parent_node_of(dir, path).lookup(path, flags)?;
        if path.ends_with('/') && !node.get_attr()?.is_dir() {
            ax_err!(NotADirectory)
        } else {
            Ok(node)
        }
    }

    /// Creates a hard link to an existing file
    pub fn create_link(
        &self, dir: Option<&VfsNodeRef>,
        path: &str, node: VfsNodeRef
    ) -> AxResult {
        if path.is_empty() {
            return ax_err!(NotFound);
        } else if path.ends_with('/') {
            return ax_err!(NotADirectory);
        }
        let parent = self.parent_node_of(dir, path);
        info!("create_link: {}", path);
        parent.link(path, node)
    }

    /// Creates a symbolic link
    pub fn create_symlink(
        &self, dir: Option<&VfsNodeRef>,
        path: &str, target: &str,
        uid: u32, gid: u32, mode: i32
    ) -> AxResult {
        if path.is_empty() {
            return ax_err!(NotFound);
        } else if path.ends_with('/') {
            return ax_err!(NotADirectory);
        }
        let parent = self.parent_node_of(dir, path);
        info!("create_symlink: {}", path);
        parent.symlink(path, target, uid, gid, mode)
    }

    /// Creates a new file or directory
    pub fn create_file(&self, dir: Option<&VfsNodeRef>, path: &str, ty: VfsNodeType, uid: u32, gid: u32, mode: i32) -> AxResult<VfsNodeRef> {
        info!("create_file: {} ..", path);
        if path.is_empty() {
            return ax_err!(NotFound);
        } else if path.ends_with('/') {
            return ax_err!(NotADirectory);
        }
        let parent = self.parent_node_of(dir, path);
        info!("create_file: step1");
        parent.create(path, ty, uid, gid, mode)?;
        let (node, _) = parent.lookup(path, 0)?;
        Ok(node)
    }

    /// Creates a new directory
    pub fn create_dir(&self, dir: Option<&VfsNodeRef>, path: &str, uid: u32, gid: u32, mode: i32) -> AxResult {
        if path.is_empty() {
            return ax_err!(InvalidInput);
        }
    
        if let Ok(_) = self.lookup(dir, path, 0) {
            return ax_err!(AlreadyExists);
        }
    
        let components: Vec<&str> = path.trim_matches('/')
                                       .split('/')
                                       .filter(|s| !s.is_empty())
                                       .collect();
                                    
        debug!("create_dir: {:?} ..", components);
        
        if components.is_empty() {
            return ax_err!(InvalidInput);
        }

        // 获取父目录路径
        let parent_path = if components.len() > 1 {
            components[..components.len()-1].join("/")
        } else {
            String::new()
        };

        // 检查父目录
        if !parent_path.is_empty() {
            match self.lookup(dir, &parent_path, 0) {
                Ok(node) => {
                    // 确保是目录
                    if !node.get_attr()?.is_dir() {
                        return ax_err!(NotADirectory);
                    }
                },
                Err(_) => return ax_err!(NotFound), // 父目录不存在且非递归模式
            }
        }
        
        // 在已存在的父目录下创建目标目录
        match self.lookup(dir, path, 0) {
            Ok(_) => ax_err!(AlreadyExists),
            Err(AxError::NotFound) => self.parent_node_of(dir, path).create(path, VfsNodeType::Dir, uid, gid, mode),
            Err(e) => Err(e),
        }
    }

    /// Returns reference to the root directory
    pub fn root_dir(&self) -> Option<Arc<RootDirectory>> {
        self.root_dir.clone()
    }

    /// Returns reference to the current working directory
    pub fn current_dir(&self) -> AxResult<String> {
        Ok(self.curr_path.clone())
    }

    /// Returns absolute path from a possibly relative path
    pub fn absolute_path(&self, path: &str) -> AxResult<String> {
        if path.starts_with('/') {
            Ok(axfs_vfs::path::canonicalize(path))
        } else {
            let path = self.curr_path.clone() + path;
            Ok(axfs_vfs::path::canonicalize(&path))
        }
    }

    /// Changes current working directory
    pub fn set_current_dir(&mut self, path: &str) -> AxResult {
        let mut abs_path = self.absolute_path(path)?;
        if !abs_path.ends_with('/') {
            abs_path += "/";
        }
        if abs_path == "/" {
            self.curr_dir = Some(self.root_dir.as_ref().unwrap().clone());
            self.curr_path = "/".into();
            return Ok(());
        }

        let node = self.lookup(None, &abs_path, 0)?;
        let attr = node.get_attr()?;
        if !attr.is_dir() {
            ax_err!(NotADirectory)
        } else if !attr.perm().owner_executable() {
            ax_err!(PermissionDenied)
        } else {
            self.curr_dir = Some(node);
            self.curr_path = abs_path;
            Ok(())
        }
    }

    /// Removes a file
    pub fn remove_file(&self, dir: Option<&VfsNodeRef>, path: &str) -> AxResult {
        let node = self.lookup(dir, path, O_NOFOLLOW)?;
        let attr = node.get_attr()?;
        if attr.is_dir() {
            ax_err!(IsADirectory)
        } else {
            self.parent_node_of(dir, path).remove(path)
        }
    }

    /// Removes a directory
    pub fn remove_dir(&self, dir: Option<&VfsNodeRef>, path: &str) -> AxResult {
        if path.is_empty() {
            return ax_err!(NotFound);
        }
        let path_check = path.trim_matches('/');
        if path_check.is_empty() {
            return ax_err!(DirectoryNotEmpty); // rm -d '/'
        } else if path_check == "."
            || path_check == ".."
            || path_check.ends_with("/.")
            || path_check.ends_with("/..")
        {
            return ax_err!(InvalidInput);
        }
        if self.root_dir.as_ref().unwrap().contains(&self.absolute_path(path)?) {
            return ax_err!(PermissionDenied);
        }

        let node = self.lookup(dir, path, 0)?;
        let attr = node.get_attr()?;
        if !attr.is_dir() {
            ax_err!(NotADirectory)
        } else if !attr.perm().owner_writable() {
            ax_err!(PermissionDenied)
        } else {
            self.parent_node_of(dir, path).remove(path)
        }
    }

    /// Renames a file or directory
    pub fn rename(&self, old: &str, new: &str) -> AxResult {
        if self.parent_node_of(None, new).lookup(new, 0).is_ok() {
            warn!("dst file already exist, now remove it");
            self.remove_file(None, new)?;
        }
        self.parent_node_of(None, old).rename(old, new)
    }
}

/// Returns reference to the initialized filesystem context
pub fn init_fs() -> Arc<SpinLock<FsStruct>> {
    INIT_FS.clone()
}

/// Initializes the filesystem subsystem
pub fn init(cpu_id: usize, dtb_pa: usize) {
    axconfig::init_once!();
    info!("Initialize fstree ...");

    axhal::arch_init_early(cpu_id);
    axalloc::init();
    page_table::init();

    spinpreempt::init(cpu_id, dtb_pa);

    axmount::init(cpu_id, dtb_pa);
    let root_dir = axmount::init_root();
    let mut fs = FsStruct::new();
    fs.init(root_dir);

    INIT_FS.init_by(Arc::new(SpinLock::new(fs)));
}

static INIT_FS: LazyInit<Arc<SpinLock<FsStruct>>> = LazyInit::new();
