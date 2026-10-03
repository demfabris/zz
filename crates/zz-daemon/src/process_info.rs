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
    pub name: String,
    pub resident_bytes: u64,
    pub virtual_bytes: u64,
    pub cpu_time: Duration,
    pub threads: u32,
    pub start_time: u64,
    pub disk_read_bytes: u64,
    pub disk_written_bytes: u64,
}

pub fn start_time(pid: u32) -> Option<u64> {
    platform::start_time(pid)
}

pub fn record(pid: u32) -> Option<ProcessRecord> {
    platform::record(pid)
}

pub fn command_name(pid: u32) -> Option<String> {
    platform::command_name(pid)
}

pub fn working_directory(pid: u32) -> Option<PathBuf> {
    platform::working_directory(pid)
}

pub fn parent(pid: u32) -> Option<u32> {
    platform::parent(pid)
}

pub fn process_group(pid: u32) -> Option<u32> {
    platform::process_group(pid)
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

    const PROC_PIDT_BSDINFOWITHUNIQID: libc::c_int = 18;
    const NAME_CACHE_CAPACITY: usize = 64;
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

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct BsdInfoWithUniqueId {
        bsd: libc::proc_bsdinfo,
        unique: UniqueIdentifierInfo,
    }

    const _: () = assert!(std::mem::size_of::<UniqueIdentifierInfo>() == 56);
    const _: () = assert!(std::mem::size_of::<BsdInfoWithUniqueId>() == 192);

    #[derive(Clone, Copy, PartialEq, Eq)]
    struct ImageKey {
        pid: u32,
        unique_id: u64,
        id_version: i32,
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    struct LaunchKey {
        parent_unique_id: u64,
        executable_uuid: [u8; 16],
        kernel_name: [libc::c_char; 32],
    }

    struct NamedImage {
        key: ImageKey,
        name: String,
        verified: bool,
    }

    struct NameCache {
        images: VecDeque<NamedImage>,
        launches: VecDeque<(LaunchKey, String)>,
    }

    thread_local! {
        static NAMES: RefCell<NameCache> = const {
            RefCell::new(NameCache {
                images: VecDeque::new(),
                launches: VecDeque::new(),
            })
        };
    }

    #[cfg(test)]
    thread_local! {
        static ARGUMENT_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    impl NameCache {
        fn name(&mut self, key: ImageKey, launch: LaunchKey, raw: libc::pid_t) -> Option<String> {
            match self.images.iter().position(|image| image.key == key) {
                Some(index) if self.images[index].verified => {
                    return Some(self.images[index].name.clone());
                }
                Some(index) => {
                    self.images.remove(index);
                }
                None => {
                    let exec_in_place = self.images.iter().any(|image| {
                        image.key.pid == key.pid && image.key.unique_id == key.unique_id
                    });
                    if !exec_in_place
                        && let Some((_, name)) =
                            self.launches.iter().find(|(seen, _)| *seen == launch)
                    {
                        let name = name.clone();
                        self.remember(key, name.clone(), false);
                        return Some(name);
                    }
                }
            }
            let Some(name) = invoked_name(raw) else {
                return image_path(raw).as_deref().and_then(basename);
            };
            self.remember(key, name.clone(), true);
            self.launches.retain(|(seen, _)| *seen != launch);
            if self.launches.len() == NAME_CACHE_CAPACITY {
                self.launches.pop_back();
            }
            self.launches.push_front((launch, name.clone()));
            Some(name)
        }

        fn remember(&mut self, key: ImageKey, name: String, verified: bool) {
            if self.images.len() == NAME_CACHE_CAPACITY {
                self.images.pop_back();
            }
            self.images.push_front(NamedImage {
                key,
                name,
                verified,
            });
        }
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
    fn fill_arguments(pid: libc::pid_t, buffer: &mut Vec<u8>) -> Option<()> {
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
        buffer.clear();
        buffer.reserve(ARGUMENTS_BUFFER_BYTES);
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
            buffer.reserve_exact(needed + 1);
        }
        (buffer.len() > std::mem::size_of::<libc::c_int>()).then_some(())
    }

    fn with_process_arguments<T>(
        pid: libc::pid_t,
        read: impl FnOnce(&[u8]) -> Option<T>,
    ) -> Option<T> {
        static BUFFER: Mutex<Vec<u8>> = Mutex::new(Vec::new());
        let mut shared = BUFFER.try_lock();
        let mut private = Vec::new();
        let buffer = shared.as_deref_mut().unwrap_or(&mut private);
        let result = fill_arguments(pid, buffer).and_then(|()| read(buffer));
        if buffer.capacity() > ARGUMENTS_BUFFER_BYTES {
            buffer.clear();
            buffer.shrink_to(ARGUMENTS_BUFFER_BYTES);
        }
        result
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

    fn basename(path: &Path) -> Option<String> {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
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

    fn invoked_name(raw: libc::pid_t) -> Option<String> {
        #[cfg(test)]
        ARGUMENT_READS.with(|reads| reads.set(reads.get() + 1));
        exec_path(raw).as_deref().and_then(basename)
    }

    pub(super) fn command_name(pid: u32) -> Option<String> {
        let raw = raw_pid(pid)?;
        let Some(info) = pid_info::<BsdInfoWithUniqueId>(raw, PROC_PIDT_BSDINFOWITHUNIQID) else {
            return invoked_name(raw).or_else(|| image_path(raw).as_deref().and_then(basename));
        };
        let key = ImageKey {
            pid,
            unique_id: info.unique.unique_id,
            id_version: info.unique.id_version,
        };
        let launch = LaunchKey {
            parent_unique_id: info.unique.parent_unique_id,
            executable_uuid: info.unique.executable_uuid,
            kernel_name: info.bsd.pbi_name,
        };
        NAMES.with_borrow_mut(|cache| cache.name(key, launch, raw))
    }

    #[allow(unsafe_code, clippy::undocumented_unsafe_blocks)]
    pub(super) fn working_directory(pid: u32) -> Option<PathBuf> {
        let info: libc::proc_vnodepathinfo = pid_info(raw_pid(pid)?, libc::PROC_PIDVNODEPATHINFO)?;
        let cwd = info.pvi_cdir;
        if cwd.vip_vi.vi_stat.vst_dev == 0 {
            return None;
        }
        let path = cwd.vip_path.as_flattened();
        let path = unsafe { std::slice::from_raw_parts(path.as_ptr().cast::<u8>(), path.len()) };
        let path = CStr::from_bytes_until_nul(path).ok()?;
        Some(PathBuf::from(OsStr::from_bytes(path.to_bytes())))
    }

    pub(super) fn parent(pid: u32) -> Option<u32> {
        bsd_info(raw_pid(pid)?)
            .map(|info| info.pbi_ppid)
            .filter(|parent| *parent != 0)
    }

    pub(super) fn process_group(pid: u32) -> Option<u32> {
        bsd_info(raw_pid(pid)?).map(|info| info.pbi_pgid)
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

    #[allow(unsafe_code, clippy::undocumented_unsafe_blocks)]
    fn disk_bytes(pid: libc::pid_t) -> (u64, u64) {
        let mut usage = MaybeUninit::<libc::rusage_info_v2>::zeroed();
        if unsafe { libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V2, usage.as_mut_ptr().cast()) }
            != 0
        {
            return (0, 0);
        }
        let usage = unsafe { usage.assume_init() };
        (usage.ri_diskio_bytesread, usage.ri_diskio_byteswritten)
    }

    #[allow(unsafe_code, clippy::undocumented_unsafe_blocks)]
    pub(super) fn sample(pid: u32) -> Option<ProcessSample> {
        let raw = raw_pid(pid)?;
        let info = bsd_info(raw)?;
        let task: libc::proc_taskinfo = pid_info(raw, libc::PROC_PIDTASKINFO)?;
        let (numer, denom) = nanoseconds_per_tick();
        let ticks = task.pti_total_user.saturating_add(task.pti_total_system);
        let name = command_name(pid).unwrap_or_else(|| {
            let comm = unsafe {
                std::slice::from_raw_parts(info.pbi_comm.as_ptr().cast::<u8>(), info.pbi_comm.len())
            };
            let comm = comm.split(|byte| *byte == 0).next().unwrap_or(&[]);
            String::from_utf8_lossy(comm).into_owned()
        });
        let (disk_read_bytes, disk_written_bytes) = disk_bytes(raw);
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
            disk_read_bytes,
            disk_written_bytes,
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

    #[cfg(test)]
    mod name_cache_tests {
        use std::{
            os::unix::fs::symlink,
            path::Path,
            process::{Child, Command, Stdio},
            time::Instant,
        };

        use super::*;

        struct Running(Child);

        impl Drop for Running {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }

        fn run(executable: &Path) -> Running {
            Running(
                Command::new(executable)
                    .arg("30")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .expect("spawn"),
            )
        }

        fn reads() -> usize {
            ARGUMENT_READS.with(std::cell::Cell::get)
        }

        fn facts(pid: u32) -> (ImageKey, LaunchKey) {
            let info: BsdInfoWithUniqueId =
                pid_info(raw_pid(pid).expect("pid"), PROC_PIDT_BSDINFOWITHUNIQID).expect("info");
            (
                ImageKey {
                    pid,
                    unique_id: info.unique.unique_id,
                    id_version: info.unique.id_version,
                },
                LaunchKey {
                    parent_unique_id: info.unique.parent_unique_id,
                    executable_uuid: info.unique.executable_uuid,
                    kernel_name: info.bsd.pbi_name,
                },
            )
        }

        #[test]
        fn a_second_child_through_the_same_link_skips_the_argument_read() {
            let directory = tempfile::tempdir().expect("fixture directory");
            let link = directory.path().join("claude");
            symlink("/bin/sleep", &link).expect("symlink");
            let first = run(&link);
            assert_eq!(command_name(first.0.id()).as_deref(), Some("claude"));
            let before = reads();
            let second = run(&link);
            assert_eq!(command_name(second.0.id()).as_deref(), Some("claude"));
            assert_eq!(reads(), before);
            assert_eq!(command_name(second.0.id()).as_deref(), Some("claude"));
            assert_eq!(reads(), before + 1);
            assert_eq!(command_name(second.0.id()).as_deref(), Some("claude"));
            assert_eq!(reads(), before + 1);
        }

        #[test]
        fn a_child_run_under_another_name_is_read_again_on_the_next_lookup() {
            let directory = tempfile::tempdir().expect("fixture directory");
            let link = directory.path().join("claude");
            symlink("/bin/sleep", &link).expect("symlink");
            let linked = run(&link);
            assert_eq!(command_name(linked.0.id()).as_deref(), Some("claude"));
            let direct = run(Path::new("/bin/sleep"));
            command_name(direct.0.id());
            assert_eq!(command_name(direct.0.id()).as_deref(), Some("sleep"));
            let again = run(Path::new("/bin/sleep"));
            assert_eq!(command_name(again.0.id()).as_deref(), Some("sleep"));
        }

        #[test]
        fn a_guess_never_outlives_one_lookup() {
            let child = run(Path::new("/bin/sleep"));
            let pid = child.0.id();
            let (_, launch) = facts(pid);
            NAMES.with_borrow_mut(|cache| cache.launches.push_front((launch, "guess".to_owned())));
            assert_eq!(command_name(pid).as_deref(), Some("guess"));
            assert_eq!(command_name(pid).as_deref(), Some("sleep"));
            assert_eq!(command_name(pid).as_deref(), Some("sleep"));
        }

        #[test]
        fn a_reused_pid_never_answers_the_previous_process_name() {
            let child = run(Path::new("/bin/sleep"));
            let pid = child.0.id();
            let (key, _) = facts(pid);
            NAMES.with_borrow_mut(|cache| {
                cache.remember(
                    ImageKey {
                        unique_id: key.unique_id.wrapping_add(1),
                        ..key
                    },
                    "stale".to_owned(),
                    true,
                );
            });
            assert_eq!(command_name(pid).as_deref(), Some("sleep"));
        }

        #[test]
        fn an_exec_in_place_reads_the_new_name_at_once() {
            let directory = tempfile::tempdir().expect("fixture directory");
            let link = directory.path().join("claude");
            symlink("/bin/bash", &link).expect("symlink");
            let mut child = Running(
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
            assert_eq!(command_name(pid).as_deref(), Some("bash"));
            let (before, _) = facts(pid);
            drop(child.0.stdin.take());
            let deadline = Instant::now() + Duration::from_secs(10);
            while facts(pid).0 == before {
                assert!(Instant::now() < deadline, "the exec never happened");
                std::thread::sleep(Duration::from_millis(5));
            }
            let before = reads();
            let mut name = command_name(pid);
            assert_eq!(reads(), before + 1);
            while name.as_deref() != Some("claude") {
                assert!(Instant::now() < deadline, "the exec was never noticed");
                std::thread::sleep(Duration::from_millis(5));
                name = command_name(pid);
            }
        }

        #[test]
        #[ignore = "timing probe, run by hand"]
        #[allow(unsafe_code, clippy::undocumented_unsafe_blocks)]
        fn foreground_flip_probe() {
            use std::{
                io::{Read as _, Write as _},
                os::{fd::FromRawFd as _, unix::process::CommandExt as _},
            };
            let mut master = -1;
            let mut slave = -1;
            assert_eq!(
                unsafe {
                    libc::openpty(
                        &raw mut master,
                        &raw mut slave,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    )
                },
                0
            );
            let stdio =
                || Stdio::from(unsafe { std::os::fd::OwnedFd::from_raw_fd(libc::dup(slave)) });
            let mut command = Command::new("/bin/bash");
            command
                .args(["--norc", "--noprofile", "-i"])
                .env("PS1", "")
                .stdin(stdio())
                .stdout(stdio())
                .stderr(stdio());
            unsafe {
                command.pre_exec(|| {
                    libc::setsid();
                    libc::ioctl(0, libc::TIOCSCTTY.into(), 0);
                    Ok(())
                });
            }
            let shell = Running(command.spawn().expect("spawn shell"));
            unsafe { libc::close(slave) };
            let mut writer = unsafe { std::fs::File::from_raw_fd(libc::dup(master)) };
            let mut reader = unsafe { std::fs::File::from_raw_fd(libc::dup(master)) };
            std::thread::spawn(move || {
                let mut buffer = [0u8; 4096];
                while reader.read(&mut buffer).is_ok_and(|read| read > 0) {}
            });
            std::thread::sleep(Duration::from_millis(300));
            writer
                .write_all(b"while :; do sleep 0.02; done\n")
                .expect("type loop");
            std::thread::sleep(Duration::from_millis(300));
            let samples = std::env::var("ZZ_NAMES_PROBE_SAMPLES")
                .ok()
                .and_then(|samples| samples.parse().ok())
                .unwrap_or(300usize);
            let interval = Duration::from_millis(
                std::env::var("ZZ_NAMES_PROBE_INTERVAL_MS")
                    .ok()
                    .and_then(|interval| interval.parse().ok())
                    .unwrap_or(100),
            );
            let mut checks = Vec::new();
            let mut argument_reads = Vec::new();
            let mut names = std::collections::BTreeMap::<String, usize>::new();
            let mut mismatches = 0usize;
            let mut compared = 0usize;
            let reads_before = reads();
            while checks.len() < samples {
                std::thread::sleep(interval);
                let group = unsafe { libc::tcgetpgrp(master) };
                let Ok(pid) = u32::try_from(group) else {
                    continue;
                };
                if pid == 0 {
                    continue;
                }
                let started = Instant::now();
                let name = command_name(pid);
                checks.push(started.elapsed().as_secs_f64() * 1e6);
                let started = Instant::now();
                let exact = exec_path(group).as_deref().and_then(basename);
                argument_reads.push(started.elapsed().as_secs_f64() * 1e6);
                if let (Some(name), Some(exact)) = (&name, &exact) {
                    compared += 1;
                    if name != exact {
                        mismatches += 1;
                    }
                }
                *names.entry(name.unwrap_or_default()).or_default() += 1;
            }
            let lookup_reads = reads() - reads_before;
            drop(shell);
            unsafe { libc::close(master) };
            let median = |values: &mut Vec<f64>| {
                values.sort_by(f64::total_cmp);
                values[values.len() / 2]
            };
            let p90 = |values: &Vec<f64>| values[values.len() * 9 / 10];
            let check_median = median(&mut checks);
            let read_median = median(&mut argument_reads);
            let names = names
                .iter()
                .map(|(name, count)| format!("\"{name}\": {count}"))
                .collect::<Vec<_>>()
                .join(", ");
            let report = format!(
                "{{\"samples\": {samples}, \"check_us_median\": {check_median:.2}, \"check_us_p90\": {:.2}, \"argument_read_us_median\": {read_median:.2}, \"argument_read_us_p90\": {:.2}, \"lookup_argument_reads\": {lookup_reads}, \"compared\": {compared}, \"mismatches\": {mismatches}, \"names\": {{{names}}}}}\n",
                p90(&checks),
                p90(&argument_reads),
            );
            print!("{report}");
            if let Ok(path) = std::env::var("ZZ_NAMES_PROBE_OUT") {
                std::fs::write(path, &report).expect("write probe report");
            }
            assert_eq!(mismatches, 0);
        }
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
    const PROCESS_GROUP: usize = 2;
    const USER_TICKS: usize = 11;
    const SYSTEM_TICKS: usize = 12;
    const THREADS: usize = 17;
    const START_TICKS: usize = 19;
    const VIRTUAL_BYTES: usize = 20;
    const RESIDENT_PAGES: usize = 21;

    struct Stat {
        data: Vec<u8>,
        comm: std::ops::Range<usize>,
    }

    impl Stat {
        fn read(pid: u32) -> Option<Self> {
            Self::parse(fs::read(format!("/proc/{pid}/stat")).ok()?)
        }

        fn parse(data: Vec<u8>) -> Option<Self> {
            let open = data.iter().position(|byte| *byte == b'(')?;
            let close = data.iter().rposition(|byte| *byte == b')')?;
            (open < close).then_some(Self {
                comm: open + 1..close,
                data,
            })
        }

        fn comm(&self) -> String {
            String::from_utf8_lossy(&self.data[self.comm.clone()]).into_owned()
        }

        fn number(&self, index: usize) -> Option<u64> {
            let field = self.data[self.comm.end + 1..]
                .split(u8::is_ascii_whitespace)
                .filter(|field| !field.is_empty())
                .nth(index)?;
            std::str::from_utf8(field).ok()?.parse().ok()
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
        static BOOT: OnceLock<u64> = OnceLock::new();
        *BOOT.get_or_init(|| {
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
        })
    }

    fn disk_bytes(pid: u32) -> (u64, u64) {
        let Ok(io) = fs::read_to_string(format!("/proc/{pid}/io")) else {
            return (0, 0);
        };
        let field = |name: &str| {
            io.lines()
                .find_map(|line| line.strip_prefix(name))
                .and_then(|value| value.trim().parse().ok())
                .unwrap_or(0)
        };
        (field("read_bytes:"), field("write_bytes:"))
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

    pub(super) fn command_name(pid: u32) -> Option<String> {
        let mut name = fs::read(format!("/proc/{pid}/comm")).ok()?;
        if name.last() == Some(&b'\n') {
            name.pop();
        }
        Some(
            String::from_utf8(name)
                .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned()),
        )
    }

    pub(super) fn working_directory(pid: u32) -> Option<PathBuf> {
        fs::read_link(format!("/proc/{pid}/cwd")).ok()
    }

    pub(super) fn parent(pid: u32) -> Option<u32> {
        u32::try_from(Stat::read(pid)?.number(PARENT)?)
            .ok()
            .filter(|parent| *parent != 0)
    }

    pub(super) fn process_group(pid: u32) -> Option<u32> {
        u32::try_from(Stat::read(pid)?.number(PROCESS_GROUP)?).ok()
    }

    pub(super) fn terminate(pid: u32) -> bool {
        super::unix_terminate(pid)
    }

    pub(super) fn sample(pid: u32) -> Option<ProcessSample> {
        let stat = Stat::read(pid)?;
        let ticks = stat
            .number(USER_TICKS)?
            .saturating_add(stat.number(SYSTEM_TICKS)?);
        let (disk_read_bytes, disk_written_bytes) = disk_bytes(pid);
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
            name: stat.comm(),
            disk_read_bytes,
            disk_written_bytes,
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
    use std::{path::PathBuf, time::Duration};

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

    pub(super) fn command_name(pid: u32) -> Option<String> {
        let key = Pid::from_u32(pid);
        refreshed(&[key], ProcessRefreshKind::nothing())
            .process(key)
            .map(|process| process.name().to_string_lossy().into_owned())
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

    pub(super) fn parent(pid: u32) -> Option<u32> {
        let key = Pid::from_u32(pid);
        refreshed(&[key], ProcessRefreshKind::nothing())
            .process(key)?
            .parent()
            .map(Pid::as_u32)
    }

    pub(super) fn process_group(_pid: u32) -> Option<u32> {
        None
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
            ProcessRefreshKind::nothing()
                .with_memory()
                .with_cpu()
                .with_disk_usage(),
        );
        let process = system.process(key)?;
        let disk = process.disk_usage();
        Some(ProcessSample {
            pid,
            parent: process.parent().map(Pid::as_u32),
            name: process.name().to_string_lossy().into_owned(),
            resident_bytes: process.memory(),
            virtual_bytes: process.virtual_memory(),
            cpu_time: Duration::from_millis(process.accumulated_cpu_time()),
            threads: process
                .tasks()
                .map_or(0, |tasks| u32::try_from(tasks.len()).unwrap_or(u32::MAX)),
            start_time: process.start_time(),
            disk_read_bytes: disk.total_read_bytes,
            disk_written_bytes: disk.total_written_bytes,
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
    use std::path::PathBuf;

    use super::{ProcessRecord, ProcessSample};

    pub(super) fn start_time(_pid: u32) -> Option<u64> {
        None
    }

    pub(super) fn record(_pid: u32) -> Option<ProcessRecord> {
        None
    }

    pub(super) fn command_name(_pid: u32) -> Option<String> {
        None
    }

    pub(super) fn working_directory(_pid: u32) -> Option<PathBuf> {
        None
    }

    pub(super) fn parent(_pid: u32) -> Option<u32> {
        None
    }

    #[cfg(unix)]
    #[allow(unsafe_code, clippy::undocumented_unsafe_blocks)]
    pub(super) fn process_group(pid: u32) -> Option<u32> {
        let process_id = libc::pid_t::try_from(pid).ok().filter(|pid| *pid > 0)?;
        let group = unsafe { libc::getpgid(process_id) };
        u32::try_from(group).ok().filter(|group| *group > 0)
    }

    #[cfg(not(unix))]
    pub(super) fn process_group(_pid: u32) -> Option<u32> {
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
        assert_eq!(command_name(pid).as_deref(), expected.name().to_str());
        assert_eq!(command_name(pid).as_deref(), Some("claude"));
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
        assert_eq!(parent(pid), Some(std::process::id()));
        assert_eq!(process_group(pid), process_group(std::process::id()));
        assert!(descendants(std::process::id()).contains(&pid));
    }

    #[test]
    fn the_current_process_matches_sysinfo() {
        let pid = std::process::id();
        let system = oracle(pid);
        let expected = system.process(Pid::from_u32(pid)).expect("oracle process");
        assert_eq!(start_time(pid), Some(expected.start_time()));
        assert_eq!(command_name(pid).as_deref(), expected.name().to_str());
        assert_eq!(host_name(), System::host_name());
        assert_eq!(parent(pid), expected.parent().map(Pid::as_u32));
        assert_eq!(
            process_group(pid),
            u32::try_from(rustix::process::getpgrp().as_raw_nonzero().get()).ok()
        );
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
        assert_eq!(command_name(pid).as_deref(), Some("bash"));
        drop(child.0.stdin.take());
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while command_name(pid).as_deref() != Some("claude") {
            assert!(
                std::time::Instant::now() < deadline,
                "the exec was never noticed"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn a_lookup_during_an_exec_does_not_pin_the_resolved_image_name() {
        let directory = tempfile::tempdir().expect("fixture directory");
        let versions = directory.path().join("versions");
        std::fs::create_dir_all(&versions).expect("versions directory");
        symlink("/bin/bash", versions.join("2.1.99")).expect("versioned binary");
        let link = directory.path().join("claude");
        symlink(Path::new("versions").join("2.1.99"), &link).expect("agent symlink");
        for _ in 0..200 {
            let child = Sleeper(
                Command::new("/bin/bash")
                    .args(["-c", "exec \"$0\" -c 'while :; do sleep 1; done'"])
                    .arg(&link)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .expect("spawn shell"),
            );
            let pid = child.0.id();
            std::thread::scope(|scope| {
                for _ in 0..4 {
                    scope.spawn(|| {
                        let deadline = std::time::Instant::now() + Duration::from_secs(5);
                        let mut name = command_name(pid);
                        while name.as_deref() != Some("claude") {
                            assert!(std::time::Instant::now() < deadline, "stuck at {name:?}");
                            name = command_name(pid);
                        }
                    });
                }
            });
        }
    }

    #[test]
    fn a_large_argument_list_still_reads_the_exec_path() {
        let directory = tempfile::tempdir().expect("fixture directory");
        let link = directory.path().join("claude");
        symlink("/bin/bash", &link).expect("symlink");
        let child = Sleeper(
            Command::new(&link)
                .args(["-c", "while :; do sleep 1; done"])
                .arg("x".repeat(100_000))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn sleeper"),
        );
        let pid = child.0.id();
        assert_eq!(command_name(pid).as_deref(), Some("claude"));
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
        assert_eq!(parent(u32::MAX), None);
        assert_eq!(process_group(u32::MAX), None);
        assert!(!terminate(0));
    }
}
