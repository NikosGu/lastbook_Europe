//! The disks a library can live on: this computer's own, external disks and USB sticks, and network shares (a NAS reached
//! by Wi-Fi or cable, through SMB or NFS). Setup shows each with its kind, and the engine reads a library from any of them.
//! Nothing here is sent anywhere: the list is shown to the person, on this computer.

use std::path::{Path, PathBuf};

/// "local" (this computer's own disk), "removable" (an external disk or a USB stick) or "network" (a share on a NAS or
/// another computer).
pub fn kind(path: &Path) -> &'static str {
    let p = super::hardware::existing(path);
    imp::kind(&p)
}

/// The disks mounted now that a library could go on: writable, and not the system's own hidden volumes.
pub fn mounted() -> Vec<PathBuf> {
    imp::mounted()
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::CStr;
    use std::path::{Path, PathBuf};

    const NETWORK_FS: &[&str] = &["smbfs", "nfs", "afpfs", "webdav", "cifs", "ftp"];

    fn fs_of(st: &libc::statfs) -> (String, String, u32) {
        let mount = unsafe { CStr::from_ptr(st.f_mntonname.as_ptr()) }.to_string_lossy().into_owned();
        let fstype = unsafe { CStr::from_ptr(st.f_fstypename.as_ptr()) }.to_string_lossy().into_owned();
        (mount, fstype, st.f_flags)
    }

    fn classify(mount: &str, fstype: &str, flags: u32) -> &'static str {
        if flags & libc::MNT_LOCAL as u32 == 0 || NETWORK_FS.contains(&fstype) {
            "network"
        } else if mount.starts_with("/Volumes/") {
            "removable"
        } else {
            "local"
        }
    }

    pub fn kind(p: &Path) -> &'static str {
        let Ok(c) = std::ffi::CString::new(p.to_string_lossy().as_bytes()) else { return "local" };
        let mut st: libc::statfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statfs(c.as_ptr(), &mut st) } != 0 {
            return "local";
        }
        let (mount, fstype, flags) = fs_of(&st);
        classify(&mount, &fstype, flags)
    }

    pub fn mounted() -> Vec<PathBuf> {
        let mut list: *mut libc::statfs = std::ptr::null_mut();
        let n = unsafe { libc::getmntinfo(&mut list, libc::MNT_NOWAIT) };
        if n <= 0 || list.is_null() {
            return Vec::new();
        }
        let all = unsafe { std::slice::from_raw_parts(list, n as usize) };
        all.iter().filter_map(|st| {
            let (mount, fstype, flags) = fs_of(st);
            let name = mount.rsplit('/').next().unwrap_or("");
            let writable = flags & libc::MNT_RDONLY as u32 == 0; // a disk image you opened is read-only
            (writable && !name.starts_with('.') && (mount.starts_with("/Volumes/") || classify(&mount, &fstype, flags) == "network")).then(|| PathBuf::from(mount))
        }).collect()
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::path::{Path, PathBuf};

    const NETWORK_FS: &[&str] = &["cifs", "smb3", "smbfs", "nfs", "nfs4", "fuse.sshfs", "fuse.rclone", "davfs", "fuse.davfs2", "9p", "glusterfs", "ceph"];
    const REMOVABLE_UNDER: &[&str] = &["/media/", "/run/media/", "/mnt/"];

    /// (mount point, file system type, read-only) of every mount, from /proc/self/mounts (spaces written \040).
    fn mounts() -> Vec<(String, String, bool)> {
        std::fs::read_to_string("/proc/self/mounts").unwrap_or_default().lines().filter_map(|l| {
            let f: Vec<&str> = l.split(' ').collect();
            (f.len() >= 4).then(|| (f[1].replace("\\040", " "), f[2].to_string(), f[3].split(',').any(|o| o == "ro")))
        }).collect()
    }

    fn classify(mount: &str, fstype: &str) -> &'static str {
        if NETWORK_FS.contains(&fstype) {
            "network"
        } else if REMOVABLE_UNDER.iter().any(|r| mount.starts_with(r)) {
            "removable"
        } else {
            "local"
        }
    }

    pub fn kind(p: &Path) -> &'static str {
        let p = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        let s = p.to_string_lossy();
        // the mount the path is on: the longest mount point it starts with
        mounts().into_iter().filter(|(m, _, _)| s == m.as_str() || s.starts_with(&format!("{}/", m.trim_end_matches('/'))) || m == "/")
            .max_by_key(|(m, _, _)| m.len()).map(|(m, t, _)| classify(&m, &t)).unwrap_or("local")
    }

    pub fn mounted() -> Vec<PathBuf> {
        mounts().into_iter().filter(|(m, t, ro)| !ro && (REMOVABLE_UNDER.iter().any(|r| m.starts_with(r)) || NETWORK_FS.contains(&t.as_str())))
            .map(|(m, _, _)| PathBuf::from(m)).collect()
    }
}

#[cfg(windows)]
mod imp {
    use std::path::{Path, PathBuf};
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};

    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;
    const DRIVE_REMOTE: u32 = 4;

    fn drive_type(root: &str) -> u32 {
        let w: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe { GetDriveTypeW(w.as_ptr()) }
    }

    pub fn kind(p: &Path) -> &'static str {
        let s = p.to_string_lossy().replace('/', "\\");
        let s = s.strip_prefix(r"\\?\UNC\").map(|r| format!(r"\\{r}")).unwrap_or_else(|| s.trim_start_matches(r"\\?\").to_string());
        if s.starts_with(r"\\") {
            return "network"; // \\server\share
        }
        let drive: String = s.chars().take(2).collect();
        if !drive.ends_with(':') {
            return "local";
        }
        match drive_type(&format!("{drive}\\")) {
            DRIVE_REMOTE => "network",
            DRIVE_REMOVABLE => "removable",
            _ => "local",
        }
    }

    pub fn mounted() -> Vec<PathBuf> {
        let mask = unsafe { GetLogicalDrives() };
        (0..26u8).filter(|i| mask & (1 << i) != 0).map(|i| format!("{}:\\", (b'A' + i) as char))
            .filter(|root| matches!(drive_type(root), DRIVE_REMOVABLE | DRIVE_FIXED | DRIVE_REMOTE)).map(PathBuf::from).collect()
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
mod imp {
    use std::path::{Path, PathBuf};
    pub fn kind(_p: &Path) -> &'static str {
        "local"
    }
    pub fn mounted() -> Vec<PathBuf> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_computer() {
        // the temporary folder is on this computer's own disk on every system the tests run on
        assert_eq!(kind(&std::env::temp_dir()), "local");
        assert_eq!(kind(&std::env::temp_dir().join("not/yet/there")), "local");
        for m in mounted() {
            assert!(["local", "removable", "network"].contains(&kind(&m)), "{m:?}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_unc_path_is_network() {
        assert_eq!(kind(Path::new(r"\\nas\share\LastbookData")), "network");
        assert_eq!(kind(Path::new(r"\\?\UNC\nas\share\LastbookData")), "network");
    }
}
