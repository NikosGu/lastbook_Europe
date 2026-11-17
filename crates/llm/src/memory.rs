//! This computer's memory: all of it, and what could be had without pushing other programs out (free plus what the system
//! would drop). The runner keeps its own copy of these few lines so it links nothing of the engine (which holds setup's
//! downloader): the runner has no network code at all.

#[cfg(target_os = "macos")]
pub fn ram_total() -> u64 {
    let mut v: u64 = 0;
    let mut n = std::mem::size_of::<u64>();
    let name = c"hw.memsize";
    let ok = unsafe { libc::sysctlbyname(name.as_ptr(), &mut v as *mut u64 as *mut libc::c_void, &mut n, std::ptr::null_mut(), 0) } == 0;
    if ok { v } else { 0 }
}

#[cfg(target_os = "macos")]
pub fn ram_free() -> u64 {
    #[allow(deprecated)]
    unsafe {
        let mut s: libc::vm_statistics64 = std::mem::zeroed();
        let mut count = (std::mem::size_of::<libc::vm_statistics64>() / std::mem::size_of::<libc::integer_t>()) as libc::mach_msg_type_number_t;
        let r = libc::host_statistics64(libc::mach_host_self(), libc::HOST_VM_INFO64, &mut s as *mut _ as libc::host_info64_t, &mut count);
        if r != libc::KERN_SUCCESS {
            return 0;
        }
        let page = libc::sysconf(libc::_SC_PAGESIZE) as u64;
        (s.free_count as u64 + s.inactive_count as u64 + s.purgeable_count as u64) * page
    }
}

#[cfg(target_os = "linux")]
fn meminfo(key: &str) -> u64 {
    std::fs::read_to_string("/proc/meminfo").ok().and_then(|t| {
        t.lines().find(|l| l.starts_with(key)).and_then(|l| l.split_whitespace().nth(1)).and_then(|kb| kb.parse::<u64>().ok())
    }).map(|kb| kb * 1024).unwrap_or(0)
}

#[cfg(target_os = "linux")]
pub fn ram_total() -> u64 {
    meminfo("MemTotal:")
}

#[cfg(target_os = "linux")]
pub fn ram_free() -> u64 {
    meminfo("MemAvailable:") // Linux's own estimate of the same
}

#[cfg(windows)]
#[repr(C)]
struct MemoryStatusEx {
    length: u32,
    memory_load: u32,
    total_phys: u64,
    avail_phys: u64,
    total_page_file: u64,
    avail_page_file: u64,
    total_virtual: u64,
    avail_virtual: u64,
    avail_extended_virtual: u64,
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
}

#[cfg(windows)]
fn status() -> Option<MemoryStatusEx> {
    let mut m: MemoryStatusEx = unsafe { std::mem::zeroed() };
    m.length = std::mem::size_of::<MemoryStatusEx>() as u32;
    (unsafe { GlobalMemoryStatusEx(&mut m) } != 0).then_some(m)
}

#[cfg(windows)]
pub fn ram_total() -> u64 {
    status().map(|m| m.total_phys).unwrap_or(0)
}

#[cfg(windows)]
pub fn ram_free() -> u64 {
    status().map(|m| m.avail_phys).unwrap_or(0)
}
