//! What this computer has: system, processor type, memory, graphics memory and room on each disk. No subprocess for any of
//! it: the system's own calls (sysctl on a Mac, DXGI on Windows, /proc and /sys on Linux).

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const GIB: u64 = 1 << 30;
/// Memory left for the system and the rest of the app beyond what the model needs (as the AI's own check keeps).
pub const RESERVE: u64 = 5 * GIB / 2;
/// Graphics memory the estimate leaves for the card's own work.
pub const VRAM_KEPT: u64 = GIB;

pub fn os_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        std::env::consts::OS
    }
}

pub fn arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" | "arm64" => "arm64",
        "x86_64" => "x64",
        other => other,
    }
}

// --------------------------------------------------------------------------------------------------------------- Mac --
#[cfg(target_os = "macos")]
fn sysctl_u64(name: &str) -> Option<u64> {
    let c = std::ffi::CString::new(name).ok()?;
    let mut v: u64 = 0;
    let mut n = std::mem::size_of::<u64>();
    let rc = unsafe { libc::sysctlbyname(c.as_ptr(), &mut v as *mut u64 as *mut libc::c_void, &mut n, std::ptr::null_mut(), 0) };
    (rc == 0).then_some(v)
}

#[cfg(target_os = "macos")]
fn sysctl_string(name: &str) -> Option<String> {
    let c = std::ffi::CString::new(name).ok()?;
    let mut n: usize = 0;
    if unsafe { libc::sysctlbyname(c.as_ptr(), std::ptr::null_mut(), &mut n, std::ptr::null_mut(), 0) } != 0 || n == 0 {
        return None;
    }
    let mut buf = vec![0u8; n];
    if unsafe { libc::sysctlbyname(c.as_ptr(), buf.as_mut_ptr() as *mut libc::c_void, &mut n, std::ptr::null_mut(), 0) } != 0 {
        return None;
    }
    buf.truncate(buf.iter().position(|&b| b == 0).unwrap_or(n));
    String::from_utf8(buf).ok()
}

// ----------------------------------------------------------------------------------------------------------- Windows --
#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

    pub fn ram() -> Option<u64> {
        let mut st: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        st.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        (unsafe { GlobalMemoryStatusEx(&mut st) } != 0).then_some(st.ullTotalPhys)
    }

    #[repr(C)]
    struct Guid(u32, u16, u16, [u8; 8]);

    #[repr(C)]
    struct AdapterDesc1 {
        description: [u16; 128],
        vendor_id: u32,
        device_id: u32,
        sub_sys_id: u32,
        revision: u32,
        dedicated_video_memory: usize,
        dedicated_system_memory: usize,
        shared_system_memory: usize,
        luid: u64,
        flags: u32,
    }

    type CreateFactory = unsafe extern "system" fn(*const Guid, *mut *mut c_void) -> i32;
    type EnumAdapters1 = unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> i32;
    type GetDesc1 = unsafe extern "system" fn(*mut c_void, *mut AdapterDesc1) -> i32;
    type Release = unsafe extern "system" fn(*mut c_void) -> u32;

    unsafe fn method<T: Copy>(obj: *mut c_void, index: usize) -> T {
        let vtbl = *(obj as *const *const usize);
        std::mem::transmute_copy(&*vtbl.add(index))
    }

    /// Each real graphics adapter (not Microsoft's software one) with its dedicated memory, from IDXGIFactory1::EnumAdapters1.
    /// dxgi.dll is loaded from System32 only.
    pub fn gpus() -> Vec<(String, u64)> {
        use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};
        let mut out = Vec::new();
        unsafe {
            let name: Vec<u16> = "dxgi.dll\0".encode_utf16().collect();
            let lib = LoadLibraryExW(name.as_ptr(), std::ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32);
            if lib.is_null() {
                return out;
            }
            let Some(create) = GetProcAddress(lib, b"CreateDXGIFactory1\0".as_ptr()) else { return out };
            let create: CreateFactory = std::mem::transmute(create);
            let iid = Guid(0x770aae78, 0xf26f, 0x4dba, [0xa8, 0x29, 0x25, 0x3c, 0x83, 0xd1, 0xb3, 0x87]); // IID_IDXGIFactory1
            let mut factory: *mut c_void = std::ptr::null_mut();
            if create(&iid, &mut factory) != 0 || factory.is_null() {
                return out;
            }
            for i in 0..16u32 {
                let mut adapter: *mut c_void = std::ptr::null_mut();
                if method::<EnumAdapters1>(factory, 12)(factory, i, &mut adapter) != 0 || adapter.is_null() {
                    break;
                }
                let mut d: AdapterDesc1 = std::mem::zeroed();
                if method::<GetDesc1>(adapter, 10)(adapter, &mut d) == 0 && d.flags & 2 == 0 {
                    let len = d.description.iter().position(|&c| c == 0).unwrap_or(128);
                    out.push((String::from_utf16_lossy(&d.description[..len]).trim().to_string(), d.dedicated_video_memory as u64));
                }
                method::<Release>(adapter, 2)(adapter);
            }
            method::<Release>(factory, 2)(factory);
        }
        out
    }
}

// ------------------------------------------------------------------------------------------------------------- Linux --
#[cfg(target_os = "linux")]
fn linux_ram() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = text.lines().find(|l| l.starts_with("MemTotal:"))?;
    line.split_whitespace().nth(1)?.parse::<u64>().ok().map(|kb| kb * 1024)
}

/// NVIDIA cards with their memory, from the driver's own files; AMD through amdgpu's mem_info_vram_total; integrated Intel
/// graphics share the computer's memory and are left out. The AI's own measure decides once the model is here.
#[cfg(target_os = "linux")]
fn linux_gpus() -> Vec<(String, u64)> {
    let mut out = Vec::new();
    let mut cards: Vec<PathBuf> = std::fs::read_dir("/sys/class/drm").into_iter().flatten().flatten().map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("card") && !n.to_string_lossy().contains('-'))).collect();
    cards.sort();
    for card in cards {
        let dev = card.join("device");
        let Ok(vendor) = std::fs::read_to_string(dev.join("vendor")) else { continue };
        let vendor = vendor.trim().to_string();
        let mut vram: u64 = std::fs::read_to_string(dev.join("mem_info_vram_total")).ok().and_then(|v| v.trim().parse().ok()).unwrap_or(0);
        let mut name = match vendor.as_str() {
            "0x10de" => "NVIDIA".to_string(),
            "0x1002" => "AMD".to_string(),
            "0x8086" => "Intel".to_string(),
            v => v.to_string(),
        };
        if vendor == "0x10de" {
            if let Ok(real) = dev.canonicalize() {
                let info = Path::new("/proc/driver/nvidia/gpus").join(real.file_name().unwrap_or_default()).join("information");
                if let Ok(text) = std::fs::read_to_string(info) {
                    if let Some(m) = text.lines().find_map(|l| l.strip_prefix("Model:")) {
                        name = m.trim().to_string();
                    }
                }
            }
            if vram == 0 {     // the PCI BAR the card maps for its memory, when that is the full size (resizable BAR)
                let big = std::fs::read_to_string(dev.join("resource")).unwrap_or_default().lines().filter_map(|l| {
                    let v: Vec<u64> = l.split_whitespace().filter_map(|x| u64::from_str_radix(x.trim_start_matches("0x"), 16).ok()).collect();
                    (v.len() >= 2 && v[1] > v[0]).then(|| v[1] - v[0] + 1)
                }).max().unwrap_or(0);
                vram = if big >= 2 * GIB { big } else { 0 };
            }
        }
        if vendor == "0x8086" && vram == 0 {
            continue;
        }
        out.push((name, vram));
    }
    out
}

/// The model name of an Apple silicon computer running Linux, else None: its graphics share the one memory.
#[cfg(target_os = "linux")]
fn apple_silicon_linux() -> Option<String> {
    let compatible = std::fs::read("/proc/device-tree/compatible").ok()?;
    if !compatible.split(|&b| b == 0).any(|c| c.starts_with(b"apple,")) {
        return None;
    }
    let model = std::fs::read("/proc/device-tree/model").ok()?;
    let s = String::from_utf8_lossy(model.split(|&b| b == 0).next().unwrap_or(&[])).into_owned();
    Some(if s.is_empty() { "Apple silicon".into() } else { s })
}

/// What the local AI runner on Linux can use: 'cuda' with an NVIDIA driver loaded, 'vulkan' with a Vulkan loader, else 'cpu'.
pub fn linux_ai_runtime() -> &'static str {
    if Path::new("/proc/driver/nvidia/version").is_file() {
        return "cuda";
    }
    for d in ["/usr/lib/x86_64-linux-gnu", "/usr/lib/aarch64-linux-gnu", "/usr/lib64", "/usr/lib"] {
        if Path::new(d).join("libvulkan.so.1").exists() {
            return "vulkan";
        }
    }
    "cpu"
}

// ------------------------------------------------------------------------------------------------------------- disks --
fn existing(path: &Path) -> PathBuf {
    let mut p = path.to_path_buf();
    while !p.exists() {
        match p.parent() {
            Some(q) if q != p => p = q.to_path_buf(),
            _ => break,
        }
    }
    p
}

/// (free, total) bytes on the disk that holds path, or the nearest folder above it that exists.
pub fn disk_of(path: &Path) -> (u64, u64) {
    let p = existing(path);
    #[cfg(unix)]
    {
        let Ok(c) = std::ffi::CString::new(p.to_string_lossy().as_bytes()) else { return (0, 0) };
        let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
            return (0, 0);
        }
        #[allow(clippy::unnecessary_cast)]
        let unit = st.f_frsize as u64;
        #[allow(clippy::unnecessary_cast)]
        return (st.f_bavail as u64 * unit, st.f_blocks as u64 * unit);
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        let wide: Vec<u16> = p.as_os_str().to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
        let (mut free, mut total, mut total_free) = (0u64, 0u64, 0u64);
        if unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, &mut total, &mut total_free) } == 0 {
            return (0, 0);
        }
        return (free, total);
    }
    #[allow(unreachable_code)]
    (0, 0)
}

/// What the system calls the disk holding path: the volume's name on a Mac (Macintosh HD), "Label (D:)" on Windows.
pub fn disk_name(path: &Path) -> Option<String> {
    let p = existing(path);
    #[cfg(target_os = "macos")]
    {
        let c = std::ffi::CString::new(p.to_string_lossy().as_bytes()).ok()?;
        let mut fs: libc::statfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statfs(c.as_ptr(), &mut fs) } != 0 {
            return None;
        }
        let mount = unsafe { std::ffi::CStr::from_ptr(fs.f_mntonname.as_ptr()) }.to_owned();
        #[repr(C)]
        struct Buf {
            length: u32,
            name: libc::attrreference_t,
            data: [u8; 1024],
        }
        let mut al: libc::attrlist = unsafe { std::mem::zeroed() };
        al.bitmapcount = libc::ATTR_BIT_MAP_COUNT;
        al.volattr = libc::ATTR_VOL_INFO | libc::ATTR_VOL_NAME;
        let mut buf: Buf = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::getattrlist(mount.as_ptr(), &mut al as *mut _ as *mut libc::c_void, &mut buf as *mut _ as *mut libc::c_void, std::mem::size_of::<Buf>(), 0) };
        if rc != 0 {
            return None;
        }
        let start = (&buf.name as *const _ as usize - &buf as *const _ as usize) as isize + buf.name.attr_dataoffset as isize;
        let len = buf.name.attr_length as usize;
        let bytes = unsafe { std::slice::from_raw_parts((&buf as *const Buf as *const u8).offset(start), len) };
        let s = String::from_utf8_lossy(bytes.split(|&b| b == 0).next().unwrap_or(&[])).into_owned();
        return (!s.is_empty()).then_some(s);
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Storage::FileSystem::GetVolumeInformationW;
        let s = p.to_string_lossy().to_string();
        let drive: String = s.trim_start_matches(r"\\?\").chars().take(2).collect();
        if !drive.ends_with(':') {
            return None;
        }
        let root: Vec<u16> = format!("{drive}\\").encode_utf16().chain(std::iter::once(0)).collect();
        let mut label = [0u16; 261];
        let ok = unsafe { GetVolumeInformationW(root.as_ptr(), label.as_mut_ptr(), 261, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), 0) };
        if ok == 0 {
            return None;
        }
        let n = label.iter().position(|&c| c == 0).unwrap_or(0);
        let name = String::from_utf16_lossy(&label[..n]);
        return Some(format!("{} ({drive})", if name.is_empty() { "Local Disk".to_string() } else { name }));
    }
    #[allow(unreachable_code)]
    {
        let _ = p;
        None
    }
}

#[cfg(unix)]
fn device_of(p: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(p).ok().map(|m| m.dev())
}

#[cfg(windows)]
fn device_of(p: &Path) -> Option<u64> {
    let s = p.to_string_lossy().to_string();
    let drive = s.trim_start_matches(r"\\?\").chars().next()?;
    Some(drive.to_ascii_uppercase() as u64)
}

/// Disk capacity for app-owned or explicitly chosen paths only, one entry per disk.
pub fn candidate_disks(extra: &[PathBuf]) -> Vec<Value> {
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for p in extra {
        let q = existing(p);
        let Some(dev) = device_of(&q) else { continue };
        if seen.contains(&dev) {
            continue;
        }
        seen.push(dev);
        let (free, total) = disk_of(&q);
        out.push(json!({"path": p.to_string_lossy(), "name": disk_name(&q), "free_bytes": free, "total_bytes": total}));
    }
    out
}

pub fn same_disk(a: &Path, b: &Path) -> bool {
    matches!((device_of(&existing(a)), device_of(&existing(b))), (Some(x), Some(y)) if x == y)
}

// ------------------------------------------------------------------------------------------------------------ the AI --
fn gbs(n: u64) -> String {
    let g = n as f64 / 1e9;
    if g >= 10.0 {
        format!("{g:.0} GB")
    } else {
        format!("{g:.1} GB")
    }
}

/// Before the model file is here to measure: the model and its context need memory_bytes, plus RESERVE. On a Mac that is
/// all one memory; elsewhere the part that fits on the graphics card does not count against the computer's own.
pub fn estimate_ai(ram: Option<u64>, gpus: &[Value], unified: bool, memory_bytes: u64) -> Value {
    let needs = memory_bytes + RESERVE;
    let Some(ram) = ram else {
        return json!({"fits": false, "needs_bytes": needs, "estimate": true, "why": "Lastbook could not tell how much memory this computer has."});
    };
    let in_ram = if unified || gpus.is_empty() {
        needs
    } else {
        let vram = gpus.iter().filter_map(|g| g["vram_bytes"].as_u64()).max().unwrap_or(0);
        memory_bytes.saturating_sub(vram.saturating_sub(VRAM_KEPT)) + RESERVE
    };
    let fits = ram >= in_ram;
    let why = if fits {
        format!("The local AI needs about {} of memory, and this computer has {}{}", gbs(needs), gbs(ram), if unified || gpus.is_empty() { "." } else { " plus its graphics memory." })
    } else {
        format!("The local AI needs about {} of this computer’s memory, and it has {}. Lastbook works without it: search and reading are the same.", gbs(in_ram), gbs(ram))
    };
    json!({"fits": fits, "needs_bytes": needs, "estimate": true, "why": why})
}

/// Everything but the AI's fit, which the caller adds.
#[allow(unused_assignments, unused_mut)]
pub fn detect(extra_disks: &[PathBuf]) -> Value {
    let name = os_name();
    let mut ram = None;
    let mut gpus: Vec<Value> = Vec::new();
    let mut unified = false;
    #[cfg(target_os = "macos")]
    {
        ram = sysctl_u64("hw.memsize");
        unified = arch() == "arm64";
        if let Some(chip) = sysctl_string("machdep.cpu.brand_string") {
            gpus.push(json!({"name": chip, "vram_bytes": 0})); // Apple silicon: the graphics share the one memory
        }
    }
    #[cfg(windows)]
    {
        ram = win::ram();
        gpus = win::gpus().into_iter().map(|(n, v)| json!({"name": n, "vram_bytes": v})).collect();
    }
    #[cfg(target_os = "linux")]
    {
        ram = linux_ram();
        gpus = linux_gpus().into_iter().map(|(n, v)| json!({"name": n, "vram_bytes": v})).collect();
        if let Some(model) = apple_silicon_linux() {
            gpus = vec![json!({"name": model, "vram_bytes": 0})];
            unified = true;
        }
    }
    let mut out = json!({"os": name, "arch": arch(), "ram_bytes": ram, "unified_memory": unified, "gpus": gpus, "disks": candidate_disks(extra_disks)});
    if name == "linux" {
        out["ai_runtime"] = json!(linux_ai_runtime());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_computer() {
        let tmp = std::env::temp_dir();
        let hw = detect(&[tmp.clone()]);
        assert!(["macos", "windows", "linux"].contains(&hw["os"].as_str().unwrap()));
        assert!(hw["ram_bytes"].as_u64().unwrap() > GIB);
        assert!(hw["disks"][0]["free_bytes"].as_u64().unwrap() > 0);
        let (free, total) = disk_of(&tmp.join("not/yet/there"));
        assert!(free > 0 && total >= free);
        if cfg!(target_os = "macos") {
            assert!(disk_name(&tmp).is_some_and(|n| !n.is_empty()));
        }
    }

    #[test]
    fn estimates() {
        let fits = estimate_ai(Some(32 * GIB), &[], true, 19_027_384_256);
        assert_eq!(fits["fits"], true);
        let small = estimate_ai(Some(16 * GIB), &[], true, 19_027_384_256);
        assert_eq!(small["fits"], false);
        assert!(small["why"].as_str().unwrap().contains("works without it"));
        let card = [json!({"name": "NVIDIA", "vram_bytes": 12 * GIB})];
        assert_eq!(estimate_ai(Some(16 * GIB), &card, false, 19_027_384_256)["fits"], true);   // the card takes most of it
        assert_eq!(estimate_ai(None, &[], true, 1)["fits"], false);
    }
}
