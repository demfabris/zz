use std::{ffi::OsString, path::PathBuf, time::Duration};

#[cfg(windows)]
pub type ProcessOwner = String;
#[cfg(not(windows))]
pub type ProcessOwner = u32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessRecord {
    pub pid: u32,
    pub start_time: u64,
    pub executable: Option<PathBuf>,
    pub arguments: Vec<OsString>,
    pub owner: Option<ProcessOwner>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessSample {
    pub pid: u32,
    pub parent: Option<u32>,
    pub name: OsString,
    pub resident_bytes: u64,
    pub virtual_bytes: u64,
    pub cpu_time: Duration,
    pub threads: u32,
    pub start_time: u64,
}

pub fn start_time(pid: u32) -> Option<u64> {
    platform::start_time(pid)
}

pub fn record(pid: u32) -> Option<ProcessRecord> {
    platform::record(pid)
}

pub fn command_name(pid: u32) -> Option<OsString> {
    platform::command_name(pid)
}

pub fn working_directory(pid: u32) -> Option<PathBuf> {
    platform::working_directory(pid)
}

pub fn terminate(pid: u32) -> bool {
    platform::terminate(pid)
}

pub fn sample(pid: u32) -> Option<ProcessSample> {
    platform::sample(pid)
}

pub fn descendants(pid: u32) -> Vec<u32> {
    platform::descendants(pid)
}

pub fn host_name() -> Option<String> {
    platform::host_name()
}

#[cfg(unix)]
#[allow(unsafe_code, clippy::undocumented_unsafe_blocks)]
fn unix_host_name() -> Option<String> {
    let mut buffer = [0u8; 256];
    if unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) } != 0 {
        return None;
    }
    let end = buffer
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(buffer.len());
    String::from_utf8(buffer[..end].to_vec()).ok()
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[allow(unsafe_code, clippy::undocumented_unsafe_blocks)]
fn unix_terminate(pid: u32) -> bool {
    libc::pid_t::try_from(pid)
        .ok()
        .filter(|pid| *pid > 0)
        .is_some_and(|pid| unsafe { libc::kill(pid, libc::SIGTERM) } == 0)
}

#[cfg(target_os = "macos")]
mod platform {
    use std::{
        cell::RefCell,
        collections::VecDeque,
        ffi::{CStr, OsStr, OsString},
        mem::MaybeUninit,
        os::unix::ffi::{OsStrExt as _, OsStringExt as _},
        path::{Path, PathBuf},
        sync::OnceLock,
        time::Duration,
    };

    use parking_lot::Mutex;

    use super::{ProcessRecord, ProcessSample};

    const PROC_PIDUNIQIDENTIFIERINFO: libc::c_int = 17;
    const RECENT_NAME_CAPACITY: usize = 4;
    const ARGUMENTS_BUFFER_BYTES: usize = 16 * 1024;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct UniqueIdentifierInfo {
        executable_uuid: [u8; 16],
        unique_id: u64,
        parent_unique_id: u64,
        id_version: i32,
        reserved: [u32; 5],
    }

    const _: () = assert!(std::mem::size_of::<UniqueIdentifierInfo>() == 56);

    #[derive(Clone, Copy, PartialEq, Eq)]
    struct ImageKey {
        pid: u32,
        unique_id: u64,
        id_version: i32,
    }

    thread_local! {
        static RECENT_NAMES: RefCell<VecDeque<(ImageKey, OsString)>> =
            const { RefCell::new(VecDeque::new()) };
    }

    fn image_key(pid: u32, raw: libc::pid_t) -> Option<ImageKey> {
        let info: UniqueIdentifierInfo = pid_info(raw, PROC_PIDUNIQIDENTIFIERINFO)?;
        Some(ImageKey {
            pid,
            unique_id: info.unique_id,
            id_version: info.id_version,
        })
    }

    fn raw_pid(pid: u32) -> Option<libc::pid_t> {
        libc::pid_t::try_from(pid).ok().filter(|pid| *pid > 0)
    }

    #[allow(unsafe_code, clippy::undocumented_unsafe_blocks)]
    fn pid_info<T: Copy>(pid: libc::pid_t, flavor: libc::c_int) -> Option<T> {
        let size = libc::c_int::try_from(std::mem::size_of::<T>()).ok()?;
        let mut info = MaybeUninit::<T>::zeroed();
        let written = unsafe { libc::proc_pidinfo(pid, flavor, 0, info.as_mut_ptr().cast(), size) };
        (written == size).then(|| unsafe { info.assume_init() })
    }

    fn bsd_info(pid: libc::pid_t) -> Option<libc::proc_bsdinfo> {
        pid_info(pid, libc::PROC_PIDTBSDINFO)
    }

    #[allow(unsafe_code, clippy::undocumented_unsafe_blocks)]
    fn arguments_size(mib: &mut [libc::c_int; 3]) -> Option<usize> {
        let mut size: libc::size_t = 0;
        let queried = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                std::ptr::null_mut(),
                &raw mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        (queried == 0).then_some(size)
    }

    #[allow(unsafe_code, clippy::undocumented_unsafe_blocks)]
    fn with_process_arguments<T>(
        pid: libc::pid_t,
        read: impl FnOnce(&[u8]) -> Option<T>,
    ) -> Option<T> {
        static BUFFER: Mutex<Vec<u8>> = Mutex::new(Vec::new());
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
        let mut buffer = BUFFER.lock();
        buffer.clear();
        if buffer.capacity() < ARGUMENTS_BUFFER_BYTES {
            buffer.reserve(ARGUMENTS_BUFFER_BYTES);
        }
        loop {
            let capacity = buffer.capacity();
            let mut size: libc::size_t = capacity;
            let status = unsafe {
                libc::sysctl(
                    mib.as_mut_ptr(),
                    3,
                    buffer.as_mut_ptr().cast(),
                    &raw mut size,
                    std::ptr::null_mut(),
                    0,
                )
            };
            if status == 0 && size < capacity {
                unsafe { buffer.set_len(size) };
                break;
            }
            let needed = arguments_size(&mut mib)?;
            if needed <= capacity {
                if status != 0 {
                    return None;
                }
                unsafe { buffer.set_len(size.min(capacity)) };
                break;
            }
            buffer.reserve(needed);
        }
        if buffer.len() <= std::mem::size_of::<libc::c_int>() {
            return None;
        }
        read(&buffer)
    }

    fn split_arguments(data: &[u8]) -> Option<(PathBuf, Vec<OsString>)> {
        let (count, rest) = data.split_at_checked(std::mem::size_of::<libc::c_int>())?;
        let mut count = libc::c_int::from_ne_bytes(count.try_into().ok()?);
        let end = rest
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(rest.len());
        let executable = PathBuf::from(OsStr::from_bytes(&rest[..end]));
        let mut rest = &rest[end..];
        let mut arguments = Vec::new();
        while count > 0 {
            while rest.first() == Some(&0) {
                rest = &rest[1..];
            }
            if rest.is_empty() {
                break;
            }
            let end = rest
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(rest.len());
            arguments.push(OsStr::from_bytes(&rest[..end]).to_os_string());
            rest = &rest[end..];
            count -= 1;
        }
        Some((executable, arguments))
    }

    fn exec_path(pid: libc::pid_t) -> Option<PathBuf> {
        with_process_arguments(pid, |data| {
            let path = &data[std::mem::size_of::<libc::c_int>()..];
            let path = &path[..path
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(path.len())];
            (!path.is_empty()).then(|| PathBuf::from(OsStr::from_bytes(path)))
        })
    }

    #[allow(unsafe_code, clippy::undocumented_unsafe_blocks)]
    fn image_path(pid: libc::pid_t) -> Option<PathBuf> {
        let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        let length = unsafe {
            libc::proc_pidpath(
                pid,
                buffer.as_mut_ptr().cast(),
                libc::PROC_PIDPATHINFO_MAXSIZE as u32,
            )
        };
        let length = usize::try_from(length).ok().filter(|length| *length > 0)?;
        buffer.truncate(length);
        Some(PathBuf::from(OsString::from_vec(buffer)))
    }

    fn basename(path: &Path) -> Option<OsString> {
        path.file_name().map(OsStr::to_os_string)
    }

    pub(super) fn start_time(pid: u32) -> Option<u64> {
        bsd_info(raw_pid(pid)?).map(|info| info.pbi_start_tvsec)
    }

    pub(super) fn record(pid: u32) -> Option<ProcessRecord> {
        let raw = raw_pid(pid)?;
        let info = bsd_info(raw)?;
        let (executable, arguments) = match with_process_arguments(raw, split_arguments) {
            Some((executable, arguments)) => (Some(executable), arguments),
            None => (image_path(raw), Vec::new()),
        };
        Some(ProcessRecord {
            pid,
            start_time: info.pbi_start_tvsec,
            executable,
            arguments,
            owner: Some(info.pbi_uid),
        })
    }

    pub(super) fn command_name(pid: u32) -> Option<OsString> {
        let raw = raw_pid(pid)?;
        let key = image_key(pid, raw);
        if let Some(name) = key.and_then(|key| {
            RECENT_NAMES.with_borrow(|recent| {
                recent
                    .iter()
                    .find(|(seen, _)| *seen == key)
                    .map(|(_, name)| name.clone())
            })
        }) {
            return Some(name);
        }
        let name = exec_path(raw)
            .or_else(|| image_path(raw))
            .as_deref()
            .and_then(basename)?;
        if let Some(key) = key {
            RECENT_NAMES.with_borrow_mut(|recent| {
                if recent.len() == RECENT_NAME_CAPACITY {
                    recent.pop_back();
                }
                recent.push_front((key, name.clone()));
            });
        }
        Some(name)
    }

    pub(super) fn working_directory(pid: u32) -> Option<PathBuf> {
        let info: libc::proc_vnodepathinfo = pid_info(raw_pid(pid)?, libc::PROC_PIDVNODEPATHINFO)?;
        let cwd = info.pvi_cdir;
        if cwd.vip_vi.vi_stat.vst_dev == 0 {
            return None;
        }
        let path: Vec<u8> = cwd
            .vip_path
            .iter()
            .flatten()
            .map(|byte| *byte as u8)
            .collect();
        let path = CStr::from_bytes_until_nul(&path).ok()?;
        Some(PathBuf::from(OsStr::from_bytes(path.to_bytes())))
    }

    pub(super) fn terminate(pid: u32) -> bool {
        super::unix_terminate(pid)
    }

    #[repr(C)]
    struct Timebase {
        numer: u32,
        denom: u32,
    }

    #[allow(unsafe_code)]
    unsafe extern "C" {
        fn mach_timebase_info(info: *mut Timebase) -> libc::c_int;
    }

    #[allow(unsafe_code, clippy::undocumented_unsafe_blocks)]
    fn nanoseconds_per_tick() -> (u64, u64) {
        static TIMEBASE: OnceLock<(u64, u64)> = OnceLock::new();
        *TIMEBASE.get_or_init(|| {
            let mut info = Timebase { numer: 0, denom: 0 };
            if unsafe { mach_timebase_info(&raw mut info) } != 0 || info.denom == 0 {
                return (1, 1);
            }
            (u64::from(info.numer), u64::from(info.denom))
        })
    }

    pub(super) fn sample(pid: u32) -> Option<ProcessSample> {
        let raw = raw_pid(pid)?;
        let info = bsd_info(raw)?;
        let task: libc::proc_taskinfo = pid_info(raw, libc::PROC_PIDTASKINFO)?;
        let (numer, denom) = nanoseconds_per_tick();
        let ticks = task.pti_total_user.saturating_add(task.pti_total_system);
        let name = command_name(pid).unwrap_or_else(|| {
            let comm: Vec<u8> = info.pbi_comm.iter().map(|byte| *byte as u8).collect();
            OsString::from_vec(comm.split(|byte| *byte == 0).next().unwrap_or(&[]).to_vec())
        });
        Some(ProcessSample {
            pid,
            parent: Some(info.pbi_ppid).filter(|parent| *parent != 0),
            name,
            resident_bytes: task.pti_resident_size,
            virtual_bytes: task.pti_virtual_size,
            cpu_time: Duration::from_nanos(
                u64::try_from(u128::from(ticks) * u128::from(numer) / u128::from(denom))
                    .unwrap_or(u64::MAX),
            ),
            threads: u32::try_from(task.pti_threadnum).unwrap_or(0),
            start_time: info.pbi_start_tvsec,
        })
    }

    #[allow(unsafe_code, clippy::undocumented_unsafe_blocks)]
    fn children(pid: libc::pid_t) -> Vec<libc::pid_t> {
        let mut buffer = vec![0 as libc::pid_t; 64];
        loop {
            let capacity = libc::c_int::try_from(buffer.len() * std::mem::size_of::<libc::pid_t>())
                .unwrap_or(libc::c_int::MAX);
            let count =
                unsafe { libc::proc_listchildpids(pid, buffer.as_mut_ptr().cast(), capacity) };
            let Ok(count) = usize::try_from(count) else {
                return Vec::new();
            };
            if count < buffer.len() {
                buffer.truncate(count);
                return buffer;
            }
            buffer.resize(buffer.len() * 2, 0);
        }
    }

    pub(super) fn descendants(pid: u32) -> Vec<u32> {
        let Some(root) = raw_pid(pid) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        let mut queue = VecDeque::from([root]);
        while let Some(parent) = queue.pop_front() {
            for child in children(parent) {
                let Ok(child_pid) = u32::try_from(child) else {
                    continue;
                };
                if child <= 0 || child_pid == pid || found.contains(&child_pid) {
                    continue;
                }
                found.push(child_pid);
                queue.push_back(child);
            }
        }
        found.sort_unstable();
        found
    }

    pub(super) fn host_name() -> Option<String> {
        super::unix_host_name()
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::{
        collections::BTreeMap,
        ffi::{OsStr, OsString},
        fs,
        os::unix::ffi::OsStrExt as _,
        path::PathBuf,
        sync::OnceLock,
        time::Duration,
    };

    use super::{ProcessRecord, ProcessSample};

    const PARENT: usize = 1;
    const USER_TICKS: usize = 11;
    const SYSTEM_TICKS: usize = 12;
    const THREADS: usize = 17;
    const START_TICKS: usize = 19;
    const VIRTUAL_BYTES: usize = 20;
    const RESIDENT_PAGES: usize = 21;

    struct Stat {
        comm: OsString,
        fields: Vec<String>,
    }

    impl Stat {
        fn read(pid: u32) -> Option<Self> {
            Self::parse(&fs::read(format!("/proc/{pid}/stat")).ok()?)
        }

        fn parse(data: &[u8]) -> Option<Self> {
            let space = data.iter().position(|byte| *byte == b' ')?;
            let rest = &data[space + 1..];
            let close = rest.iter().rposition(|byte| *byte == b')')?;
            let comm = &rest[..close];
            let comm = comm.strip_prefix(b"(").unwrap_or(comm);
            let fields = std::str::from_utf8(&rest[close + 1..])
                .ok()?
                .split_whitespace()
                .map(str::to_owned)
                .collect();
            Some(Self {
                comm: OsStr::from_bytes(comm).to_os_string(),
                fields,
            })
        }

        fn number(&self, index: usize) -> Option<u64> {
            self.fields.get(index)?.parse().ok()
        }

        fn start_time(&self) -> Option<u64> {
            Some(self.number(START_TICKS)? / clock_ticks() + boot_time())
        }
    }

    #[allow(unsafe_code, clippy::undocumented_unsafe_blocks)]
    fn clock_ticks() -> u64 {
        static TICKS: OnceLock<u64> = OnceLock::new();
        *TICKS.get_or_init(|| {
            u64::try_from(unsafe { libc::sysconf(libc::_SC_CLK_TCK) })
                .ok()
                .filter(|ticks| *ticks > 0)
                .unwrap_or(100)
        })
    }

    #[allow(unsafe_code, clippy::undocumented_unsafe_blocks)]
    fn page_size() -> u64 {
        static PAGE: OnceLock<u64> = OnceLock::new();
        *PAGE.get_or_init(|| {
            u64::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) })
                .ok()
                .filter(|page| *page > 0)
                .unwrap_or(4096)
        })
    }

    #[allow(unsafe_code, clippy::undocumented_unsafe_blocks)]
    fn boot_time() -> u64 {
        if let Some(seconds) = fs::read_to_string("/proc/stat").ok().and_then(|stat| {
            stat.lines()
                .find_map(|line| line.strip_prefix("btime"))
                .and_then(|value| value.trim().parse().ok())
        }) {
            return seconds;
        }
        let mut uptime = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &raw mut uptime) } == 0 {
            u64::try_from(uptime.tv_sec).unwrap_or(0)
        } else {
            0
        }
    }

    fn effective_user(pid: u32) -> Option<u32> {
        fs::read_to_string(format!("/proc/{pid}/status"))
            .ok()?
            .lines()
            .find_map(|line| line.strip_prefix("Uid:"))?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    }

    fn executable(pid: u32) -> Option<PathBuf> {
        let mut path = fs::read_link(format!("/proc/{pid}/exe")).ok()?;
        let deleted = b" (deleted)";
        if let Some(name) = path
            .file_name()
            .map(OsStr::as_bytes)
            .and_then(|name| name.strip_suffix(deleted))
        {
            let name = OsStr::from_bytes(name).to_os_string();
            path.set_file_name(name);
        }
        Some(path)
    }

    fn arguments(pid: u32) -> Vec<OsString> {
        fs::read(format!("/proc/{pid}/cmdline"))
            .unwrap_or_default()
            .split(|byte| *byte == 0)
            .map(<[u8]>::trim_ascii)
            .filter(|argument| !argument.is_empty())
            .map(|argument| OsStr::from_bytes(argument).to_os_string())
            .collect()
    }

    pub(super) fn start_time(pid: u32) -> Option<u64> {
        Stat::read(pid)?.start_time()
    }

    pub(super) fn record(pid: u32) -> Option<ProcessRecord> {
        let stat = Stat::read(pid)?;
        Some(ProcessRecord {
            pid,
            start_time: stat.start_time()?,
            executable: executable(pid),
            arguments: arguments(pid),
            owner: effective_user(pid),
        })
    }

    pub(super) fn command_name(pid: u32) -> Option<OsString> {
        Stat::read(pid).map(|stat| stat.comm)
    }

    pub(super) fn working_directory(pid: u32) -> Option<PathBuf> {
        fs::read_link(format!("/proc/{pid}/cwd")).ok()
    }

    pub(super) fn terminate(pid: u32) -> bool {
        super::unix_terminate(pid)
    }

    pub(super) fn sample(pid: u32) -> Option<ProcessSample> {
        let stat = Stat::read(pid)?;
        let ticks = stat
            .number(USER_TICKS)?
            .saturating_add(stat.number(SYSTEM_TICKS)?);
        Some(ProcessSample {
            pid,
            parent: stat
                .number(PARENT)
                .and_then(|parent| u32::try_from(parent).ok())
                .filter(|parent| *parent != 0),
            resident_bytes: stat
                .number(RESIDENT_PAGES)
                .unwrap_or(0)
                .saturating_mul(page_size()),
            virtual_bytes: stat.number(VIRTUAL_BYTES).unwrap_or(0),
            cpu_time: Duration::from_nanos(ticks.saturating_mul(1_000_000_000) / clock_ticks()),
            threads: stat
                .number(THREADS)
                .and_then(|threads| u32::try_from(threads).ok())
                .unwrap_or(0),
            start_time: stat.start_time()?,
            name: stat.comm,
        })
    }

    pub(super) fn descendants(pid: u32) -> Vec<u32> {
        let mut children = BTreeMap::<u32, Vec<u32>>::new();
        for entry in fs::read_dir("/proc").into_iter().flatten().flatten() {
            let Some(child) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
            else {
                continue;
            };
            if let Some(parent) = Stat::read(child)
                .and_then(|stat| stat.number(PARENT))
                .and_then(|parent| u32::try_from(parent).ok())
            {
                children.entry(parent).or_default().push(child);
            }
        }
        let mut found = Vec::new();
        let mut pending = vec![pid];
        while let Some(parent) = pending.pop() {
            for child in children.remove(&parent).unwrap_or_default() {
                if child != pid {
                    found.push(child);
                    pending.push(child);
                }
            }
        }
        found.sort_unstable();
        found
    }

    pub(super) fn host_name() -> Option<String> {
        super::unix_host_name()
    }
}

#[cfg(windows)]
mod platform {
    use std::{ffi::OsString, path::PathBuf, time::Duration};

    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, Signal, System, UpdateKind};

    use super::{ProcessRecord, ProcessSample};

    fn refreshed(pids: &[Pid], refresh: ProcessRefreshKind) -> System {
        let mut system = System::new();
        system.refresh_processes_specifics(ProcessesToUpdate::Some(pids), true, refresh);
        system
    }

    pub(super) fn start_time(pid: u32) -> Option<u64> {
        let pid = Pid::from_u32(pid);
        refreshed(&[pid], ProcessRefreshKind::nothing())
            .process(pid)
            .map(sysinfo::Process::start_time)
    }

    pub(super) fn record(pid: u32) -> Option<ProcessRecord> {
        let key = Pid::from_u32(pid);
        let system = refreshed(&[key], ProcessRefreshKind::everything());
        let process = system.process(key)?;
        Some(ProcessRecord {
            pid,
            start_time: process.start_time(),
            executable: process.exe().map(PathBuf::from),
            arguments: process.cmd().to_vec(),
            owner: process.user_id().map(|user| (**user).to_string()),
        })
    }

    pub(super) fn command_name(pid: u32) -> Option<OsString> {
        let key = Pid::from_u32(pid);
        refreshed(&[key], ProcessRefreshKind::nothing())
            .process(key)
            .map(|process| process.name().to_os_string())
    }

    pub(super) fn working_directory(pid: u32) -> Option<PathBuf> {
        let key = Pid::from_u32(pid);
        refreshed(
            &[key],
            ProcessRefreshKind::nothing().with_cwd(UpdateKind::Always),
        )
        .process(key)?
        .cwd()
        .map(PathBuf::from)
    }

    pub(super) fn terminate(pid: u32) -> bool {
        let key = Pid::from_u32(pid);
        refreshed(&[key], ProcessRefreshKind::nothing())
            .process(key)
            .and_then(|process| process.kill_with(Signal::Kill))
            == Some(true)
    }

    pub(super) fn sample(pid: u32) -> Option<ProcessSample> {
        let key = Pid::from_u32(pid);
        let system = refreshed(
            &[key],
            ProcessRefreshKind::nothing().with_memory().with_cpu(),
        );
        let process = system.process(key)?;
        Some(ProcessSample {
            pid,
            parent: process.parent().map(Pid::as_u32),
            name: process.name().to_os_string(),
            resident_bytes: process.memory(),
            virtual_bytes: process.virtual_memory(),
            cpu_time: Duration::from_millis(process.accumulated_cpu_time()),
            threads: process
                .tasks()
                .map_or(0, |tasks| u32::try_from(tasks.len()).unwrap_or(u32::MAX)),
            start_time: process.start_time(),
        })
    }

    pub(super) fn descendants(pid: u32) -> Vec<u32> {
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing(),
        );
        let root = Pid::from_u32(pid);
        let mut found = system
            .processes()
            .keys()
            .copied()
            .filter(|candidate| {
                let mut cursor = *candidate;
                for _ in 0..64 {
                    let Some(parent) = system.process(cursor).and_then(sysinfo::Process::parent)
                    else {
                        return false;
                    };
                    if parent == root {
                        return *candidate != root;
                    }
                    if parent == cursor {
                        return false;
                    }
                    cursor = parent;
                }
                false
            })
            .map(Pid::as_u32)
            .collect::<Vec<_>>();
        found.sort_unstable();
        found
    }

    pub(super) fn host_name() -> Option<String> {
        System::host_name()
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
mod platform {
    use std::{ffi::OsString, path::PathBuf};

    use super::{ProcessRecord, ProcessSample};

    pub(super) fn start_time(_pid: u32) -> Option<u64> {
        None
    }

    pub(super) fn record(_pid: u32) -> Option<ProcessRecord> {
        None
    }

    pub(super) fn command_name(_pid: u32) -> Option<OsString> {
        None
    }

    pub(super) fn working_directory(_pid: u32) -> Option<PathBuf> {
        None
    }

    pub(super) fn terminate(_pid: u32) -> bool {
        false
    }

    pub(super) fn sample(_pid: u32) -> Option<ProcessSample> {
        None
    }

    pub(super) fn descendants(_pid: u32) -> Vec<u32> {
        Vec::new()
    }

    #[cfg(unix)]
    pub(super) fn host_name() -> Option<String> {
        super::unix_host_name()
    }

    #[cfg(not(unix))]
    pub(super) fn host_name() -> Option<String> {
        None
    }
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use std::{
        ffi::OsStr,
        os::unix::fs::symlink,
        path::Path,
        process::{Child, Command, Stdio},
    };

    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

    use super::*;

    struct Sleeper(Child);

    impl Drop for Sleeper {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn sleeper(executable: &Path, directory: &Path) -> Sleeper {
        Sleeper(
            Command::new(executable)
                .arg("30")
                .current_dir(directory)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn sleeper"),
        )
    }

    fn oracle(pid: u32) -> System {
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
            true,
            ProcessRefreshKind::everything(),
        );
        system
    }

    fn sleep_binary() -> &'static Path {
        ["/bin/sleep", "/usr/bin/sleep"]
            .into_iter()
            .map(Path::new)
            .find(|path| path.exists())
            .expect("a sleep binary")
    }

    #[test]
    fn facts_match_sysinfo_for_a_symlinked_child() {
        let directory = tempfile::tempdir().expect("fixture directory");
        let link = directory.path().join("claude");
        symlink(sleep_binary(), &link).expect("symlink");
        let child = sleeper(&link, directory.path());
        let pid = child.0.id();

        let system = oracle(pid);
        let expected = system.process(Pid::from_u32(pid)).expect("oracle process");
        assert_eq!(command_name(pid).as_deref(), Some(expected.name()));
        assert_eq!(command_name(pid).as_deref(), Some(OsStr::new("claude")));
        assert_eq!(start_time(pid), Some(expected.start_time()));
        assert_eq!(working_directory(pid).as_deref(), expected.cwd());
        let actual = record(pid).expect("record");
        assert_eq!(actual.pid, pid);
        assert_eq!(actual.start_time, expected.start_time());
        assert_eq!(actual.executable.as_deref(), expected.exe());
        assert_eq!(actual.arguments, expected.cmd());
        assert_eq!(
            actual.owner,
            expected.effective_user_id().map(|user| **user)
        );
        let sample = sample(pid).expect("sample");
        assert_eq!(sample.parent, Some(std::process::id()));
        assert_eq!(sample.start_time, expected.start_time());
        assert!(descendants(std::process::id()).contains(&pid));
    }

    #[test]
    fn the_current_process_matches_sysinfo() {
        let pid = std::process::id();
        let system = oracle(pid);
        let expected = system.process(Pid::from_u32(pid)).expect("oracle process");
        assert_eq!(start_time(pid), Some(expected.start_time()));
        assert_eq!(command_name(pid).as_deref(), Some(expected.name()));
        assert_eq!(host_name(), System::host_name());
        let sample = sample(pid).expect("sample");
        assert!(sample.resident_bytes > 0);
        assert!(sample.threads >= 1);
        assert!(sample.cpu_time > Duration::ZERO);
    }

    #[test]
    fn the_name_cache_notices_an_exec_of_the_same_image() {
        let directory = tempfile::tempdir().expect("fixture directory");
        let link = directory.path().join("claude");
        symlink("/bin/bash", &link).expect("symlink");
        let mut child = Sleeper(
            Command::new("/bin/bash")
                .args([
                    "-c",
                    "read -r _; exec \"$0\" -c 'while :; do sleep 1; done'",
                ])
                .arg(&link)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn shell"),
        );
        let pid = child.0.id();
        assert_eq!(command_name(pid).as_deref(), Some(OsStr::new("bash")));
        drop(child.0.stdin.take());
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while command_name(pid).as_deref() != Some(OsStr::new("claude")) {
            assert!(
                std::time::Instant::now() < deadline,
                "the exec was never noticed"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn a_large_environment_still_reads_the_exec_path() {
        let directory = tempfile::tempdir().expect("fixture directory");
        let link = directory.path().join("claude");
        symlink(sleep_binary(), &link).expect("symlink");
        let child = Sleeper(
            Command::new(&link)
                .arg("30")
                .env("ZZ_PROCESS_INFO_PADDING", "x".repeat(100_000))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn sleeper"),
        );
        let pid = child.0.id();
        assert_eq!(command_name(pid).as_deref(), Some(OsStr::new("claude")));
        let system = oracle(pid);
        let expected = system.process(Pid::from_u32(pid)).expect("oracle process");
        let actual = record(pid).expect("record");
        assert_eq!(actual.executable.as_deref(), expected.exe());
        assert_eq!(actual.arguments, expected.cmd());
    }

    #[test]
    fn missing_processes_answer_nothing() {
        assert_eq!(start_time(0), None);
        assert_eq!(command_name(0), None);
        assert_eq!(record(u32::MAX), None);
        assert_eq!(working_directory(u32::MAX), None);
        assert!(!terminate(0));
    }
}
