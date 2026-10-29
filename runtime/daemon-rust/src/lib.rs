mod display_readback;
mod limits;
mod peridot;
mod sconfig;
use std::ffi::c_void;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Output as ProcessOutput, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

mod app_controls;
mod app_runtime;
mod bypass_policy;
mod service_control;
mod subsystem_clocks;
mod system_colors;
mod touch_resampler;

pub const SOCKET_NAME: &str = "peridot_essentiald_v15";
pub const REVERSE_SOCKET_NAME: &str = "peridot_essential_app_v15";
pub const PROTOCOL_VERSION: &str = "peridot.4";
const LOOPBACK_PORT: u16 = 732;

const AF_UNIX: i32 = 1;
#[cfg(target_os = "android")]
const AF_NETLINK: i32 = 16;
const SOCK_STREAM: i32 = 1;
#[cfg(target_os = "android")]
const SOCK_DGRAM: i32 = 2;
const SOCK_CLOEXEC: i32 = 0x80000;
#[cfg(target_os = "android")]
const NETLINK_KOBJECT_UEVENT: i32 = 15;
const SOL_SOCKET: i32 = 1;
const SO_PEERCRED: i32 = 17;

#[repr(C)]
struct SockAddrUn {
    sun_family: u16,
    sun_path: [i8; 108],
}

#[repr(C)]
struct UCred {
    pid: i32,
    uid: u32,
    gid: u32,
}

#[repr(C)]
#[cfg(target_os = "android")]
struct SockAddrNl {
    nl_family: u16,
    nl_pad: u16,
    nl_pid: u32,
    nl_groups: u32,
}

unsafe extern "C" {
    fn socket(domain: i32, ty: i32, protocol: i32) -> i32;
    fn bind(fd: i32, addr: *const c_void, len: u32) -> i32;
    fn listen(fd: i32, backlog: i32) -> i32;
    fn accept4(fd: i32, addr: *mut c_void, len: *mut u32, flags: i32) -> i32;
    fn connect(fd: i32, addr: *const c_void, len: u32) -> i32;
    fn getsockopt(fd: i32, level: i32, name: i32, value: *mut c_void, len: *mut u32) -> i32;
    #[cfg(target_os = "android")]
    fn recv(fd: i32, buf: *mut c_void, len: usize, flags: i32) -> isize;
    fn close(fd: i32) -> i32;
}

#[cfg(target_os = "android")]
mod vendor_binder {
    use std::ffi::{c_char, c_void};
    use std::ptr;
    use std::sync::OnceLock;

    const STATUS_OK: i32 = 0;
    const FLAG_PRIVATE_VENDOR: u32 = 0x1000_0000;

    #[repr(C)]
    pub struct AIBinder {
        _private: [u8; 0],
    }
    #[repr(C)]
    pub struct AIBinder_Class {
        _private: [u8; 0],
    }
    #[repr(C)]
    pub struct AParcel {
        _private: [u8; 0],
    }
    #[repr(C)]
    pub struct AStatus {
        _private: [u8; 0],
    }

    type OnCreate = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
    type OnDestroy = unsafe extern "C" fn(*mut c_void);
    type OnTransact = unsafe extern "C" fn(*mut AIBinder, u32, *const AParcel, *mut AParcel) -> i32;

    #[link(name = "binder_ndk")]
    unsafe extern "C" {
        fn AIBinder_Class_define(
            descriptor: *const c_char,
            on_create: OnCreate,
            on_destroy: OnDestroy,
            on_transact: OnTransact,
        ) -> *mut AIBinder_Class;
        fn AIBinder_associateClass(binder: *mut AIBinder, clazz: *const AIBinder_Class) -> bool;
        fn AIBinder_prepareTransaction(binder: *mut AIBinder, input: *mut *mut AParcel) -> i32;
        fn AIBinder_transact(
            binder: *mut AIBinder,
            code: u32,
            input: *mut *mut AParcel,
            output: *mut *mut AParcel,
            flags: u32,
        ) -> i32;
        fn AIBinder_decStrong(binder: *mut AIBinder);
        fn AParcel_writeInt32(parcel: *mut AParcel, value: i32) -> i32;
        fn AParcel_readInt32(parcel: *const AParcel, value: *mut i32) -> i32;
        fn AParcel_readStatusHeader(parcel: *const AParcel, status: *mut *mut AStatus) -> i32;
        fn AParcel_delete(parcel: *mut AParcel);
        fn AStatus_isOk(status: *const AStatus) -> bool;
        fn AStatus_delete(status: *mut AStatus);
    }

    unsafe extern "C" fn on_create(_: *mut c_void) -> *mut c_void {
        ptr::null_mut()
    }

    unsafe extern "C" fn on_destroy(_: *mut c_void) {}

    unsafe extern "C" fn on_transact(
        _: *mut AIBinder,
        _: u32,
        _: *const AParcel,
        _: *mut AParcel,
    ) -> i32 {
        -38
    }

    const RTLD_NOW: i32 = 2;

    type CheckServiceFn = unsafe extern "C" fn(instance: *const c_char) -> *mut AIBinder;

    #[link(name = "dl")]
    unsafe extern "C" {
        fn dlopen(filename: *const c_char, flags: i32) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }

    static CHECK_SERVICE_FN: OnceLock<Option<CheckServiceFn>> = OnceLock::new();
    static TOUCH_CLASS: OnceLock<usize> = OnceLock::new();
    static DISPLAY_CLASS: OnceLock<usize> = OnceLock::new();

    fn service_manager_check_service(instance: *const c_char) -> *mut AIBinder {
        let resolved = CHECK_SERVICE_FN.get_or_init(|| unsafe {
            let handle = dlopen(c"libbinder_ndk.so".as_ptr(), RTLD_NOW);
            if handle.is_null() {
                return None;
            }

            let set_threads_sym = dlsym(
                handle,
                c"ABinderProcess_setThreadPoolMaxThreadCount".as_ptr(),
            );
            if !set_threads_sym.is_null() {
                let set_threads: unsafe extern "C" fn(u32) -> bool =
                    std::mem::transmute(set_threads_sym);
                set_threads(4);
            }
            let start_threads_sym = dlsym(handle, c"ABinderProcess_startThreadPool".as_ptr());
            if !start_threads_sym.is_null() {
                let start_threads: unsafe extern "C" fn() = std::mem::transmute(start_threads_sym);
                start_threads();
            }

            let symbol = dlsym(handle, c"AServiceManager_checkService".as_ptr());
            if symbol.is_null() {
                return None;
            }

            Some(std::mem::transmute::<*mut c_void, CheckServiceFn>(symbol))
        });

        match *resolved {
            Some(function) => unsafe { function(instance) },
            None => ptr::null_mut(),
        }
    }

    fn class_for(
        descriptor: &'static [u8],
        slot: &'static OnceLock<usize>,
    ) -> *const AIBinder_Class {
        let raw = *slot.get_or_init(|| unsafe {
            AIBinder_Class_define(
                descriptor.as_ptr().cast(),
                on_create,
                on_destroy,
                on_transact,
            ) as usize
        });
        raw as *const AIBinder_Class
    }

    fn check_service(
        service: &'static [u8],
        descriptor: &'static [u8],
        slot: &'static OnceLock<usize>,
    ) -> Option<*mut AIBinder> {
        let binder = service_manager_check_service(service.as_ptr().cast());
        if binder.is_null() {
            return None;
        }

        let clazz = class_for(descriptor, slot);
        if clazz.is_null() || !unsafe { AIBinder_associateClass(binder, clazz) } {
            unsafe { AIBinder_decStrong(binder) };
            return None;
        }

        Some(binder)
    }

    pub fn touch_available() -> bool {
        const SERVICE: &[u8] = b"vendor.xiaomi.hw.touchfeature.ITouchFeature/default\0";
        const DESCRIPTOR: &[u8] = b"vendor.xiaomi.hw.touchfeature.ITouchFeature\0";
        let Some(binder) = check_service(SERVICE, DESCRIPTOR, &TOUCH_CLASS) else {
            return false;
        };
        unsafe { AIBinder_decStrong(binder) };
        true
    }

    pub fn display_available() -> bool {
        const SERVICE: &[u8] =
            b"vendor.xiaomi.hardware.displayfeature_aidl.IDisplayFeature/default\0";
        const DESCRIPTOR: &[u8] = b"vendor.xiaomi.hardware.displayfeature_aidl.IDisplayFeature\0";
        let Some(binder) = check_service(SERVICE, DESCRIPTOR, &DISPLAY_CLASS) else {
            return false;
        };
        unsafe { AIBinder_decStrong(binder) };
        true
    }

    pub fn set_touch_mode(display_id: i32, mode: i32, value: i32) -> bool {
        const SERVICE: &[u8] = b"vendor.xiaomi.hw.touchfeature.ITouchFeature/default\0";
        const DESCRIPTOR: &[u8] = b"vendor.xiaomi.hw.touchfeature.ITouchFeature\0";
        let Some(binder) = check_service(SERVICE, DESCRIPTOR, &TOUCH_CLASS) else {
            return false;
        };

        let mut input: *mut AParcel = ptr::null_mut();
        let mut output: *mut AParcel = ptr::null_mut();
        let mut ok = unsafe { AIBinder_prepareTransaction(binder, &mut input) } == STATUS_OK;

        if ok {
            ok &= unsafe { AParcel_writeInt32(input, display_id) } == STATUS_OK;
            ok &= unsafe { AParcel_writeInt32(input, mode) } == STATUS_OK;
            ok &= unsafe { AParcel_writeInt32(input, value) } == STATUS_OK;
        }

        if ok {
            ok &= unsafe {
                AIBinder_transact(binder, 9, &mut input, &mut output, FLAG_PRIVATE_VENDOR)
            } == STATUS_OK;
        } else if !input.is_null() {
            unsafe { AParcel_delete(input) };
        }

        if ok && !output.is_null() {
            let mut status: *mut AStatus = ptr::null_mut();
            ok &= unsafe { AParcel_readStatusHeader(output, &mut status) } == STATUS_OK;
            if !status.is_null() {
                ok &= unsafe { AStatus_isOk(status) };
                unsafe { AStatus_delete(status) };
            } else {
                ok = false;
            }

            let mut result = -1i32;
            if ok {
                ok &= unsafe { AParcel_readInt32(output, &mut result) } == STATUS_OK;
                ok &= result == 0;
            }
        } else {
            ok = false;
        }

        if !output.is_null() {
            unsafe { AParcel_delete(output) };
        }
        unsafe { AIBinder_decStrong(binder) };
        ok
    }

    pub fn get_touch_mode(display_id: i32, mode: i32) -> Option<i32> {
        const SERVICE: &[u8] = b"vendor.xiaomi.hw.touchfeature.ITouchFeature/default\0";
        const DESCRIPTOR: &[u8] = b"vendor.xiaomi.hw.touchfeature.ITouchFeature\0";
        let Some(binder) = check_service(SERVICE, DESCRIPTOR, &TOUCH_CLASS) else {
            return None;
        };

        let mut input: *mut AParcel = ptr::null_mut();
        let mut output: *mut AParcel = ptr::null_mut();
        let mut ok = unsafe { AIBinder_prepareTransaction(binder, &mut input) } == STATUS_OK;

        if ok {
            ok &= unsafe { AParcel_writeInt32(input, display_id) } == STATUS_OK;
            ok &= unsafe { AParcel_writeInt32(input, mode) } == STATUS_OK;
        }

        if ok {
            ok &= unsafe {
                AIBinder_transact(binder, 1, &mut input, &mut output, FLAG_PRIVATE_VENDOR)
            } == STATUS_OK;
        } else if !input.is_null() {
            unsafe { AParcel_delete(input) };
        }

        let mut result = -1i32;
        if ok && !output.is_null() {
            let mut status: *mut AStatus = ptr::null_mut();
            ok &= unsafe { AParcel_readStatusHeader(output, &mut status) } == STATUS_OK;
            if !status.is_null() {
                ok &= unsafe { AStatus_isOk(status) };
                unsafe { AStatus_delete(status) };
            } else {
                ok = false;
            }

            if ok {
                ok &= unsafe { AParcel_readInt32(output, &mut result) } == STATUS_OK;
            }
        } else {
            ok = false;
        }

        if !output.is_null() {
            unsafe { AParcel_delete(output) };
        }
        unsafe { AIBinder_decStrong(binder) };
        if ok { Some(result) } else { None }
    }

    pub fn set_display_feature(case_id: i32, mode_id: i32, cookie: i32) -> bool {
        const SERVICE: &[u8] =
            b"vendor.xiaomi.hardware.displayfeature_aidl.IDisplayFeature/default\0";
        const DESCRIPTOR: &[u8] = b"vendor.xiaomi.hardware.displayfeature_aidl.IDisplayFeature\0";
        let Some(binder) = check_service(SERVICE, DESCRIPTOR, &DISPLAY_CLASS) else {
            return false;
        };

        let mut input: *mut AParcel = ptr::null_mut();
        let mut output: *mut AParcel = ptr::null_mut();
        let mut ok = unsafe { AIBinder_prepareTransaction(binder, &mut input) } == STATUS_OK;

        for value in [0, case_id, mode_id, cookie] {
            if ok {
                ok &= unsafe { AParcel_writeInt32(input, value) } == STATUS_OK;
            }
        }

        if ok {
            ok &= unsafe {
                AIBinder_transact(binder, 7, &mut input, &mut output, FLAG_PRIVATE_VENDOR)
            } == STATUS_OK;
        } else if !input.is_null() {
            unsafe { AParcel_delete(input) };
        }

        if ok && !output.is_null() {
            let mut status: *mut AStatus = ptr::null_mut();
            ok &= unsafe { AParcel_readStatusHeader(output, &mut status) } == STATUS_OK;
            if !status.is_null() {
                ok &= unsafe { AStatus_isOk(status) };
                unsafe { AStatus_delete(status) };
            } else {
                ok = false;
            }
        } else {
            ok = false;
        }

        if !output.is_null() {
            unsafe { AParcel_delete(output) };
        }
        unsafe { AIBinder_decStrong(binder) };
        ok
    }
}

#[cfg(not(target_os = "android"))]
mod vendor_binder {
    pub fn touch_available() -> bool {
        false
    }
    pub fn display_available() -> bool {
        false
    }
    pub fn get_touch_mode(_: i32, _: i32) -> Option<i32> {
        None
    }

    pub fn set_touch_mode(_: i32, _: i32, _: i32) -> bool {
        false
    }
    pub fn set_display_feature(_: i32, _: i32, _: i32) -> bool {
        false
    }
}

static TOUCH_STATE: AtomicI32 = AtomicI32::new(-1);
static TOUCH_SUSTAINED_RATE: AtomicI32 = AtomicI32::new(-1);
static TOUCH_INSTANT_RATE: AtomicI32 = AtomicI32::new(-1);
static TOUCH_PANEL: AtomicI32 = AtomicI32::new(0);
static TOUCH_CONTROL_PATH: AtomicI32 = AtomicI32::new(0);
static DISPLAY_COLOR_STATE: AtomicI32 = AtomicI32::new(-1);
static DISPLAY_TEMP_STATE: AtomicI32 = AtomicI32::new(-1);
static DISPLAY_SUNLIGHT_STATE: AtomicI32 = AtomicI32::new(-1);
static DISPLAY_SILKY_STATE: AtomicI32 = AtomicI32::new(-1);
static DISPLAY_VIDEO_STATE: AtomicI32 = AtomicI32::new(-1);
static DISPLAY_DOLBY_STATE: AtomicI32 = AtomicI32::new(-1);
static PERFORMANCE_STATE: AtomicI32 = AtomicI32::new(-1);

fn abstract_addr(name: &str) -> Result<(SockAddrUn, u32), String> {
    let bytes = name.as_bytes();
    if bytes.len() + 1 >= 108 {
        return Err("socket name too long".into());
    }

    let mut addr = SockAddrUn {
        sun_family: AF_UNIX as u16,
        sun_path: [0; 108],
    };
    addr.sun_path[0] = 0;
    for (i, b) in bytes.iter().enumerate() {
        addr.sun_path[i + 1] = *b as i8;
    }
    Ok((addr, (2 + 1 + bytes.len()) as u32))
}

pub fn bind_listener(name: &str) -> Result<RawFd, String> {
    let (addr, len) = abstract_addr(name)?;
    let fd = unsafe { socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0) };
    if fd < 0 {
        return Err(format!(
            "socket failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    if unsafe { bind(fd, &addr as *const _ as *const c_void, len) } != 0 {
        let e = std::io::Error::last_os_error();
        unsafe { close(fd) };
        return Err(format!("bind failed: {e}"));
    }
    if unsafe { listen(fd, 16) } != 0 {
        let e = std::io::Error::last_os_error();
        unsafe { close(fd) };
        return Err(format!("listen failed: {e}"));
    }
    Ok(fd)
}

pub fn accept_stream(listener: RawFd) -> Result<UnixStream, String> {
    let fd = unsafe {
        accept4(
            listener,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            SOCK_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(format!(
            "accept failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(unsafe { UnixStream::from_raw_fd(fd) })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AppUidPolicy {
    SelinuxOnly,
    Enforce(u32),
    EnforceAppId(u32),
    Reject,
}

static APP_UID_POLICY: OnceLock<AppUidPolicy> = OnceLock::new();
static APP_CLIENT_SEEN: AtomicI32 = AtomicI32::new(0);
static APP_CLIENT_TRANSPORT: AtomicI32 = AtomicI32::new(0);

fn package_app_id_from_packages_list(contents: &str, package: &str) -> Option<u32> {
    contents.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        if fields.next()? != package {
            return None;
        }
        fields
            .next()?
            .parse::<u32>()
            .ok()
            .filter(|uid| *uid >= 10_000)
            .map(|uid| uid % 100_000)
    })
}

fn configured_app_uid_policy() -> AppUidPolicy {
    *APP_UID_POLICY.get_or_init(|| match std::env::var("PERIDOT_APP_UID") {
        Err(std::env::VarError::NotPresent) => match std::env::var("PERIDOT_APP_PACKAGE") {
            Err(std::env::VarError::NotPresent) => AppUidPolicy::SelinuxOnly,
            Err(std::env::VarError::NotUnicode(_)) => AppUidPolicy::Reject,
            Ok(package) => fs::read_to_string("/data/system/packages.list")
                .ok()
                .and_then(|contents| package_app_id_from_packages_list(&contents, &package))
                .map(AppUidPolicy::EnforceAppId)
                .unwrap_or(AppUidPolicy::Reject),
        },
        Err(std::env::VarError::NotUnicode(_)) => AppUidPolicy::Reject,
        Ok(value) => value
            .parse::<u32>()
            .ok()
            .filter(|uid| *uid >= 10_000)
            .map(AppUidPolicy::Enforce)
            .unwrap_or(AppUidPolicy::Reject),
    })
}

fn peer_uid(stream: &UnixStream) -> Result<u32, String> {
    let mut credentials = UCred {
        pid: 0,
        uid: u32::MAX,
        gid: u32::MAX,
    };
    let mut length = std::mem::size_of::<UCred>() as u32;
    let result = unsafe {
        getsockopt(
            stream.as_raw_fd(),
            SOL_SOCKET,
            SO_PEERCRED,
            &mut credentials as *mut UCred as *mut c_void,
            &mut length,
        )
    };
    if result != 0 {
        return Err(format!(
            "SO_PEERCRED failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    if length != std::mem::size_of::<UCred>() as u32 {
        return Err(format!("unexpected SO_PEERCRED length: {length}"));
    }
    Ok(credentials.uid)
}

fn tcp_endpoint(value: &str) -> Option<(&str, u16)> {
    let (address, port) = value.split_once(':')?;
    let port = u16::from_str_radix(port, 16).ok()?;
    Some((address, port))
}

fn tcp_client_uid_from_table(table: &str, server_port: u16, client_port: u16) -> Option<u32> {
    table.lines().skip(1).find_map(|line| {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 8 || fields[3] != "01" {
            return None;
        }
        let (local_address, local_port) = tcp_endpoint(fields[1])?;
        let (remote_address, remote_port) = tcp_endpoint(fields[2])?;
        if local_address != "0100007F"
            || remote_address != "0100007F"
            || local_port != client_port
            || remote_port != server_port
        {
            return None;
        }
        fields[7].parse::<u32>().ok()
    })
}

fn tcp_peer_uid(stream: &TcpStream) -> Result<u32, String> {
    let server_port = stream
        .local_addr()
        .map_err(|error| format!("loopback local address: {error}"))?
        .port();
    let client_port = stream
        .peer_addr()
        .map_err(|error| format!("loopback peer address: {error}"))?
        .port();

    let mut last_read_error = None;
    // Android's proc socket table can lag accept() briefly on vendor kernels.
    // Stay within the app's three-second request timeout, but do not reject a
    // legitimate package connection merely because the first snapshot raced.
    for _ in 0..500 {
        match fs::read_to_string("/proc/net/tcp") {
            Ok(table) => {
                if let Some(uid) = tcp_client_uid_from_table(&table, server_port, client_port) {
                    return Ok(uid);
                }
            }
            Err(error) => last_read_error = Some(error.to_string()),
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    Err(format!(
        "loopback peer UID unavailable from /proc/net/tcp for 127.0.0.1:{client_port} -> 127.0.0.1:{server_port}{}",
        last_read_error
            .map(|error| format!(" ({error})"))
            .unwrap_or_default()
    ))
}

fn client_uid_allowed(peer: u32, policy: AppUidPolicy) -> bool {
    peer == 0
        || match policy {
            AppUidPolicy::SelinuxOnly => true,
            AppUidPolicy::Enforce(expected) => peer == expected,
            AppUidPolicy::EnforceAppId(expected) => peer >= 10_000 && peer % 100_000 == expected,
            AppUidPolicy::Reject => false,
        }
}

fn record_app_client(peer: u32, transport: i32) {
    if peer != 0 {
        APP_CLIENT_SEEN.store(1, Ordering::Release);
        APP_CLIENT_TRANSPORT.store(transport, Ordering::Release);
    }
}

pub fn connect_stream(name: &str) -> Result<UnixStream, String> {
    let (addr, len) = abstract_addr(name)?;
    let fd = unsafe { socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0) };
    if fd < 0 {
        return Err(format!(
            "socket failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    if unsafe { connect(fd, &addr as *const _ as *const c_void, len) } != 0 {
        let e = std::io::Error::last_os_error();
        unsafe { close(fd) };
        return Err(format!("connect failed: {e}"));
    }
    Ok(unsafe { UnixStream::from_raw_fd(fd) })
}

fn read_trimmed<P: AsRef<Path>>(path: P) -> Result<String, String> {
    fs::read_to_string(path.as_ref())
        .map(|s| s.trim().to_string())
        .map_err(|e| format!("read {}: {e}", path.as_ref().display()))
}

fn write_verified(path: &Path, value: &str) -> Result<String, String> {
    service_control::write(path, format!("{value}\n"))
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    read_trimmed(path)
}

const PERIDOT_MAX_FCC_UA: i64 = 22_000_000;
const BYPASS_CHARGING_PATHS: [&str; 2] = [
    "/sys/class/power_supply/battery/bypass_charging",
    "/sys/class/power_supply/battery/bypass_charge",
];

static CHARGING_LAST_USB_ONLINE: AtomicI32 = AtomicI32::new(-1);
static CHARGING_BOOT_RESTORE_DONE: AtomicI32 = AtomicI32::new(0);

fn charging_path() -> PathBuf {
    PathBuf::from("/sys/class/power_supply/usb/sic_mode")
}

fn charging_fcc_path() -> Option<PathBuf> {
    None
}

fn bypass_charging_path() -> Option<PathBuf> {
    let capability = Path::new("/sys/class/power_supply/battery/bypass_charging_supported");
    if !Path::new("/sys/class/power_supply/battery/bypass_charging_active").exists()
        || read_trimmed(capability)
            .ok()
            .and_then(|value| parse_kernel_bool(&value))
            != Some(true)
    {
        return None;
    }
    BYPASS_CHARGING_PATHS
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.exists())
}

fn parse_kernel_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "y" | "yes" | "true" | "on" | "enabled" => Some(true),
        "0" | "n" | "no" | "false" | "off" | "disabled" => Some(false),
        _ => None,
    }
}

fn bypass_charging_state() -> i32 {
    bypass_charging_path()
        .and_then(|path| read_trimmed(path).ok())
        .and_then(|value| parse_kernel_bool(&value))
        .map(i32::from)
        .unwrap_or(-1)
}

fn apply_bypass_charging(enabled: bool) -> Result<(), String> {
    let path = bypass_charging_path()
        .ok_or_else(|| "bypass charging is not supported by this kernel".to_string())?;
    // The OEM driver restarts its neutral-current observation on a write,
    // including a repeated enable. Reads, page changes and wake restoration
    // must not restart that hardware settling interval.
    if read_trimmed(&path)
        .ok()
        .and_then(|value| parse_kernel_bool(&value))
        == Some(enabled)
    {
        return Ok(());
    }
    let requested = if enabled { "1" } else { "0" };
    let readback = write_verified(&path, requested)?;
    let actual = parse_kernel_bool(&readback)
        .ok_or_else(|| format!("bypass charging readback is invalid: {readback}"))?;
    if actual != enabled {
        return Err(format!(
            "bypass charging verify failed: requested {}, live {}",
            i32::from(enabled),
            i32::from(actual)
        ));
    }
    Ok(())
}

#[derive(Default)]
struct BypassPolicyRuntime {
    selection: Option<(bool, i32)>,
    held: bool,
    last_write: Option<Instant>,
}

static BYPASS_POLICY: OnceLock<Mutex<BypassPolicyRuntime>> = OnceLock::new();
static BYPASS_TRANSACTION: Mutex<()> = Mutex::new(());
static BYPASS_POLICY_ERROR: AtomicI32 = AtomicI32::new(0);
static BYPASS_MONITOR_DIRTY: Mutex<bool> = Mutex::new(true);
static BYPASS_MONITOR_WAKE: Condvar = Condvar::new();

fn wake_bypass_monitor() {
    *BYPASS_MONITOR_DIRTY
        .lock()
        .expect("bypass monitor poisoned") = true;
    BYPASS_MONITOR_WAKE.notify_one();
}

fn reconcile_bypass(enabled: bool, threshold: i32, explicit: bool) -> Result<(), String> {
    let mut policy = BYPASS_POLICY
        .get_or_init(|| Mutex::new(BypassPolicyRuntime::default()))
        .lock()
        .map_err(|_| "bypass policy lock poisoned".to_string())?;
    let live = bypass_charging_state();
    if live < 0 {
        return Err("bypass charging is not supported by this kernel".into());
    }
    if policy.selection != Some((enabled, threshold)) {
        // Recover the threshold latch from the real hardware after a daemon
        // restart, rather than briefly resuming charging in its release band.
        policy.held = live == 1;
        policy.selection = Some((enabled, threshold));
    }
    let desired = bypass_policy::target(
        enabled,
        threshold,
        battery("capacity").parse::<i32>().ok(),
        policy.held,
    )?;
    let transition = desired != policy.held;
    if live != i32::from(desired) {
        // Events drive normal transitions. Bound retries of a vendor conflict;
        // never run a sysfs write loop or restart an unchanged request.
        if !explicit
            && !transition
            && policy
                .last_write
                .is_some_and(|time| time.elapsed() < Duration::from_secs(5))
        {
            return Ok(());
        }
        policy.last_write = Some(Instant::now());
        apply_bypass_charging(desired)?;
    }
    policy.held = desired;
    BYPASS_POLICY_ERROR.store(0, Ordering::Release);
    Ok(())
}

fn maintain_bypass_policy() {
    let _transaction = BYPASS_TRANSACTION
        .lock()
        .expect("bypass transaction poisoned");
    let state = persisted_state().lock().ok().map(|state| state.clone());
    if let Some(state) = state.filter(|state| state.bypass_charging == 1) {
        if let Err(error) = reconcile_bypass(true, state.bypass_threshold, false) {
            if BYPASS_POLICY_ERROR.swap(1, Ordering::AcqRel) == 0 {
                eprintln!("PERIDOT_BYPASS_POLICY_FAIL {error}");
            }
        }
    }
}

fn configure_bypass(enabled: Option<bool>, threshold: Option<i32>) -> Result<(), String> {
    let _transaction = BYPASS_TRANSACTION
        .lock()
        .map_err(|_| "bypass transaction poisoned")?;
    let old = persisted_state()
        .lock()
        .map_err(|_| "persisted state poisoned")?
        .clone();
    let threshold_only = enabled.is_none();
    let enabled = enabled.unwrap_or(old.bypass_charging == 1);
    let threshold = threshold.unwrap_or(old.bypass_threshold);
    if !bypass_policy::valid_threshold(threshold) {
        return Err("invalid bypass threshold".into());
    }
    if threshold_only && !enabled {
        if bypass_charging_path().is_none() {
            return Err("bypass charging is not supported by this kernel".into());
        }
        // Selecting a future threshold while OFF must not change hardware.
        return mutate_persisted_state(|state| state.bypass_threshold = threshold);
    }
    if enabled && old.bypass_charging != 1 && old.charging > 0 && charging_fcc_path().is_some() {
        // While armed, threshold charging belongs to OEM, not a saved watt tier.
        apply_charging_profile(0)?;
    }
    reconcile_bypass(enabled, threshold, true)?;
    if let Err(error) = mutate_persisted_state(|state| {
        state.bypass_charging = i32::from(enabled);
        state.bypass_threshold = threshold;
    }) {
        let _ = reconcile_bypass(old.bypass_charging == 1, old.bypass_threshold, true);
        return Err(error);
    }
    if !enabled && old.bypass_charging == 1 && charging_fcc_path().is_some() {
        apply_charging_profile(normalize_charging_profile(old.charging))?;
    }
    wake_bypass_monitor();
    Ok(())
}

fn valid_charging_profile(profile: i32) -> bool {
    matches!(profile, 0 | 25 | 33 | 65 | 85 | 90)
}

fn normalize_charging_profile(profile: i32) -> i32 {
    match profile {
        // v1.18.x stored the old boost switch as 8. Preserve that intent as
        // the full Peridot charging tier after upgrading.
        8 => 90,
        value if valid_charging_profile(value) => value,
        _ => 0,
    }
}

fn charging_profile_fcc_ua(profile: i32) -> Option<i64> {
    match profile {
        25 => Some(6_111_000),
        33 => Some(8_067_000),
        65 => Some(15_889_000),
        85 => Some(20_778_000),
        90 => Some(PERIDOT_MAX_FCC_UA),
        _ => None,
    }
}

fn read_i64(path: &Path) -> Option<i64> {
    read_trimmed(path).ok()?.parse::<i64>().ok()
}

fn write_charging_if_present(path: &Path, value: &str) -> Result<Option<String>, String> {
    if path.exists() {
        write_verified(path, value).map(Some)
    } else {
        Ok(None)
    }
}

fn apply_charging_profile(profile: i32) -> Result<(), String> {
    // Profile 0 = OEM default (no daemon intervention needed).
    // Profiles 1-4 are defined in the UI but hardware calibration has never
    // been populated. Until charging_calibration is written (factory test),
    // all non-zero profiles silently retain OEM behaviour.
    if profile == 0 {
        return Ok(());
    }
    // Ponytail: OEM charging is active; charging_calibration never written.
    // Add real write path when hardware calibration data is available.
    Err("charging tiers unimplemented; OEM charging active".into())
}

#[cfg(target_os = "android")]
fn maintain_charging_state(profile: i32, bypass_charging: i32) -> Result<(), String> {
    let profile = normalize_charging_profile(profile);
    let online = usb("online").parse::<i32>().unwrap_or(-1);
    let previous_online = CHARGING_LAST_USB_ONLINE.swap(online, Ordering::AcqRel);

    if bypass_charging == 1 {
        maintain_bypass_policy();
        if previous_online != online {
            wake_bypass_monitor();
        }
        return Ok(());
    }
    if online != 1 || previous_online == 1 {
        return Ok(());
    }

    if profile != 0 {
        apply_charging_profile(profile)?;
    }

    Ok(())
}

fn normalize_reported_power_w(raw: i64) -> i32 {
    if raw <= 0 {
        -1
    } else if raw <= 500 {
        raw as i32
    } else if raw <= 500_000 {
        (raw / 1_000) as i32
    } else {
        (raw / 1_000_000) as i32
    }
}

fn charging_adapter_watts() -> i32 {
    [usb("apdo_max"), usb("power_max"), bms("adapting_power")]
        .into_iter()
        .filter_map(|value| value.parse::<i64>().ok())
        .map(normalize_reported_power_w)
        .max()
        .unwrap_or(-1)
}

fn battery(name: &str) -> String {
    read_trimmed(format!("/sys/class/power_supply/battery/{name}"))
        .unwrap_or_else(|_| "NA".to_string())
}

fn usb(name: &str) -> String {
    read_trimmed(format!("/sys/class/power_supply/usb/{name}")).unwrap_or_else(|_| "NA".to_string())
}

fn bms(name: &str) -> String {
    read_trimmed(format!("/sys/class/power_supply/bms/{name}")).unwrap_or_else(|_| "NA".to_string())
}

fn sanitize(value: String) -> String {
    value.replace(';', ",").replace(['\n', '\r'], " ")
}

fn read_cpu_online_mask_string() -> String {
    read_trimmed("/sys/devices/system/cpu/online").unwrap_or_else(|_| "NA".into())
}

fn live_cpu_mask() -> i32 {
    let mut mask = 0x01;

    for cpu in 1..=7 {
        let path = format!("/sys/devices/system/cpu/cpu{cpu}/online");

        if read_trimmed(&path)
            .map(|value| value == "1")
            .unwrap_or(false)
        {
            mask |= 1 << cpu;
        }
    }

    mask
}

fn core_ctl_paths() -> Vec<PathBuf> {
    let mut paths = Vec::<PathBuf>::new();

    for cpu in 0..=7 {
        let path = PathBuf::from(format!("/sys/devices/system/cpu/cpu{cpu}/core_ctl/enable"));

        if path.exists() && !paths.contains(&path) {
            paths.push(path);
        }
    }

    for policy in [0, 2, 5, 7] {
        let path = PathBuf::from(format!(
            "/sys/devices/system/cpu/cpufreq/policy{policy}/core_ctl/enable"
        ));

        if path.exists() && !paths.contains(&path) {
            paths.push(path);
        }
    }

    let global = PathBuf::from("/sys/devices/system/cpu/core_ctl/enable");

    if global.exists() && !paths.contains(&global) {
        paths.push(global);
    }

    paths
}

fn set_core_ctl_enabled(enabled: bool) -> Result<usize, String> {
    let paths = core_ctl_paths();

    CORE_CTL_NODE_COUNT.store(paths.len() as i32, Ordering::Release);

    let desired = if enabled { "1" } else { "0" };

    for path in &paths {
        let actual = write_verified(path, desired)?;

        if actual != desired {
            return Err(format!("core_ctl verify {}={actual}", path.display()));
        }
    }

    Ok(paths.len())
}

fn write_cpu_online(cpu: usize, online: bool) -> Result<(), String> {
    if !(1..=7).contains(&cpu) {
        return Err("CPU0 is pinned online; valid manual cores are CPU1-CPU7".into());
    }

    let path = PathBuf::from(format!("/sys/devices/system/cpu/cpu{cpu}/online"));

    if !path.exists() {
        return Err(format!("CPU{cpu} online node missing"));
    }

    let desired = if online { "1" } else { "0" };
    // Avoid redundant hotplug transitions. Vendor CPU teardown can briefly
    // return EBUSY while migrating work; retry only that transient, finitely.
    if read_trimmed(&path)? == desired {
        return Ok(());
    }
    for attempt in 0..3 {
        match service_control::write(&path, format!("{desired}\n")) {
            Ok(()) => break,
            Err(error) if error.raw_os_error() == Some(16) && attempt < 2 => {
                std::thread::sleep(Duration::from_millis(20 * (attempt + 1)));
            }
            Err(error) => return Err(format!("write {}: {error}", path.display())),
        }
    }
    let actual = read_trimmed(&path)?;

    if actual != desired {
        return Err(format!(
            "CPU{cpu} online verify expected={desired} actual={actual}"
        ));
    }

    Ok(())
}

fn apply_saved_cpu_mask(mask: i32) -> Result<(), String> {
    let mask = (mask | 0x01) & 0xFF;

    for cpu in 1..=7 {
        if (mask & (1 << cpu)) != 0 {
            write_cpu_online(cpu, true)?;
        }
    }

    for cpu in 1..=7 {
        if (mask & (1 << cpu)) == 0 {
            write_cpu_online(cpu, false)?;
        }
    }

    let actual = live_cpu_mask();

    if actual != mask {
        return Err(format!(
            "CPU online mask verify expected=0x{mask:02x} actual=0x{actual:02x}"
        ));
    }

    Ok(())
}

fn set_cpu_manual(enabled: bool) -> Result<(), String> {
    CPU_WRITE_ACK.store(-1, Ordering::Release);

    let result = (|| -> Result<(), String> {
        if enabled {
            let _ = set_core_ctl_enabled(false)?;
            let current = live_cpu_mask();

            mutate_persisted_state(|state| {
                state.cpu_manual = 1;
                state.cpu_online_mask = current | 0x01;
            })?;
        } else {
            apply_saved_cpu_mask(0xFF)?;
            let _ = set_core_ctl_enabled(true)?;

            mutate_persisted_state(|state| {
                state.cpu_manual = 0;
                state.cpu_online_mask = 0xFF;
            })?;
        }

        Ok(())
    })();

    CPU_WRITE_ACK.store(if result.is_ok() { 1 } else { 0 }, Ordering::Release);

    result
}

fn set_cpu_core(cpu: usize, online: bool) -> Result<(), String> {
    let manual = persisted_state()
        .lock()
        .ok()
        .map(|state| state.cpu_manual == 1)
        .unwrap_or(false);

    if !manual {
        CPU_WRITE_ACK.store(0, Ordering::Release);
        return Err("manual CPU core control is disabled".into());
    }

    CPU_WRITE_ACK.store(-1, Ordering::Release);

    match write_cpu_online(cpu, online) {
        Ok(()) => {
            let mask = live_cpu_mask();

            let persisted = mutate_persisted_state(|state| {
                state.cpu_online_mask = mask | 0x01;
            });
            CPU_WRITE_ACK.store(if persisted.is_ok() { 1 } else { 0 }, Ordering::Release);
            persisted
        }
        Err(error) => {
            CPU_WRITE_ACK.store(0, Ordering::Release);
            Err(error)
        }
    }
}

fn restore_cpu_state() {
    let state = persisted_state()
        .lock()
        .ok()
        .map(|state| state.clone())
        .unwrap_or_default();

    if state.cpu_manual == 1 {
        let result =
            set_core_ctl_enabled(false).and_then(|_| apply_saved_cpu_mask(state.cpu_online_mask));

        CPU_WRITE_ACK.store(if result.is_ok() { 1 } else { 0 }, Ordering::Release);
    } else if state.cpu_manual == 0 {
        let result = apply_saved_cpu_mask(0xFF).and_then(|_| set_core_ctl_enabled(true));

        CPU_WRITE_ACK.store(if result.is_ok() { 1 } else { 0 }, Ordering::Release);

        if result.is_ok() {
            let _ = mutate_persisted_state(|state| {
                state.cpu_manual = 0;
                state.cpu_online_mask = 0xFF;
            });
        }
    }
}

fn cpu_freq(cpu: usize) -> String {
    let candidates = [
        format!("/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_cur_freq"),
        format!("/sys/devices/system/cpu/cpu{cpu}/cpufreq/cpuinfo_cur_freq"),
    ];
    for p in candidates {
        if let Ok(v) = read_trimmed(&p) {
            return v;
        }
    }
    "NA".into()
}

fn policy_governor(policy: i32) -> String {
    read_trimmed(format!(
        "/sys/devices/system/cpu/cpufreq/policy{policy}/scaling_governor"
    ))
    .unwrap_or_else(|_| "unknown".into())
}

fn gpu_governor() -> String {
    peridot::read("governor").unwrap_or_else(|| "unavailable".into())
}

fn scheduler_name(path: &Path) -> Option<String> {
    let raw = read_trimmed(path).ok()?;
    if let (Some(a), Some(b)) = (raw.find('['), raw.find(']'))
        && b > a + 1
    {
        return Some(raw[a + 1..b].to_string());
    }
    None
}

fn ufs_scheduler_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let Ok(entries) = fs::read_dir("/sys/block") else {
        return paths;
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("sd") {
            continue;
        }

        let device = entry.path().join("device");
        let is_ufs = fs::canonicalize(&device)
            .ok()
            .map(|path| path.to_string_lossy().contains("ufshci"))
            .unwrap_or(false);
        if !is_ufs {
            continue;
        }

        let scheduler = entry.path().join("queue/scheduler");
        if scheduler.is_file() {
            paths.push(scheduler);
        }
    }

    paths.sort();
    paths
}

fn io_scheduler() -> String {
    let mut active: Option<String> = None;
    for path in ufs_scheduler_paths() {
        let Some(current) = scheduler_name(&path) else {
            return "unknown".into();
        };
        if active.as_ref().is_some_and(|value| value != &current) {
            return "mixed".into();
        }
        active = Some(current);
    }
    active.unwrap_or_else(|| "unknown".into())
}

fn list_contains(path: &str, target: &str) -> bool {
    read_trimmed(path)
        .map(|v| {
            v.split_whitespace()
                .map(|x| x.trim_matches(&['[', ']'][..]))
                .any(|x| x == target)
        })
        .unwrap_or(false)
}

fn set_cpu_governor(policy: i32, governor: &str) -> Result<(), String> {
    const SAFE: &[&str] = &[
        "walt",
        "migov",
        "conservative",
        "powersave",
        "performance",
        "schedutil",
    ];
    if !matches!(policy, 0 | 2 | 5 | 7) || !SAFE.contains(&governor) {
        return Err("invalid cpu governor request".into());
    }
    let base = format!("/sys/devices/system/cpu/cpufreq/policy{policy}");
    let available = format!("{base}/scaling_available_governors");
    if !list_contains(&available, governor) {
        return Err(format!(
            "governor {governor} not available for policy{policy}"
        ));
    }
    let path = PathBuf::from(format!("{base}/scaling_governor"));
    let actual = write_verified(&path, governor)?;
    if actual == governor {
        Ok(())
    } else {
        Err(format!("cpu governor verify {actual}"))
    }
}

fn cpu_policy_default_range(policy: i32) -> Option<(i32, i32)> {
    let table = cpu_available_frequencies(policy);
    Some((*table.first()?, *table.last()?))
}

fn parse_cpu_frequency_table(raw: &str) -> Vec<i32> {
    let mut frequencies = raw
        .split_whitespace()
        .filter_map(|value| value.parse::<i64>().ok())
        .filter(|khz| *khz > 0)
        .map(|khz| (khz / 1_000) as i32)
        .collect::<Vec<_>>();
    frequencies.sort_unstable();
    frequencies.dedup();
    frequencies
}

fn parse_cpu_time_in_state(raw: &str) -> Vec<i32> {
    let mut frequencies = raw
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter_map(|value| value.parse::<i64>().ok())
        .filter(|khz| *khz > 0)
        .map(|khz| (khz / 1_000) as i32)
        .collect::<Vec<_>>();
    frequencies.sort_unstable();
    frequencies.dedup();
    frequencies
}

fn cpu_available_frequencies(policy: i32) -> Vec<i32> {
    if !matches!(policy, 0 | 2 | 5 | 7) {
        return Vec::new();
    }

    let available = read_trimmed(format!(
        "/sys/devices/system/cpu/cpufreq/policy{policy}/scaling_available_frequencies"
    ))
    .map(|raw| parse_cpu_frequency_table(&raw))
    .unwrap_or_default();
    if !available.is_empty() {
        return available;
    }

    read_trimmed(format!(
        "/sys/devices/system/cpu/cpufreq/policy{policy}/stats/time_in_state"
    ))
    .map(|raw| parse_cpu_time_in_state(&raw))
    .unwrap_or_default()
}

fn peridot_cpu_khz(policy: i32, mhz_id: i32) -> Result<i64, String> {
    let base = format!("/sys/devices/system/cpu/cpufreq/policy{policy}");
    let raw = read_trimmed(format!("{base}/scaling_available_frequencies")).or_else(|_| {
        read_trimmed(format!("{base}/stats/time_in_state")).map(|raw| {
            raw.lines()
                .filter_map(|line| line.split_whitespace().next())
                .collect::<Vec<_>>()
                .join(" ")
        })
    })?;
    resolve_peridot_cpu_khz(&raw, mhz_id)
}

fn resolve_peridot_cpu_khz(raw: &str, mhz_id: i32) -> Result<i64, String> {
    let values: Vec<i64> = raw
        .split_whitespace()
        .filter_map(|v| v.parse().ok())
        .filter(|v: &i64| *v > 0 && *v / 1000 == i64::from(mhz_id))
        .collect();
    match values.as_slice() {
        [khz] => Ok(*khz),
        _ => Err(format!("CPU OPP {mhz_id} MHz unavailable or ambiguous")),
    }
}

fn cpu_frequency_table_csv(policy: i32) -> String {
    cpu_available_frequencies(policy)
        .iter()
        .map(i32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn read_cpu_cluster_limit(policy: i32, node: &str) -> Result<i32, String> {
    read_trimmed(format!(
        "/sys/devices/system/cpu/cpufreq/policy{policy}/{node}"
    ))
    .map_err(|error| format!("policy{policy} {node} read: {error}"))?
    .parse::<i64>()
    .map_err(|error| format!("policy{policy} {node} parse: {error}"))
    .and_then(|khz| {
        let mhz = (khz / 1_000) as i32;
        if mhz > 0 {
            Ok(mhz)
        } else {
            Err(format!("policy{policy} {node} reported {khz} kHz"))
        }
    })
}

fn get_cpu_cluster_live_limit(policy: i32, node: &str, fallback: i32) -> i32 {
    read_cpu_cluster_limit(policy, node).unwrap_or(fallback)
}

fn get_cpu_cluster_live_min_freq(policy: i32) -> i32 {
    let fallback = cpu_policy_default_range(policy)
        .map(|range| range.0)
        .unwrap_or(0);
    get_cpu_cluster_live_limit(policy, "scaling_min_freq", fallback)
}

fn get_cpu_cluster_live_max_freq(policy: i32) -> i32 {
    let fallback = cpu_policy_default_range(policy)
        .map(|range| range.1)
        .unwrap_or(0);
    get_cpu_cluster_live_limit(policy, "scaling_max_freq", fallback)
}

fn persisted_cpu_range(state: &PersistedState, policy: i32) -> Option<(i32, i32)> {
    let range = match policy {
        0 => (state.cpu_min_freq0, state.cpu_max_freq0),
        2 => (state.cpu_min_freq2, state.cpu_max_freq2),
        5 => (state.cpu_min_freq5, state.cpu_max_freq5),
        7 => (state.cpu_min_freq7, state.cpu_max_freq7),
        _ => return None,
    };

    (range.0 > 0 && range.1 > 0).then_some(range)
}

fn effective_cpu_target_range(state: &PersistedState, policy: i32) -> (i32, i32) {
    persisted_cpu_range(state, policy).unwrap_or_else(|| {
        let defaults = cpu_policy_default_range(policy).unwrap_or((0, 0));
        (
            get_cpu_cluster_live_limit(policy, "scaling_min_freq", defaults.0),
            get_cpu_cluster_live_limit(policy, "scaling_max_freq", defaults.1),
        )
    })
}

fn validate_cpu_frequency_range_against(
    policy: i32,
    min_mhz: i32,
    max_mhz: i32,
    available: &[i32],
) -> Result<(), String> {
    if !matches!(policy, 0 | 2 | 5 | 7) || min_mhz <= 0 || max_mhz <= 0 {
        return Err("invalid cpu frequency range".into());
    }
    if min_mhz > max_mhz {
        return Err("cpu minimum frequency exceeds maximum".into());
    }

    if available.is_empty() {
        return Err(format!("policy{policy} frequency table unavailable"));
    }
    if !available.contains(&min_mhz) {
        return Err(format!(
            "{min_mhz} MHz is not a supported policy{policy} frequency"
        ));
    }
    if !available.contains(&max_mhz) {
        return Err(format!(
            "{max_mhz} MHz is not a supported policy{policy} frequency"
        ));
    }

    Ok(())
}

fn validate_cpu_frequency_range(policy: i32, min_mhz: i32, max_mhz: i32) -> Result<(), String> {
    let available = cpu_available_frequencies(policy);
    validate_cpu_frequency_range_against(policy, min_mhz, max_mhz, &available)
}

const MI_THERMAL_CPU_LIMITS: &str = "/sys/devices/virtual/thermal/thermal_message/cpu_limits";
const MI_THERMAL_SCONFIG: &str = "/sys/devices/virtual/thermal/thermal_message/sconfig";
const MI_THERMAL_NO_LIMITS_MODE: i32 = 6;
const MTK_POWERHAL_CPU_FREQ: &str = "/proc/powerhal_cpu_ctrl/perfserv_freq";

static CPU_FREQ_APPLY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn cpu_freq_apply_lock() -> &'static Mutex<()> {
    CPU_FREQ_APPLY_LOCK.get_or_init(|| Mutex::new(()))
}

fn mi_thermal_cpu_limit_request(policy: i32, max_mhz: i32) -> String {
    format!("cpu{policy} {}", max_mhz as i64 * 1_000)
}

fn mtk_powerhal_cpu_range_request(policy: i32, min_mhz: i32, max_mhz: i32) -> String {
    format!(
        "{policy} {} {}",
        min_mhz as i64 * 1_000,
        max_mhz as i64 * 1_000
    )
}

fn persisted_cpu_ranges_active(state: &PersistedState) -> bool {
    [
        (state.cpu_min_freq0, state.cpu_max_freq0),
        (state.cpu_min_freq2, state.cpu_max_freq2),
        (state.cpu_min_freq5, state.cpu_max_freq5),
        (state.cpu_min_freq7, state.cpu_max_freq7),
    ]
    .into_iter()
    .any(|(min_mhz, max_mhz)| min_mhz > 0 && max_mhz > 0)
}

fn valid_mi_thermal_config_mode(mode: i32) -> bool {
    (0..=0x800).contains(&mode)
}

fn normalize_mi_thermal_config_mode(mode: i32) -> Result<i32, String> {
    // The kernel interface reports -1 until Xiaomi userspace publishes its
    // first explicit selection. mi_thermald is already running the normal
    // configuration at that point, so preserve it as mode 0 when taking
    // temporary ownership for a custom CPU range.
    if mode == -1 {
        Ok(0)
    } else if valid_mi_thermal_config_mode(mode) {
        Ok(mode)
    } else {
        Err(format!("MI thermal config reported invalid mode {mode}"))
    }
}

fn read_mi_thermal_config_mode() -> Result<i32, String> {
    read_trimmed(MI_THERMAL_SCONFIG)
        .map_err(|error| format!("MI thermal config read: {error}"))?
        .parse::<i32>()
        .map_err(|error| format!("MI thermal config parse: {error}"))
        .and_then(normalize_mi_thermal_config_mode)
}

fn write_mi_thermal_config_mode(mode: i32) -> Result<(), String> {
    if !valid_mi_thermal_config_mode(mode) {
        return Err(format!("invalid MI thermal config mode {mode}"));
    }

    service_control::write(MI_THERMAL_SCONFIG, mode.to_string())
        .map_err(|error| format!("MI thermal config write: {error}"))?;
    let actual = read_mi_thermal_config_mode()?;
    if actual != mode {
        return Err(format!(
            "MI thermal config verify requested {mode}, live {actual}"
        ));
    }

    Ok(())
}

fn ensure_cpu_unrestricted_mode_unlocked() -> Result<(), String> {
    CPU_THERMAL_MODE_ACK.store(-1, Ordering::Release);
    Ok(())
}

fn ensure_cpu_unrestricted_mode() -> Result<(), String> {
    let _guard = cpu_freq_apply_lock()
        .lock()
        .map_err(|_| "CPU frequency apply lock poisoned".to_string())?;
    ensure_cpu_unrestricted_mode_unlocked()
}

fn restore_cpu_thermal_mode_unlocked() -> Result<(), String> {
    Ok(())
}

fn restore_cpu_thermal_mode() -> Result<(), String> {
    let _guard = cpu_freq_apply_lock()
        .lock()
        .map_err(|_| "CPU frequency apply lock poisoned".to_string())?;
    restore_cpu_thermal_mode_unlocked()
}

fn write_optional_cpu_control(path: &str, value: &str, label: &str) -> Result<(), String> {
    if !Path::new(path).exists() {
        return Ok(());
    }

    service_control::write(path, value).map_err(|error| format!("{label} write: {error}"))
}

fn apply_cluster_freq_controls_unlocked(
    policy: i32,
    min_mhz: i32,
    max_mhz: i32,
) -> Result<(), String> {
    validate_cpu_frequency_range(policy, min_mhz, max_mhz)?;
    let min = peridot_cpu_khz(policy, min_mhz)?;
    let max = peridot_cpu_khz(policy, max_mhz)?;
    let base = format!("/sys/devices/system/cpu/cpufreq/policy{policy}");
    let read = || -> Result<(i64, i64), String> {
        let get = |node: &str| -> Result<i64, String> {
            read_trimmed(format!("{base}/{node}"))?
                .parse::<i64>()
                .map_err(|e| format!("policy{policy} {node} read: {e}"))
        };
        Ok((get("scaling_min_freq")?, get("scaling_max_freq")?))
    };
    let old = read()?;
    let status = limits::submit(
        min,
        max,
        old,
        |bound, value| {
            let node = match bound {
                limits::Bound::Min => "scaling_min_freq",
                limits::Bound::Max => "scaling_max_freq",
            };
            service_control::write(format!("{base}/{node}"), format!("{value}\n"))
                .map_err(|e| format!("policy{policy} {node} requested {value} kHz: {e}"))
        },
        read,
    )?;
    CPU_FREQ_WRITE_ACK.store(if status.exact { 1 } else { 2 }, Ordering::Release);
    eprintln!(
        "PERIDOT_CPU_REQUEST policy={policy} requested_khz={min}-{max} effective_khz={}-{} exact={}",
        status.min, status.max, status.exact
    );
    Ok(())
}

fn apply_cluster_freq_controls(policy: i32, min_mhz: i32, max_mhz: i32) -> Result<(), String> {
    let _guard = cpu_freq_apply_lock()
        .lock()
        .map_err(|_| "CPU frequency apply lock poisoned".to_string())?;
    apply_cluster_freq_controls_unlocked(policy, min_mhz, max_mhz)
}

fn set_cpu_cluster_min_freq(policy: i32, mhz: i32) -> Result<(), String> {
    let state = persisted_state()
        .lock()
        .ok()
        .map(|state| state.clone())
        .unwrap_or_default();
    let (_, current_max) = effective_cpu_target_range(&state, policy);
    set_cpu_cluster_freq_range(policy, mhz, current_max.max(mhz))
}

fn set_cpu_cluster_max_freq(policy: i32, mhz: i32) -> Result<(), String> {
    let state = persisted_state()
        .lock()
        .ok()
        .map(|state| state.clone())
        .unwrap_or_default();
    let (current_min, _) = effective_cpu_target_range(&state, policy);
    set_cpu_cluster_freq_range(policy, current_min.min(mhz), mhz)
}

fn set_cpu_cluster_freq_range(policy: i32, min_mhz: i32, max_mhz: i32) -> Result<(), String> {
    CPU_FREQ_WRITE_ACK.store(-1, Ordering::Release);
    let state_before = persisted_state()
        .lock()
        .ok()
        .map(|state| state.clone())
        .unwrap_or_default();
    let had_saved_range = persisted_cpu_ranges_active(&state_before);
    let guard = cpu_freq_apply_lock()
        .lock()
        .map_err(|_| "CPU frequency apply lock poisoned".to_string());
    let result = match guard {
        Ok(_guard) => {
            let mut result = validate_cpu_frequency_range(policy, min_mhz, max_mhz)
                .and_then(|_| ensure_cpu_unrestricted_mode_unlocked())
                .and_then(|_| apply_cluster_freq_controls_unlocked(policy, min_mhz, max_mhz));
            if result.is_ok() {
                result = mutate_persisted_state(|state| match policy {
                    0 => {
                        state.cpu_min_freq0 = min_mhz;
                        state.cpu_max_freq0 = max_mhz;
                    }
                    2 => {
                        state.cpu_min_freq2 = min_mhz;
                        state.cpu_max_freq2 = max_mhz;
                    }
                    5 => {
                        state.cpu_min_freq5 = min_mhz;
                        state.cpu_max_freq5 = max_mhz;
                    }
                    7 => {
                        state.cpu_min_freq7 = min_mhz;
                        state.cpu_max_freq7 = max_mhz;
                    }
                    _ => {}
                });
            } else if !had_saved_range {
                // The first custom range did not apply. Do not leave the
                // vendor thermal configuration changed for a failed command.
                let _ = restore_cpu_thermal_mode_unlocked();
            }
            result
        }
        Err(error) => Err(error),
    };

    if result.is_err() {
        CPU_FREQ_WRITE_ACK.store(0, Ordering::Release);
    }
    result
}

fn reset_cpu_cluster_freq_range(policy: i32) -> Result<(), String> {
    CPU_FREQ_WRITE_ACK.store(-1, Ordering::Release);
    let available = cpu_available_frequencies(policy);
    let Some((&min_mhz, &max_mhz)) = available.first().zip(available.last()) else {
        CPU_FREQ_WRITE_ACK.store(0, Ordering::Release);
        return Err(format!("policy{policy} frequency table unavailable"));
    };

    let guard = cpu_freq_apply_lock()
        .lock()
        .map_err(|_| "CPU frequency apply lock poisoned".to_string());
    let result = match guard {
        Ok(_guard) => {
            let state_before = persisted_state()
                .lock()
                .ok()
                .map(|state| state.clone())
                .unwrap_or_default();
            let mut result = if persisted_cpu_ranges_active(&state_before) {
                ensure_cpu_unrestricted_mode_unlocked()
                    .and_then(|_| apply_cluster_freq_controls_unlocked(policy, min_mhz, max_mhz))
            } else {
                apply_cluster_freq_controls_unlocked(policy, min_mhz, max_mhz)
            };
            if result.is_ok() {
                result = mutate_persisted_state(|state| match policy {
                    0 => {
                        state.cpu_min_freq0 = -1;
                        state.cpu_max_freq0 = -1;
                    }
                    2 => {
                        state.cpu_min_freq2 = -1;
                        state.cpu_max_freq2 = -1;
                    }
                    5 => {
                        state.cpu_min_freq5 = -1;
                        state.cpu_max_freq5 = -1;
                    }
                    7 => {
                        state.cpu_min_freq7 = -1;
                        state.cpu_max_freq7 = -1;
                    }
                    _ => {}
                });
            }

            if result.is_ok() {
                let state_after = persisted_state()
                    .lock()
                    .ok()
                    .map(|state| state.clone())
                    .unwrap_or_default();
                if !persisted_cpu_ranges_active(&state_after) {
                    restore_cpu_thermal_mode_unlocked()
                } else {
                    Ok(())
                }
            } else {
                result
            }
        }
        Err(error) => Err(error),
    };

    if result.is_err() {
        CPU_FREQ_WRITE_ACK.store(0, Ordering::Release);
    }
    result
}

fn set_gpu_governor(governor: &str) -> Result<(), String> {
    if governor != "msm-adreno-tz" {
        return Err("Peridot exposes only msm-adreno-tz".into());
    }
    peridot::write("governor", governor)
}

fn set_io_scheduler(scheduler: &str) -> Result<(), String> {
    const SAFE: &[&str] = &["none", "mq-deadline", "kyber", "bfq"];
    if !SAFE.contains(&scheduler) {
        return Err("invalid io scheduler request".into());
    }

    let paths = ufs_scheduler_paths();
    if paths.is_empty() {
        return Err("no UFS scheduler nodes detected".into());
    }

    for path in &paths {
        if !list_contains(path.to_string_lossy().as_ref(), scheduler) {
            return Err(format!(
                "scheduler {scheduler} not available on {}",
                path.display()
            ));
        }
    }

    for path in &paths {
        service_control::write(path, format!("{scheduler}\n"))
            .map_err(|error| format!("write {}: {error}", path.display()))?;
        let actual = scheduler_name(path).unwrap_or_else(|| "unknown".into());
        if actual != scheduler {
            return Err(format!(
                "scheduler verify {} expected {scheduler}, read {actual}",
                path.display()
            ));
        }
    }

    Ok(())
}

#[allow(dead_code)]
fn write_if_present(path: &str, value: &str) -> Result<bool, String> {
    let p = Path::new(path);
    if !p.exists() {
        return Ok(false);
    }
    service_control::write(p, format!("{value}\n")).map_err(|e| format!("write {path}: {e}"))?;
    Ok(true)
}

fn clear_gpu_cooling_cap() {
    // Thermal policy remains owned by the kernel and vendor.
}

fn profile_uses_ged_boost(profile: i32) -> bool {
    let _ = profile;
    false
}

fn write_beast_gpu_constraints() {
    let _ = peridot::apply_profile(3);
}

// Beast must never disable DVFS while the GPU is still running a previous
// OPP. This is especially important during boot: the MediaTek power service
// can publish its stock OPP 40 target after sys.boot_completed, and freezing
// DVFS at that point leaves the GPU stuck below 1300 MHz until the UI submits
// another profile command. Keep DVFS enabled while arming OPP 0, and disable
// it only after the live GED frequency confirms 1300 MHz.
fn arm_or_lock_beast_gpu() -> bool {
    peridot::apply_profile(3).is_ok()
}

fn settle_beast_gpu_lock(attempts: usize, delay: Duration) -> bool {
    let _ = (attempts, delay);
    peridot::apply_profile(3).is_ok()
}

pub fn enforce_performance_profile(profile: i32) -> bool {
    peridot::apply_profile(profile).is_ok()
}

pub fn apply_performance_profile(profile: i32) -> Result<(), String> {
    let _guard = gpu_profile_apply_lock()
        .lock()
        .map_err(|_| "GPU lock poisoned")?;
    peridot::apply_profile(profile)?;
    let (min, max) = peridot::profile_range(profile)?;
    mutate_persisted_state(|state| {
        state.perf = profile;
        state.gpu_min_freq_mhz = min;
        state.gpu_max_freq_mhz = max;
        state.gpu_governor = "msm-adreno-tz".into();
        state.gpu = "msm-adreno-tz".into();
        state.gpu_ged_boost = 0;
        state.gpu_uncap = i32::from(profile == 3);
        state.gpu_power_policy = "kernel-managed".into();
        state.gpu_profile_cpu_isolated = 1;
    })?;
    PERFORMANCE_STATE.store(profile, Ordering::Release);
    PERFORMANCE_PROFILE_SUPPORTED.store(1, Ordering::Release);
    PERFORMANCE_PROFILE_VERIFIED.store(1, Ordering::Release);
    PERFORMANCE_PROFILE_OK.store(
        i32::from(peridot::profile_verified(profile)),
        Ordering::Release,
    );
    Ok(())
}

fn classify_touch_panel_version(version: &str) -> i32 {
    let version = version.to_ascii_lowercase();
    if version.contains("goodix") || version.contains("gdix") || version.contains("gt9916") {
        1
    } else if version.contains("focal") || version.contains("fts") || version.contains("ft3683") {
        2
    } else {
        0
    }
}

fn touch_panel_code() -> i32 {
    let version = fs::read_to_string("/proc/tp_fw_version").unwrap_or_default();
    let detected = classify_touch_panel_version(&version);
    if detected != 0 {
        detected
    } else if Path::new("/sys/devices/platform/goodix_ts.0").exists() {
        1
    } else {
        0
    }
}

#[derive(Clone, Copy, Debug)]
struct TouchThpLayout {
    pid: u32,
    configured_rate_addr: u64,
    current_rate_addr: u64,
}

fn touch_service_pid() -> Option<u32> {
    let entries = fs::read_dir("/proc").ok()?;
    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(cmdline) = fs::read(format!("/proc/{pid}/cmdline")) else {
            continue;
        };
        let process_name = cmdline.split(|byte| *byte == 0).next().unwrap_or_default();
        if process_name == b"vendor.xiaomi.hw.touchfeature-service"
            || process_name.ends_with(b"/vendor.xiaomi.hw.touchfeature-service")
        {
            return Some(pid);
        }
    }
    None
}

fn parse_proc_map_range(line: &str) -> Option<(u64, u64, &str, u64)> {
    let mut fields = line.split_whitespace();
    let range = fields.next()?;
    let permissions = fields.next()?;
    let file_offset = u64::from_str_radix(fields.next()?, 16).ok()?;
    let (start, end) = range.split_once('-')?;
    Some((
        u64::from_str_radix(start, 16).ok()?,
        u64::from_str_radix(end, 16).ok()?,
        permissions,
        file_offset,
    ))
}

fn read_u16_from_slice(bytes: &[u8], offset: usize) -> Option<u16> {
    let raw: [u8; 2] = bytes.get(offset..offset + 2)?.try_into().ok()?;
    Some(u16::from_le_bytes(raw))
}

fn find_touch_thp_config_offset(bytes: &[u8]) -> Option<usize> {
    // Both Peridot panel variants use libtouchreport_hal.so and the same THP
    // timing block. Match all three vendor timing pairs instead of relying on
    // one firmware build's absolute virtual address.
    if bytes.len() < 0x38 {
        return None;
    }

    for offset in (0..=bytes.len() - 0x38).step_by(2) {
        let configured_super_rate = read_u16_from_slice(bytes, offset + 0x28)?;
        if read_u16_from_slice(bytes, offset) == Some(135)
            && read_u16_from_slice(bytes, offset + 0x04) == Some(135)
            && read_u16_from_slice(bytes, offset + 0x18) == Some(240)
            && read_u16_from_slice(bytes, offset + 0x1c) == Some(240)
            && read_u16_from_slice(bytes, offset + 0x24) == Some(240)
            && matches!(configured_super_rate, 240 | 480 | 500 | 600 | 650 | 1000)
        {
            return Some(offset);
        }
    }
    None
}

fn locate_touch_thp_layout() -> Result<TouchThpLayout, String> {
    Err("Peridot THP memory layout is not validated for Peridot".into())
}

fn read_touch_thp_rate(layout: TouchThpLayout, address: u64) -> Result<u16, String> {
    let mut memory = fs::OpenOptions::new()
        .read(true)
        .open(format!("/proc/{}/mem", layout.pid))
        .map_err(|e| format!("touch service memory: {e}"))?;
    memory
        .seek(SeekFrom::Start(address))
        .map_err(|e| format!("touch rate seek: {e}"))?;
    let mut raw = [0u8; 2];
    memory
        .read_exact(&mut raw)
        .map_err(|e| format!("touch rate read: {e}"))?;
    Ok(u16::from_le_bytes(raw))
}

fn write_touch_thp_rate(rate: u16) -> Result<TouchThpLayout, String> {
    if !matches!(rate, 240 | 480) {
        return Err(format!("unsupported Peridot THP rate {rate}"));
    }

    let layout = locate_touch_thp_layout()?;
    if service_control::original("touch.thp_rate").is_none() {
        let original = read_touch_thp_rate(layout, layout.configured_rate_addr)?;
        service_control::remember("touch.thp_rate", serde_json::json!(original))?;
    }
    let mut memory = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(format!("/proc/{}/mem", layout.pid))
        .map_err(|e| format!("touch service memory write: {e}"))?;
    memory
        .seek(SeekFrom::Start(layout.configured_rate_addr))
        .map_err(|e| format!("touch rate seek: {e}"))?;
    memory
        .write_all(&rate.to_le_bytes())
        .map_err(|e| format!("touch rate write: {e}"))?;

    let configured = read_touch_thp_rate(layout, layout.configured_rate_addr)?;
    if configured == rate {
        Ok(layout)
    } else {
        Err(format!(
            "THP configuration verify failed: requested {rate}, read {configured}"
        ))
    }
}

fn touch_profile_locked_rate(profile: i32) -> Option<u16> {
    match profile {
        1 => Some(240),
        2 | 3 => Some(480),
        _ => None,
    }
}

fn touch_profile_rates(profile: i32) -> (i32, i32) {
    match profile {
        1 => (240, 0),  // Native 240 Hz; whole-ms testers show about 250.
        2 => (480, 0),  // Native 480 Hz; whole-ms testers show about 500.
        3 => (1000, 0), // One-millisecond Android output cadence.
        _ => (-1, -1),
    }
}

fn goodix_report_rate_hz() -> Option<u16> {
    let raw = fs::read_to_string("/sys/devices/platform/goodix_ts.0/switch_report_rate").ok()?;
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized == "0" || normalized.contains("240hz") {
        Some(240)
    } else if normalized == "1" || normalized.contains("480hz") {
        Some(480)
    } else {
        None
    }
}

fn touch_profile_is_live(profile: i32) -> bool {
    matches!(profile, 0 | 1) && vendor_binder::get_touch_mode(0, 0) == Some(profile)
}

type TouchHalStep = (i32, i32, bool);

fn touch_hal_profile_sequence(profile: i32) -> Result<&'static [TouchHalStep], String> {
    // These are Xiaomi TouchFeature HAL modes, not calls into the HyperOS
    // Game Turbo application. The HAL lives in Peridot's vendor/ODM stack and
    // abstracts both supported Goodix and FocalTech panels.
    //
    // mode 0: game mode; 1: active mode; 2-6: response calibration;
    // 7: orientation; 202: super report path; 10001-10004: vendor Super Touch.
    // Super-report commands are deliberately last so a subsequent game-mode
    // write cannot return the report pipeline to its 240 Hz base path.
    // Native 240 and native 480 must remain separate transactions. Peridot's
    // mode-2 value 99 is the vendor high-sensitivity latch for the 240 Hz path:
    // the HAL normalizes its public readback to 4, but omitting the latch makes
    // the panel deliver only about 135 Hz while still claiming 240 Hz. The
    // 480 Hz super-report path uses the ordinary response calibration instead.
    let sequence: &'static [TouchHalStep] = match profile {
        0 => &[
            (10001, 0, false),
            (10002, 0, false),
            (10003, 0, false),
            (10004, 0, false),
            (202, 0, true),
            (0, 0, true),
            (1, 0, true),
            (2, 3, false),
            (3, 2, false),
            (4, 2, false),
            (5, 2, false),
            (6, 2, false),
            (7, 0, false),
        ],
        1 => &[
            (10001, 0, false),
            (10002, 0, false),
            (10003, 0, false),
            (10004, 0, false),
            (0, 1, true),
            (1, 1, true),
            (2, 99, true),
            (3, 4, false),
            (4, 4, false),
            (5, 4, false),
            (6, 0, false),
            (7, 0, false),
            (202, 1, true),
        ],
        2 => &[
            (10001, 0, false),
            (10002, 0, false),
            (10003, 0, false),
            (10004, 0, false),
            (0, 1, true),
            (1, 1, true),
            (2, 4, false),
            (3, 4, false),
            (4, 4, false),
            (5, 4, false),
            (6, 0, false),
            (7, 0, false),
            (202, 1, true),
        ],
        3 => &[
            (0, 1, true),
            (1, 1, true),
            (2, 4, false),
            (3, 4, false),
            (4, 4, false),
            (5, 4, false),
            (6, 0, false),
            (7, 0, false),
            (10002, 1, false),
            (10003, 1, false),
            (10004, 2, false),
            (202, 1, true),
            (10001, 1, true),
        ],
        _ => return Err("invalid touch profile".into()),
    };

    Ok(sequence)
}

fn apply_touch_hal_profile(profile: i32) -> Result<(), String> {
    let sequence = touch_hal_profile_sequence(profile)?;

    let mut failed_required = Vec::new();
    for &(mode, value, required) in sequence {
        if !vendor_binder::set_touch_mode(0, mode, value) && required {
            failed_required.push(mode);
        }
    }

    if failed_required.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "touch HAL rejected required modes {:?}",
            failed_required
        ))
    }
}

fn apply_touch_driver_fallback(profile: i32, panel: i32) -> Result<(), String> {
    // The generic HAL is the all-panel route. This fallback keeps 240/480
    // control available on Goodix-based ported ROMs that omit the HAL service.
    // FocalTech does not expose an equivalent writable report-rate sysfs node.
    if profile == 0 {
        return Ok(());
    }

    if panel != 1 {
        return Err("Peridot TouchFeature HAL unavailable for this panel".into());
    }

    if !matches!(profile, 1 | 2) {
        return Err("this touch profile requires the Peridot vendor touch HAL".into());
    }

    let value = if profile == 1 { "0" } else { "1" };
    service_control::write(
        "/sys/devices/platform/goodix_ts.0/switch_report_rate",
        value,
    )
    .map_err(|e| format!("Goodix report-rate fallback: {e}"))?;

    let expected = if profile == 1 { 240 } else { 480 };
    for _ in 0..4 {
        std::thread::sleep(Duration::from_millis(20));
        if goodix_report_rate_hz() == Some(expected) {
            return Ok(());
        }
    }
    Err(format!(
        "Goodix report-rate verify failed: requested {expected}, live {:?}",
        goodix_report_rate_hz()
    ))
}

fn write_peridot_game_mode(value: i32) -> Result<(), String> {
    if !vendor_binder::set_touch_mode(0, 0, value) {
        return Err("Xiaomi game-mode write failed".into());
    }
    if vendor_binder::get_touch_mode(0, 0) != Some(value) {
        return Err("Xiaomi game-mode readback mismatch".into());
    }
    Ok(())
}

fn restore_peridot_game_mode() -> Result<(), String> {
    if let Some(value) = service_control::original("touch.game_mode").and_then(|v| v.as_i64()) {
        write_peridot_game_mode(value as i32)?;
    }
    Ok(())
}

fn set_touch_profile(profile: i32) -> Result<(), String> {
    let _guard = touch_profile_apply_lock()
        .lock()
        .map_err(|_| "touch lock poisoned")?;
    apply_touch_profile_locked(profile)
}

fn apply_touch_profile_locked(profile: i32) -> Result<(), String> {
    if !matches!(profile, 0 | 1) {
        return Err("480 Hz is pending adaptation; resampled touch profiles were removed".into());
    }
    let previous = vendor_binder::get_touch_mode(0, 0)
        .filter(|v| matches!(v, 0 | 1))
        .ok_or("Xiaomi game-mode readback unavailable")?;
    service_control::remember("touch.game_mode", serde_json::json!(previous))?;
    touch_resampler::set_target_hz(0)?;
    let result = write_peridot_game_mode(profile)
        .and_then(|_| mutate_persisted_state(|state| state.touch = profile));
    if let Err(error) = result {
        let rollback = write_peridot_game_mode(previous);
        TOUCH_APPLY_ACK.store(0, Ordering::Release);
        return Err(match rollback {
            Ok(()) => error,
            Err(e) => format!("{error}; rollback: {e}"),
        });
    }
    TOUCH_STATE.store(profile, Ordering::Release);
    TOUCH_CONTROL_PATH.store(1, Ordering::Release);
    // Confirm the HAL mode only; never report a tester observation as measured Hz.
    TOUCH_SUSTAINED_RATE.store(-1, Ordering::Release);
    TOUCH_INSTANT_RATE.store(-1, Ordering::Release);
    TOUCH_APPLY_ACK.store(1, Ordering::Release);
    Ok(())
}

fn release_touch_control() -> Result<(), String> {
    let _guard = touch_profile_apply_lock()
        .lock()
        .map_err(|_| "touch profile apply lock poisoned")?;
    touch_resampler::set_target_hz(0)?;
    restore_peridot_game_mode()?;
    let originals = service_control::originals();
    let mut released = Vec::new();
    if service_control::original("touch.hal").is_some() && vendor_binder::touch_available() {
        // Release game/super-report requests once, not on every wake or boot.
        apply_touch_hal_profile(0)?;
    }
    if let Some(rate) = service_control::original("touch.thp_rate").and_then(|v| v.as_u64()) {
        let layout = locate_touch_thp_layout()?;
        let mut memory = OpenOptions::new()
            .read(true)
            .write(true)
            .open(format!("/proc/{}/mem", layout.pid))
            .map_err(|e| e.to_string())?;
        memory
            .seek(SeekFrom::Start(layout.configured_rate_addr))
            .map_err(|e| e.to_string())?;
        memory
            .write_all(&(rate as u16).to_le_bytes())
            .map_err(|e| e.to_string())?;
        if read_touch_thp_rate(layout, layout.configured_rate_addr)? != rate as u16 {
            return Err("OEM touch timing readback mismatch".into());
        }
    }
    for (key, value) in originals {
        let touch_node = key == "/proc/touch_boost/enable"
            || key == "/sys/devices/platform/goodix_ts.0/switch_report_rate";
        if touch_node {
            let original = value.as_str().ok_or("invalid original touch node")?;
            fs::write(&key, original).map_err(|e| format!("restore {key}: {e}"))?;
            let actual = read_trimmed(&key)?;
            if service_control::normalized_original(Path::new(&key), &actual) != original {
                return Err(format!("OEM touch readback mismatch: {key}"));
            }
        }
        if touch_node
            || matches!(
                key.as_str(),
                "touch.hal" | "touch.thp_rate" | "touch.game_mode"
            )
        {
            released.push(key);
        }
    }
    mutate_persisted_state(|state| state.touch = -1)?;
    service_control::forget(&released)?;
    TOUCH_STATE.store(-1, Ordering::Release);
    TOUCH_SUSTAINED_RATE.store(-1, Ordering::Release);
    TOUCH_INSTANT_RATE.store(-1, Ordering::Release);
    TOUCH_CONTROL_PATH.store(0, Ordering::Release);
    TOUCH_APPLY_ACK.store(-1, Ordering::Release);
    Ok(())
}

fn display_mode_apply_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn set_display_color(mode: i32) -> Result<(), String> {
    let _guard = display_mode_apply_lock()
        .lock()
        .map_err(|_| "display mode lock poisoned")?;
    apply_display_color_locked(mode)
}

fn apply_display_color_locked(mode: i32) -> Result<(), String> {
    if mode == -1 {
        // Do not guess a ROM's colour mode by forcing Vivid or Original.
        // The HAL has no portable ownership-release/get-original transaction.
        // Relinquish persistence so the ROM can own future colour changes.
        mutate_persisted_state(|state| state.display_color = -1)?;
        DISPLAY_COLOR_STATE.store(-1, Ordering::Release);
        DISPLAY_APPLY_ACK.store(-1, Ordering::Release);
        return Ok(());
    }
    service_control::remember("display.hal", serde_json::json!(true))?;
    let request = match mode {
        // Original PRO must enter expert mode, not the separate Standard case.
        0 => {
            let gamut = persisted_state()
                .lock()
                .map_err(|_| "state lock poisoned")?
                .expert_gamut;
            if !vendor_binder::set_display_feature(26, gamut, 0) {
                Err("display HAL expert activation failed".to_string())
            } else {
                verify_display_expert(0, gamut)
            }
        }
        1 | 2 => {
            let case_id = if mode == 1 { 0 } else { 1 };
            if vendor_binder::set_display_feature(case_id, 2, 255) {
                Ok(())
            } else {
                Err("display HAL color transaction failed".to_string())
            }
        }
        _ => Err("invalid display color mode".to_string()),
    };
    if let Err(error) = request {
        DISPLAY_APPLY_ACK.store(0, Ordering::Release);
        return Err(error);
    }
    DISPLAY_COLOR_STATE.store(mode, Ordering::Release);
    mutate_persisted_state(|s| s.display_color = mode)?;
    DISPLAY_APPLY_ACK.store(1, Ordering::Release);
    Ok(())
}

fn set_display_temp(mode: i32) -> Result<(), String> {
    let _guard = display_mode_apply_lock()
        .lock()
        .map_err(|_| "display mode lock poisoned")?;
    apply_display_temp_locked(mode)
}

fn apply_display_temp_locked(mode: i32) -> Result<(), String> {
    if mode == -1 {
        mutate_persisted_state(|state| state.display_temp = -1)?;
        DISPLAY_TEMP_STATE.store(-1, Ordering::Release);
        DISPLAY_APPLY_ACK.store(-1, Ordering::Release);
        return Ok(());
    }
    service_control::remember("display.hal", serde_json::json!(true))?;
    if !matches!(mode, 1..=3) {
        return Err("invalid display temperature".into());
    }
    if !vendor_binder::set_display_feature(23, mode, 255) {
        DISPLAY_APPLY_ACK.store(0, Ordering::Release);
        return Err("display HAL temperature transaction failed".into());
    }
    DISPLAY_TEMP_STATE.store(mode, Ordering::Release);
    mutate_persisted_state(|s| s.display_temp = mode)?;
    DISPLAY_APPLY_ACK.store(1, Ordering::Release);
    Ok(())
}

fn set_display_toggle(case_id: i32, enabled: bool, state: &AtomicI32) -> Result<(), String> {
    service_control::remember("display.hal", serde_json::json!(true))?;
    let val = if enabled { 1 } else { 0 };
    if !vendor_binder::set_display_feature(case_id, val, 255) {
        DISPLAY_APPLY_ACK.store(0, Ordering::Release);
        return Err(format!("display HAL feature {case_id} transaction failed"));
    }
    state.store(val, Ordering::Release);
    mutate_persisted_state(|s| match case_id {
        57 => s.silky = val,
        27 => s.video = val,
        44 => s.dolby = val,
        _ => {}
    })?;
    DISPLAY_APPLY_ACK.store(1, Ordering::Release);
    Ok(())
}

const DEFAULT_STATE_DIR: &str = "/data/adb/peridot-essential";
const STATE_DIR_ENV: &str = "PERIDOT_STATE_DIR";

static PERFORMANCE_PROFILE_SUPPORTED: AtomicI32 = AtomicI32::new(-1);
static PERFORMANCE_PROFILE_VERIFIED: AtomicI32 = AtomicI32::new(-1);
static PERFORMANCE_PROFILE_OK: AtomicI32 = AtomicI32::new(-1);
static PERSISTENCE_LOADED: AtomicI32 = AtomicI32::new(0);
static DISPLAY_APPLY_ACK: AtomicI32 = AtomicI32::new(-1);
static TOUCH_APPLY_ACK: AtomicI32 = AtomicI32::new(-1);
static KEEPALIVE_APPLY_ACK: AtomicI32 = AtomicI32::new(-1);
static KEEPALIVE_APPLY_COUNT: AtomicI32 = AtomicI32::new(0);
static CPU_WRITE_ACK: AtomicI32 = AtomicI32::new(-1);
static CPU_FREQ_WRITE_ACK: AtomicI32 = AtomicI32::new(-1);
static CPU_THERMAL_MODE_ACK: AtomicI32 = AtomicI32::new(-1);
static CORE_CTL_NODE_COUNT: AtomicI32 = AtomicI32::new(0);
static TOUCH_PROFILE_APPLY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static GPU_PROFILE_APPLY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

static DISPLAY_WIDTH: AtomicI32 = AtomicI32::new(-1);
static DISPLAY_HEIGHT: AtomicI32 = AtomicI32::new(-1);
static DISPLAY_DENSITY: AtomicI32 = AtomicI32::new(-1);
static DISPLAY_NATIVE_DENSITY: AtomicI32 = AtomicI32::new(-1);
static DISPLAY_HZ_X10: AtomicI32 = AtomicI32::new(-1);
static DISPLAY_MAX_HZ_X10: AtomicI32 = AtomicI32::new(-1);

fn gpu_profile_apply_lock() -> &'static Mutex<()> {
    GPU_PROFILE_APPLY_LOCK.get_or_init(|| Mutex::new(()))
}

fn touch_profile_apply_lock() -> &'static Mutex<()> {
    TOUCH_PROFILE_APPLY_LOCK.get_or_init(|| Mutex::new(()))
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct PersistedState {
    charging: i32,
    bypass_charging: i32,
    bypass_threshold: i32,
    touch: i32,
    display_color: i32,
    display_temp: i32,
    display_width: i32,
    display_height: i32,
    display_density: i32,
    sunlight: i32,
    silky: i32,
    video: i32,
    dolby: i32,
    expert_gamut: i32,
    expert: [i32; 8],
    perf: i32,
    gpu_profile_cpu_isolated: i32,
    cpu_manual: i32,
    cpu_online_mask: i32,
    cpu0: String,
    cpu2: String,
    cpu5: String,
    cpu7: String,
    gpu: String,
    io: String,
    brightness_prev: i32,
    zram_size_mb: i32,
    zram_algorithm: String,
    zram_swappiness: i32,
    gpu_min_freq_mhz: i32,
    gpu_max_freq_mhz: i32,
    gpu_governor: String,
    gpu_ged_boost: i32,
    gpu_uncap: i32,
    gpu_power_policy: String,
    cpu_min_freq0: i32,
    cpu_max_freq0: i32,
    cpu_min_freq2: i32,
    cpu_min_freq5: i32,
    cpu_max_freq2: i32,
    cpu_max_freq5: i32,
    cpu_min_freq7: i32,
    cpu_max_freq7: i32,
    cpu_thermal_mode_prev: i32,
    sconfig_override: i32,
}

impl Default for PersistedState {
    fn default() -> Self {
        Self {
            charging: 0,
            bypass_charging: 0,
            bypass_threshold: 0,
            touch: -1,
            display_color: -1,
            display_temp: -1,
            display_width: -1,
            display_height: -1,
            display_density: -1,
            sunlight: -1,
            silky: -1,
            video: -1,
            dolby: -1,
            expert_gamut: 1,
            expert: [128, 128, 128, 128, 0, 0, 50, 255],
            perf: 0,
            gpu_profile_cpu_isolated: 1,
            cpu_manual: 0,
            cpu_online_mask: 0xFF,
            cpu0: String::new(),
            cpu2: String::new(),
            cpu5: String::new(),
            cpu7: String::new(),
            gpu: String::new(),
            io: String::new(),
            brightness_prev: -1,
            zram_size_mb: 8192,
            zram_algorithm: "lz4".to_string(),
            zram_swappiness: 100,
            gpu_min_freq_mhz: -1,
            gpu_max_freq_mhz: -1,
            gpu_governor: String::new(),
            gpu_ged_boost: 0,
            gpu_uncap: 0,
            gpu_power_policy: String::new(),
            cpu_min_freq0: -1,
            cpu_max_freq0: -1,
            cpu_min_freq2: -1,
            cpu_min_freq5: -1,
            cpu_max_freq2: -1,
            cpu_max_freq5: -1,
            cpu_min_freq7: -1,
            cpu_max_freq7: -1,
            cpu_thermal_mode_prev: -1,
            sconfig_override: -1,
        }
    }
}

fn legacy_gpu_profile_cpu_signature(state: &PersistedState) -> bool {
    let governors = (
        state.cpu0.as_str(),
        state.cpu2.as_str(),
        state.cpu7.as_str(),
    );
    let ranges = (
        state.cpu_min_freq0,
        state.cpu_max_freq0,
        state.cpu_min_freq2,
        state.cpu_max_freq2,
        state.cpu_min_freq7,
        state.cpu_max_freq7,
    );

    match state.perf {
        3 => {
            governors == ("performance", "performance", "performance")
                && ranges == (2100, 2100, 3000, 3000, 3250, 3250)
        }
        1 => {
            governors == ("schedutil", "schedutil", "schedutil")
                && ranges == (300, 2100, 400, 3000, 1000, 3250)
        }
        2 => {
            governors == ("schedutil", "schedutil", "schedutil")
                && ranges == (300, 1400, 400, 1800, 1000, 1800)
        }
        0 => {
            governors == ("sugov_ext", "sugov_ext", "sugov_ext")
                && ranges == (-1, -1, -1, -1, -1, -1)
        }
        _ => false,
    }
}

fn migrate_legacy_gpu_profile_cpu_state(state: &mut PersistedState) -> bool {
    if state.gpu_profile_cpu_isolated == 1 {
        return false;
    }

    let reset_legacy_cpu = legacy_gpu_profile_cpu_signature(state);
    if reset_legacy_cpu {
        state.cpu0.clear();
        state.cpu2.clear();
        state.cpu7.clear();
        state.cpu_min_freq0 = -1;
        state.cpu_max_freq0 = -1;
        state.cpu_min_freq2 = -1;
        state.cpu_max_freq2 = -1;
        state.cpu_min_freq7 = -1;
        state.cpu_max_freq7 = -1;
    }

    state.gpu_profile_cpu_isolated = 1;
    reset_legacy_cpu
}

fn restore_vendor_cpu_defaults() {
    for policy in [0, 2, 5, 7] {
        if let Some((min, max)) = cpu_policy_default_range(policy) {
            let _ = apply_cluster_freq_controls(policy, min, max);
        }
        let _ = set_cpu_governor(policy, "walt");
    }
}

static PERSISTED_STATE: OnceLock<Mutex<PersistedState>> = OnceLock::new();
static RESOLVED_STATE_DIR: OnceLock<PathBuf> = OnceLock::new();

fn state_dir() -> &'static Path {
    RESOLVED_STATE_DIR
        .get_or_init(|| {
            std::env::var_os(STATE_DIR_ENV)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .unwrap_or_else(|| PathBuf::from(DEFAULT_STATE_DIR))
        })
        .as_path()
}

fn state_file() -> PathBuf {
    state_dir().join("state.conf")
}

fn load_persisted_state() -> PersistedState {
    let Ok(raw) = fs::read_to_string(state_file()) else {
        return cleared_device_state();
    };
    parse_persisted_state(&raw)
}

fn parse_persisted_state(raw: &str) -> PersistedState {
    let mut state = PersistedState::default();

    // State written before GPU/CPU profile isolation did not carry this key.
    // Mark an existing file as legacy until the parser finds the new marker.
    state.gpu_profile_cpu_isolated = 0;

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };

        let key = key.trim();
        let value = value.trim();
        let int_value = || value.parse::<i32>().ok();

        match key {
            "charging" => {
                if let Some(v) = int_value() {
                    state.charging = v;
                }
            }
            "bypass_charging" => {
                if let Some(v) = int_value() {
                    state.bypass_charging = if v == 1 { 1 } else { 0 };
                }
            }
            "bypass_threshold" => {
                if let Some(v) = int_value().filter(|v| bypass_policy::valid_threshold(*v)) {
                    state.bypass_threshold = v;
                }
            }
            "touch" => {
                if let Some(v) = int_value() {
                    state.touch = if matches!(v, 0 | 1) { v } else { -1 };
                }
            }
            "display_color" => {
                if let Some(v) = int_value() {
                    state.display_color = v;
                }
            }
            "display_temp" => {
                if let Some(v) = int_value() {
                    state.display_temp = v;
                }
            }
            "display_width" => {
                if let Some(v) = int_value() {
                    state.display_width = v;
                }
            }
            "display_height" => {
                if let Some(v) = int_value() {
                    state.display_height = v;
                }
            }
            "display_density" => {
                if let Some(v) = int_value() {
                    state.display_density = v;
                }
            }
            "sunlight" => {
                if let Some(v) = int_value() {
                    state.sunlight = v;
                }
            }
            "silky" => {
                if let Some(v) = int_value() {
                    state.silky = v;
                }
            }
            "video" => {
                if let Some(v) = int_value() {
                    state.video = v;
                }
            }
            "dolby" => {
                if let Some(v) = int_value() {
                    state.dolby = v;
                }
            }
            "expert_gamut" => {
                if let Some(v) = int_value() {
                    state.expert_gamut = v;
                }
            }
            "expert_1" => {
                if let Some(v) = int_value() {
                    state.expert[0] = v;
                }
            }
            "expert_2" => {
                if let Some(v) = int_value() {
                    state.expert[1] = v;
                }
            }
            "expert_3" => {
                if let Some(v) = int_value() {
                    state.expert[2] = v;
                }
            }
            "expert_4" => {
                if let Some(v) = int_value() {
                    state.expert[3] = v;
                }
            }
            "expert_5" => {
                if let Some(v) = int_value() {
                    state.expert[4] = v;
                }
            }
            "expert_6" => {
                if let Some(v) = int_value() {
                    state.expert[5] = v;
                }
            }
            "expert_7" => {
                if let Some(v) = int_value() {
                    state.expert[6] = v;
                }
            }
            "expert_8" => {
                if let Some(v) = int_value() {
                    state.expert[7] = v;
                }
            }
            "perf" => {
                if let Some(v) = int_value() {
                    state.perf = v;
                }
            }
            "gpu_profile_cpu_isolated" => {
                if let Some(v) = int_value() {
                    state.gpu_profile_cpu_isolated = if v == 1 { 1 } else { 0 };
                }
            }
            "cpu_manual" => {
                if let Some(v) = int_value() {
                    state.cpu_manual = if v == -1 {
                        -1
                    } else if v == 1 {
                        1
                    } else {
                        0
                    };
                }
            }
            "cpu_online_mask" => {
                if let Some(v) = int_value() {
                    state.cpu_online_mask = (v | 0x01) & 0xFF;
                }
            }
            "cpu0" => state.cpu0 = value.to_string(),
            "cpu2" => state.cpu2 = value.to_string(),
            "cpu5" => state.cpu5 = value.to_string(),
            "cpu7" => state.cpu7 = value.to_string(),
            "gpu" => state.gpu = value.to_string(),
            "io" => state.io = value.to_string(),
            "brightness_prev" => {
                if let Some(v) = int_value() {
                    state.brightness_prev = v;
                }
            }
            "zram_size_mb" => {
                if let Some(v) = int_value() {
                    state.zram_size_mb = v;
                }
            }
            "zram_algorithm" => state.zram_algorithm = value.to_string(),
            "zram_swappiness" => {
                if let Some(v) = int_value() {
                    state.zram_swappiness = v;
                }
            }
            "gpu_min_freq_mhz" => {
                if let Some(v) = int_value() {
                    state.gpu_min_freq_mhz = v;
                }
            }
            "gpu_max_freq_mhz" => {
                if let Some(v) = int_value() {
                    state.gpu_max_freq_mhz = v;
                }
            }
            "gpu_governor" => state.gpu_governor = value.to_string(),
            "gpu_ged_boost" => {
                if let Some(v) = int_value() {
                    state.gpu_ged_boost = v;
                }
            }
            "gpu_uncap" => {
                if let Some(v) = int_value() {
                    state.gpu_uncap = v;
                }
            }
            "gpu_power_policy" => state.gpu_power_policy = value.to_string(),
            "cpu_min_freq0" => {
                if let Some(v) = int_value() {
                    state.cpu_min_freq0 = v;
                }
            }
            "cpu_max_freq0" => {
                if let Some(v) = int_value() {
                    state.cpu_max_freq0 = v;
                }
            }
            "cpu_min_freq2" => {
                if let Some(v) = int_value() {
                    state.cpu_min_freq2 = v;
                }
            }
            "cpu_min_freq5" => {
                if let Some(v) = int_value() {
                    state.cpu_min_freq5 = v;
                }
            }
            "cpu_max_freq2" => {
                if let Some(v) = int_value() {
                    state.cpu_max_freq2 = v;
                }
            }
            "cpu_max_freq5" => {
                if let Some(v) = int_value() {
                    state.cpu_max_freq5 = v;
                }
            }
            "cpu_min_freq7" => {
                if let Some(v) = int_value() {
                    state.cpu_min_freq7 = v;
                }
            }
            "cpu_max_freq7" => {
                if let Some(v) = int_value() {
                    state.cpu_max_freq7 = v;
                }
            }
            "sconfig_override" => {
                if let Some(v) = int_value().filter(|v| *v == -1 || sconfig::supported_mode(*v)) {
                    state.sconfig_override = v;
                }
            }
            "cpu_thermal_mode_prev" => {
                if let Some(v) = int_value() {
                    state.cpu_thermal_mode_prev = if valid_mi_thermal_config_mode(v) {
                        v
                    } else {
                        -1
                    };
                }
            }
            _ => {}
        }
    }

    if state.charging != -1 {
        state.charging = normalize_charging_profile(state.charging);
    }
    if !matches!(state.touch, 0 | 1) {
        state.touch = -1;
    }
    if !(-1..=2).contains(&state.display_color) {
        state.display_color = -1;
    }
    if !matches!(state.display_temp, -1 | 1..=3) {
        state.display_temp = -1;
    }
    if !matches!(state.sunlight, -1 | 0 | 1) {
        state.sunlight = -1;
    }
    if !matches!(state.silky, -1 | 0 | 1) {
        state.silky = -1;
    }
    if !matches!(state.video, -1 | 0 | 1) {
        state.video = -1;
    }
    if !matches!(state.dolby, -1 | 0 | 1) {
        state.dolby = -1;
    }

    state
}

fn persisted_state() -> &'static Mutex<PersistedState> {
    PERSISTED_STATE.get_or_init(|| Mutex::new(load_persisted_state()))
}

fn save_persisted_state(state: &PersistedState) -> Result<(), String> {
    // Maintenance guards consume effective targets. Only the user's global
    // preferences may reach state.conf while an app temporarily owns controls.
    let global = app_runtime::global_preferences(state)?;
    let state = &global;
    let state_dir = state_dir();
    fs::create_dir_all(state_dir).map_err(|e| format!("state mkdir: {e}"))?;
    fs::set_permissions(state_dir, fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("state directory permissions: {e}"))?;

    let mut out = String::new();
    out.push_str(&format!("charging={}\n", state.charging));
    out.push_str(&format!("bypass_charging={}\n", state.bypass_charging));
    out.push_str(&format!("bypass_threshold={}\n", state.bypass_threshold));
    out.push_str(&format!("touch={}\n", state.touch));
    out.push_str(&format!("display_color={}\n", state.display_color));
    out.push_str(&format!("display_temp={}\n", state.display_temp));
    out.push_str(&format!("display_width={}\n", state.display_width));
    out.push_str(&format!("display_height={}\n", state.display_height));
    out.push_str(&format!("display_density={}\n", state.display_density));
    out.push_str(&format!("sunlight={}\n", state.sunlight));
    out.push_str(&format!("silky={}\n", state.silky));
    out.push_str(&format!("video={}\n", state.video));
    out.push_str(&format!("dolby={}\n", state.dolby));
    out.push_str(&format!("expert_gamut={}\n", state.expert_gamut));

    for i in 0..8 {
        out.push_str(&format!("expert_{}={}\n", i + 1, state.expert[i]));
    }

    out.push_str(&format!("perf={}\n", state.perf));
    out.push_str(&format!(
        "gpu_profile_cpu_isolated={}\n",
        state.gpu_profile_cpu_isolated
    ));
    out.push_str(&format!("cpu_manual={}\n", state.cpu_manual));
    out.push_str(&format!(
        "cpu_online_mask={}\n",
        state.cpu_online_mask | 0x01
    ));
    out.push_str(&format!("cpu0={}\n", state.cpu0));
    out.push_str(&format!("cpu2={}\n", state.cpu2));
    out.push_str(&format!("cpu5={}\n", state.cpu5));
    out.push_str(&format!("cpu7={}\n", state.cpu7));
    out.push_str(&format!("gpu={}\n", state.gpu));
    out.push_str(&format!("io={}\n", state.io));
    out.push_str(&format!("brightness_prev={}\n", state.brightness_prev));
    out.push_str(&format!("zram_size_mb={}\n", state.zram_size_mb));
    out.push_str(&format!("zram_algorithm={}\n", state.zram_algorithm));
    out.push_str(&format!("zram_swappiness={}\n", state.zram_swappiness));
    out.push_str(&format!("gpu_min_freq_mhz={}\n", state.gpu_min_freq_mhz));
    out.push_str(&format!("gpu_max_freq_mhz={}\n", state.gpu_max_freq_mhz));
    out.push_str(&format!("gpu_governor={}\n", state.gpu_governor));
    out.push_str(&format!("gpu_ged_boost={}\n", state.gpu_ged_boost));
    out.push_str(&format!("gpu_uncap={}\n", state.gpu_uncap));
    out.push_str(&format!("gpu_power_policy={}\n", state.gpu_power_policy));
    out.push_str(&format!("cpu_min_freq0={}\n", state.cpu_min_freq0));
    out.push_str(&format!("cpu_max_freq0={}\n", state.cpu_max_freq0));
    out.push_str(&format!("cpu_min_freq2={}\n", state.cpu_min_freq2));
    out.push_str(&format!("cpu_min_freq5={}\n", state.cpu_min_freq5));
    out.push_str(&format!("cpu_max_freq2={}\n", state.cpu_max_freq2));
    out.push_str(&format!("cpu_max_freq5={}\n", state.cpu_max_freq5));
    out.push_str(&format!("cpu_min_freq7={}\n", state.cpu_min_freq7));
    out.push_str(&format!("cpu_max_freq7={}\n", state.cpu_max_freq7));
    out.push_str(&format!("sconfig_override={}\n", state.sconfig_override));
    out.push_str(&format!(
        "cpu_thermal_mode_prev={}\n",
        state.cpu_thermal_mode_prev
    ));

    let state_file = state_file();
    let tmp = state_dir.join("state.conf.tmp");
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|e| format!("state open: {e}"))?;
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("state temporary-file permissions: {e}"))?;
    file.write_all(out.as_bytes())
        .map_err(|e| format!("state write: {e}"))?;
    file.sync_all()
        .map_err(|e| format!("state file sync: {e}"))?;
    drop(file);
    fs::rename(&tmp, &state_file).map_err(|e| format!("state rename: {e}"))?;
    fs::set_permissions(&state_file, fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("state file permissions: {e}"))?;
    fs::File::open(state_dir)
        .and_then(|dir| dir.sync_all())
        .map_err(|e| format!("state directory sync: {e}"))?;
    Ok(())
}

fn mutate_persisted_state<F>(f: F) -> Result<(), String>
where
    F: FnOnce(&mut PersistedState),
{
    let mut guard = persisted_state()
        .lock()
        .map_err(|_| "persisted state lock poisoned".to_string())?;
    let mut candidate = guard.clone();
    f(&mut candidate);
    if !app_runtime::transient_write() {
        save_persisted_state(&candidate)?;
    }
    *guard = candidate;
    Ok(())
}

fn expert_value_range(channel: i32) -> Option<(i32, i32)> {
    match channel {
        1..=3 => Some((50, 255)),
        4 => Some((-180, 180)),
        5 => Some((-50, 50)),
        6 => Some((-255, 255)),
        7 => Some((-100, 100)),
        8 => Some((170, 270)),
        _ => None,
    }
}

fn verify_display_expert(channel: i32, expected: i32) -> Result<(), String> {
    let mut last = "no display readback".to_string();
    for _ in 0..3 {
        std::thread::sleep(Duration::from_millis(60));
        let result = run_process_with_timeout(
            "/system/bin/dumpsys",
            &["vendor.xiaomi.hardware.displayfeature_aidl.IDisplayFeature/default"],
            Duration::from_millis(800),
        )
        .and_then(|output| {
            if !output.status.success() {
                return Err("display dump failed".into());
            }
            display_readback::expert(&output.stdout)
        });
        match result {
            Ok(values) if values[0] == 1 && values[(channel + 1) as usize] == expected => {
                return Ok(());
            }
            Ok(values) => {
                last = format!(
                    "expert readback differs: enabled={}, channel={channel}, requested={expected}, live={}",
                    values[0],
                    values[(channel + 1) as usize]
                )
            }
            Err(error) => last = error,
        }
    }
    Err(last)
}

fn set_expert_gamut(gamut: i32) -> Result<(), String> {
    let _guard = display_mode_apply_lock()
        .lock()
        .map_err(|_| "display mode lock poisoned")?;
    service_control::remember("display.hal", serde_json::json!(true))?;
    if !matches!(gamut, 1..=3) {
        return Err("invalid expert gamut".into());
    }

    let display_color = persisted_state()
        .lock()
        .ok()
        .map(|state| state.display_color)
        .unwrap_or(-1);

    if display_color != 0 {
        return Err("expert calibration requires Original PRO".into());
    }

    if !vendor_binder::set_display_feature(26, gamut, 0) {
        DISPLAY_APPLY_ACK.store(0, Ordering::Release);
        return Err("display HAL expert gamut transaction failed".into());
    }

    verify_display_expert(0, gamut).inspect_err(|_| {
        DISPLAY_APPLY_ACK.store(0, Ordering::Release);
    })?;
    mutate_persisted_state(|s| s.expert_gamut = gamut).inspect_err(|_| {
        DISPLAY_APPLY_ACK.store(0, Ordering::Release);
    })?;
    DISPLAY_APPLY_ACK.store(1, Ordering::Release);
    Ok(())
}

fn set_expert_channel(channel: i32, value: i32) -> Result<(), String> {
    let _guard = display_mode_apply_lock()
        .lock()
        .map_err(|_| "display mode lock poisoned")?;
    service_control::remember("display.hal", serde_json::json!(true))?;
    let display_color = persisted_state()
        .lock()
        .ok()
        .map(|state| state.display_color)
        .unwrap_or(-1);

    if display_color != 0 {
        return Err("expert calibration requires Original PRO".into());
    }

    let Some((min, max)) = expert_value_range(channel) else {
        return Err("invalid expert channel".into());
    };

    if value < min || value > max {
        return Err(format!(
            "expert channel {channel} out of range {min}..{max}: {value}"
        ));
    }

    if !vendor_binder::set_display_feature(26, value, channel) {
        DISPLAY_APPLY_ACK.store(0, Ordering::Release);
        return Err(format!(
            "display HAL expert channel transaction failed channel={channel}"
        ));
    }

    verify_display_expert(channel, value).inspect_err(|_| {
        DISPLAY_APPLY_ACK.store(0, Ordering::Release);
    })?;
    mutate_persisted_state(|s| s.expert[(channel - 1) as usize] = value).inspect_err(|_| {
        DISPLAY_APPLY_ACK.store(0, Ordering::Release);
    })?;
    DISPLAY_APPLY_ACK.store(1, Ordering::Release);
    Ok(())
}

fn reset_expert_display() -> Result<(), String> {
    const DEFAULTS: &[(i32, i32)] = &[
        (1, 255),
        (2, 255),
        (3, 255),
        (4, 0),
        (5, 0),
        (6, 0),
        (7, 50),
        (8, 262),
    ];

    set_display_color(0)?;
    set_display_temp(2)?;

    if !vendor_binder::set_display_feature(26, 1, 0) {
        return Err("display HAL reset gamut failed".into());
    }

    for &(channel, value) in DEFAULTS {
        if !vendor_binder::set_display_feature(26, value, channel) {
            return Err(format!("display HAL reset channel {channel} failed"));
        }
    }

    for &(channel, value) in DEFAULTS {
        verify_display_expert(channel, value).inspect_err(|_| {
            DISPLAY_APPLY_ACK.store(0, Ordering::Release);
        })?;
    }
    verify_display_expert(0, 1)?;
    mutate_persisted_state(|s| {
        s.display_color = 0;
        s.display_temp = 2;
        s.expert_gamut = 1;
        s.expert = [255, 255, 255, 0, 0, 0, 50, 262];
    })
    .inspect_err(|_| {
        DISPLAY_APPLY_ACK.store(0, Ordering::Release);
    })?;
    DISPLAY_APPLY_ACK.store(1, Ordering::Release);

    Ok(())
}

fn run_process_with_timeout(
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<ProcessOutput, String> {
    let mut child = ProcessCommand::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("{} {} spawn: {error}", program, args.join(" ")))?;

    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("{} {} stdout unavailable", program, args.join(" ")))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| format!("{} {} stderr unavailable", program, args.join(" ")))?;
    let stdout_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).map(|_| bytes)
    });

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(format!(
                    "{} {} timed out after {} ms",
                    program,
                    args.join(" "),
                    timeout.as_millis()
                ));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(format!("{} {} wait: {error}", program, args.join(" ")));
            }
        }
    };

    let stdout = stdout_reader
        .join()
        .map_err(|_| format!("{} {} stdout reader panicked", program, args.join(" ")))?
        .map_err(|error| format!("{} {} stdout: {error}", program, args.join(" ")))?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| format!("{} {} stderr reader panicked", program, args.join(" ")))?
        .map_err(|error| format!("{} {} stderr: {error}", program, args.join(" ")))?;

    Ok(ProcessOutput {
        status,
        stdout,
        stderr,
    })
}

fn run_settings_command(args: &[&str]) -> Result<String, String> {
    let mut command_args = Vec::with_capacity(args.len() + 1);
    command_args.push("settings");
    command_args.extend_from_slice(args);
    let output =
        run_process_with_timeout("/system/bin/cmd", &command_args, Duration::from_secs(3))?;

    if !output.status.success() {
        return Err(format!(
            "settings {:?} failed status={}",
            args, output.status
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn set_sunlight(enabled: bool) -> Result<(), String> {
    service_control::remember("display.hal", serde_json::json!(true))?;
    if enabled {
        let current = run_settings_command(&["get", "system", "screen_brightness"])?
            .parse::<i32>()
            .map_err(|_| "screen_brightness is not numeric".to_string())?;

        if !vendor_binder::set_display_feature(12, 1, 255) {
            DISPLAY_APPLY_ACK.store(0, Ordering::Release);
            return Err("display HAL sunlight enable failed".into());
        }

        let target = (current + 13).clamp(1, 255);

        if let Err(e) =
            run_settings_command(&["put", "system", "screen_brightness", &target.to_string()])
        {
            let _ = vendor_binder::set_display_feature(12, 0, 255);
            DISPLAY_APPLY_ACK.store(0, Ordering::Release);
            return Err(e);
        }

        mutate_persisted_state(|s| {
            s.sunlight = 1;
            s.brightness_prev = current;
        })
        .inspect_err(|_| {
            DISPLAY_APPLY_ACK.store(0, Ordering::Release);
        })?;
        DISPLAY_SUNLIGHT_STATE.store(1, Ordering::Release);
        DISPLAY_APPLY_ACK.store(1, Ordering::Release);

        Ok(())
    } else {
        if !vendor_binder::set_display_feature(12, 0, 255) {
            DISPLAY_APPLY_ACK.store(0, Ordering::Release);
            return Err("display HAL sunlight disable failed".into());
        }

        let previous = persisted_state()
            .lock()
            .ok()
            .map(|s| s.brightness_prev)
            .unwrap_or(-1);

        if previous >= 1 {
            run_settings_command(&["put", "system", "screen_brightness", &previous.to_string()])?;
        }

        mutate_persisted_state(|s| {
            s.sunlight = 0;
            s.brightness_prev = -1;
        })
        .inspect_err(|_| {
            DISPLAY_APPLY_ACK.store(0, Ordering::Release);
        })?;
        DISPLAY_SUNLIGHT_STATE.store(0, Ordering::Release);
        DISPLAY_APPLY_ACK.store(1, Ordering::Release);

        Ok(())
    }
}

fn record_successful_command(cmd: &str) -> Result<(), String> {
    if let Some(arg) = cmd.strip_prefix("SET charging ") {
        let value = arg
            .trim()
            .parse::<i32>()
            .map_err(|_| "charging persistence parse failed".to_string())?;
        if !valid_charging_profile(value) {
            return Err("charging persistence profile invalid".into());
        }
        return mutate_persisted_state(|state| state.charging = value);
    }

    if let Some(arg) = cmd.strip_prefix("SET charging.bypass ") {
        let value = match arg.trim() {
            "1" => 1,
            "0" => 0,
            _ => return Err("bypass charging persistence state invalid".into()),
        };
        return mutate_persisted_state(|state| state.bypass_charging = value);
    }

    if let Some(arg) = cmd.strip_prefix("SET charging.bypass_threshold ") {
        let value = arg
            .trim()
            .parse::<i32>()
            .map_err(|_| "invalid bypass threshold")?;
        if !bypass_policy::valid_threshold(value) {
            return Err("invalid bypass threshold".into());
        }
        return mutate_persisted_state(|state| state.bypass_threshold = value);
    }

    if let Some(rest) = cmd.strip_prefix("SET cpu.gov ") {
        let mut parts = rest.split_whitespace();
        let policy = parts
            .next()
            .ok_or_else(|| "CPU governor persistence policy missing".to_string())?
            .parse::<i32>()
            .map_err(|_| "CPU governor persistence policy invalid".to_string())?;
        let gov = parts.next().unwrap_or("").to_string();
        if gov.is_empty() {
            return Err("CPU governor persistence value missing".into());
        }
        return mutate_persisted_state(|state| match policy {
            0 => state.cpu0 = gov.clone(),
            2 => state.cpu2 = gov.clone(),
            5 => state.cpu5 = gov.clone(),
            7 => state.cpu7 = gov.clone(),
            _ => {}
        });
    }

    if let Some(arg) = cmd.strip_prefix("SET io.scheduler ") {
        let value = arg.trim().to_string();
        return mutate_persisted_state(|state| state.io = value);
    }

    Ok(())
}

fn drift_status(desired: &str, actual: &str) -> i32 {
    if desired.is_empty() {
        -1
    } else if desired == actual {
        0
    } else {
        1
    }
}

fn cpu_frequency_drift_status(
    target_min: i32,
    target_max: i32,
    live_min: i32,
    live_max: i32,
) -> i32 {
    if target_min <= 0 || target_max <= 0 {
        -1
    } else if target_min == live_min && target_max == live_max {
        0
    } else {
        1
    }
}

fn parse_number_after(line: &str, key: &str) -> Option<f64> {
    let pos = line.find(key)?;
    let tail = &line[pos + key.len()..];

    let token: String = tail
        .chars()
        .skip_while(|c| c.is_whitespace() || *c == '=' || *c == ':')
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();

    token.parse::<f64>().ok()
}

fn scaled_display_density(native_density: i32, width: i32) -> i32 {
    (native_density * width + 610) / 1220
}

fn refresh_display_info() {
    let mut width = -1;
    let mut height = -1;
    let mut density = -1;
    let mut native_density = -1;
    let mut current_x10 = -1;
    let mut max_x10 = -1;

    if let Ok(output) = run_process_with_timeout(
        "/system/bin/cmd",
        &["window", "size"],
        Duration::from_millis(1500),
    ) {
        let text = String::from_utf8_lossy(&output.stdout);
        let mut physical_size = None;
        let mut override_size = None;

        for line in text.lines() {
            let parsed = line
                .split_once(':')
                .and_then(|(_, raw)| raw.trim().split_once('x'))
                .and_then(|(w, h)| Some((w.trim().parse().ok()?, h.trim().parse().ok()?)));

            if line.contains("Override size:") {
                override_size = parsed;
            } else if line.contains("Physical size:") {
                physical_size = parsed;
            }
        }

        if let Some((parsed_width, parsed_height)) = override_size.or(physical_size) {
            width = parsed_width;
            height = parsed_height;
        }
    }

    if let Ok(output) = run_process_with_timeout(
        "/system/bin/cmd",
        &["window", "density"],
        Duration::from_millis(1500),
    ) {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            if line.contains("Override density:") {
                density = line
                    .split(':')
                    .nth(1)
                    .and_then(|raw| raw.trim().parse::<i32>().ok())
                    .unwrap_or(-1);
            } else if line.contains("Physical density:") {
                native_density = line
                    .split(':')
                    .nth(1)
                    .and_then(|raw| raw.trim().parse::<i32>().ok())
                    .unwrap_or(-1);
            }
        }
    }

    if native_density <= 0
        && let Ok(output) = run_process_with_timeout(
            "/system/bin/getprop",
            &["ro.sf.lcd_density"],
            Duration::from_millis(1500),
        )
    {
        native_density = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<i32>()
            .unwrap_or(-1);
    }
    if density <= 0 {
        density = native_density;
    }

    for path in [
        "/sys/devices/virtual/mi_display/disp_feature/disp-DSI-0/dynamic_fps",
        "/sys/class/mi_display/disp-DSI-0/dynamic_fps",
    ] {
        if let Ok(raw) = fs::read_to_string(path)
            && let Ok(rate) = raw.trim().parse::<f64>()
            && (1.0..=1000.0).contains(&rate)
        {
            current_x10 = (rate * 10.0).round() as i32;
            break;
        }
    }

    if let Ok(output) = run_process_with_timeout(
        "/system/bin/dumpsys",
        &["display"],
        Duration::from_millis(1500),
    ) {
        let text = String::from_utf8_lossy(&output.stdout);
        let mut active_id: Option<i32> = None;

        for line in text.lines() {
            if active_id.is_none()
                && let Some(v) = parse_number_after(line, "mActiveModeId")
            {
                active_id = Some(v as i32);
            }

            for key in ["fps=", "refreshRate=", "refreshRate:"] {
                if let Some(v) = parse_number_after(line, key)
                    && (1.0..=1000.0).contains(&v)
                {
                    let x10 = (v * 10.0).round() as i32;

                    if x10 > max_x10 {
                        max_x10 = x10;
                    }

                    if let Some(id) = active_id {
                        let id_a = format!("id={id}");
                        let id_b = format!("modeId={id}");

                        if (line.contains(&id_a) || line.contains(&id_b)) && current_x10 <= 0 {
                            current_x10 = x10;
                        }
                    }
                }
            }
        }
    }

    // Prefer the ROM's configured peak over the panel capability. This keeps
    // the UI truthful when adaptive refresh is currently idling below the
    // user's selected 90/120 Hz setting.
    for (namespace, key) in [
        ("system", "peak_refresh_rate"),
        ("secure", "user_refresh_rate"),
        ("system", "user_refresh_rate"),
    ] {
        if let Ok(output) = run_process_with_timeout(
            "/system/bin/cmd",
            &["settings", "get", namespace, key],
            Duration::from_millis(1500),
        ) && let Ok(rate) = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<f64>()
            && (1.0..=1000.0).contains(&rate)
        {
            max_x10 = (rate * 10.0).round() as i32;
            break;
        }
    }

    DISPLAY_WIDTH.store(width, Ordering::Release);
    DISPLAY_HEIGHT.store(height, Ordering::Release);
    DISPLAY_DENSITY.store(density, Ordering::Release);
    DISPLAY_NATIVE_DENSITY.store(native_density, Ordering::Release);
    DISPLAY_HZ_X10.store(current_x10, Ordering::Release);
    DISPLAY_MAX_HZ_X10.store(max_x10, Ordering::Release);
}

fn apply_display_resolution(
    width: i32,
    height: i32,
    density: i32,
    persist: bool,
) -> Result<(), String> {
    capture_window_originals()?;
    let native = width <= 0 || height <= 0 || (width == 1220 && height == 2712);
    let detected_density = DISPLAY_NATIVE_DENSITY.load(Ordering::Acquire);
    let native_density = if detected_density > 0 {
        detected_density
    } else {
        520
    };
    let target_density = if native {
        native_density
    } else if density > 0 {
        density
    } else {
        scaled_display_density(native_density, width)
    };

    let run_wm = |args: &[&str]| -> Result<(), String> {
        let mut command_args = Vec::with_capacity(args.len() + 1);
        command_args.push("window");
        command_args.extend_from_slice(args);
        let output =
            run_process_with_timeout("/system/bin/cmd", &command_args, Duration::from_secs(3))
                .map_err(|error| format!("wm {}: {error}", args.join(" ")))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "wm {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    };

    if native {
        run_wm(&["size", "reset"])?;
        run_wm(&["density", "reset"])?;
    } else {
        let size = format!("{width}x{height}");
        let density = target_density.to_string();
        run_wm(&["size", &size])?;
        run_wm(&["density", &density])?;
    }

    let expected_width = if native { 1220 } else { width };
    let expected_height = if native { 2712 } else { height };
    let mut verified = false;
    for _ in 0..4 {
        std::thread::sleep(Duration::from_millis(35));
        refresh_display_info();
        verified = DISPLAY_WIDTH.load(Ordering::Acquire) == expected_width
            && DISPLAY_HEIGHT.load(Ordering::Acquire) == expected_height
            && DISPLAY_DENSITY.load(Ordering::Acquire) == target_density;
        if verified {
            break;
        }
    }
    if !verified {
        return Err(format!(
            "display resolution verify failed: requested {expected_width}x{expected_height} {target_density}dpi, live {}x{} {}dpi",
            DISPLAY_WIDTH.load(Ordering::Acquire),
            DISPLAY_HEIGHT.load(Ordering::Acquire),
            DISPLAY_DENSITY.load(Ordering::Acquire),
        ));
    }

    if persist {
        mutate_persisted_state(|state| {
            if native {
                state.display_width = 0;
                state.display_height = 0;
                state.display_density = 0;
            } else {
                state.display_width = width;
                state.display_height = height;
                state.display_density = target_density;
            }
        })
        .inspect_err(|_| {
            DISPLAY_APPLY_ACK.store(0, Ordering::Release);
        })?;
    }

    DISPLAY_APPLY_ACK.store(1, Ordering::Release);
    Ok(())
}

fn screen_is_on() -> Option<bool> {
    for path in [
        "/sys/class/backlight/panel0-backlight/actual_brightness",
        "/sys/class/backlight/panel0-backlight/brightness",
        "/sys/class/leds/lcd-backlight/brightness",
    ] {
        if let Ok(raw) = fs::read_to_string(path)
            && let Ok(value) = raw.trim().parse::<i64>()
        {
            return Some(value > 0);
        }
    }

    let output = run_process_with_timeout(
        "/system/bin/dumpsys",
        &["power"],
        Duration::from_millis(1500),
    )
    .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);

    if text.contains("mWakefulness=Awake") || text.contains("Display Power: state=ON") {
        Some(true)
    } else if text.contains("mWakefulness=Asleep")
        || text.contains("mWakefulness=Dozing")
        || text.contains("Display Power: state=OFF")
    {
        Some(false)
    } else {
        None
    }
}

fn reassert_runtime_state(force_touch: bool) -> Result<(), String> {
    if !service_control::active() {
        return Ok(());
    }
    // NOTE: Refresh rate is NOT touched here. The system's own
    // DisplayModeDirector / PRIORITY_MIUI_REFRESH_RATE / thermal voter
    // handles refresh rate based on the user's choice in Settings.
    // Overriding it from the daemon caused a tug-of-war that made the
    // display oscillate between 60 Hz and 120 Hz.

    let state = persisted_state()
        .lock()
        .ok()
        .map(|guard| guard.clone())
        .unwrap_or_default();

    let mut attempted = 0i32;
    let mut applied = 0i32;

    if matches!(state.touch, 0 | 1) {
        attempted += 1;
        // Serialize with OEM release, then recheck ownership. A wake event
        // queued before the user's selection must not resurrect the old mode.
        let _touch_guard = touch_profile_apply_lock()
            .lock()
            .map_err(|_| "touch profile apply lock poisoned".to_string())?;
        let owned = persisted_state()
            .lock()
            .map_err(|_| "persisted state lock poisoned")?
            .touch
            == state.touch;
        let touch_matches = TOUCH_STATE.load(Ordering::Acquire) == state.touch
            && TOUCH_APPLY_ACK.load(Ordering::Acquire) == 1
            && touch_profile_is_live(state.touch);
        if !owned
            || (!force_touch && touch_matches)
            || apply_touch_profile_locked(state.touch).is_ok()
        {
            applied += 1;
        }
    }

    let ok = attempted > 0 && applied == attempted;

    KEEPALIVE_APPLY_ACK.store(if ok { 1 } else { 0 }, Ordering::Release);

    if ok {
        KEEPALIVE_APPLY_COUNT.fetch_add(1, Ordering::AcqRel);
        Ok(())
    } else {
        Err(format!(
            "runtime persistence verify failed: applied {applied} of {attempted} settings"
        ))
    }
}

fn restore_sunlight(state: &PersistedState) {
    if state.sunlight == 1 {
        if vendor_binder::set_display_feature(12, 1, 255) {
            DISPLAY_SUNLIGHT_STATE.store(1, Ordering::Release);

            // A saved pre-reboot brightness must not be restored later as if
            // it belonged to the new boot/session. Keep the HAL state only.
            let _ = mutate_persisted_state(|s| s.brightness_prev = -1);
        }
    } else if state.sunlight == 0 && vendor_binder::set_display_feature(12, 0, 255) {
        DISPLAY_SUNLIGHT_STATE.store(0, Ordering::Release);
        let _ = mutate_persisted_state(|s| s.brightness_prev = -1);
    }
}

fn restore_persisted_state() {
    if !service_control::active() {
        return;
    }
    restore_cpu_state();

    let mut state = persisted_state()
        .lock()
        .ok()
        .map(|s| s.clone())
        .unwrap_or_default();

    if state.sconfig_override >= 0 {
        if let Err(error) = sconfig::set(state.sconfig_override) {
            eprintln!("PERIDOT_SCONFIG_RESTORE_FAIL {error}");
        }
    }

    let legacy_state = state.gpu_profile_cpu_isolated != 1;
    let reset_legacy_cpu = migrate_legacy_gpu_profile_cpu_state(&mut state);
    if legacy_state {
        if let Ok(mut guard) = persisted_state().lock() {
            *guard = state.clone();
        }
        let _ = save_persisted_state(&state);
    }
    if reset_legacy_cpu {
        // Undo only the exact CPU signature written by an older GPU profile.
        // Custom CPU settings that do not match that signature are preserved.
        restore_vendor_cpu_defaults();
    }

    PERSISTENCE_LOADED.store(1, Ordering::Release);

    if state.bypass_charging != 1
        && valid_charging_profile(state.charging)
        && CHARGING_BOOT_RESTORE_DONE
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    {
        let _ = apply_charging_profile(state.charging);
    }

    if state.bypass_charging == 1 {
        maintain_bypass_policy();
        wake_bypass_monitor();
    }

    if matches!(state.touch, 0 | 1) {
        if let Ok(_guard) = touch_profile_apply_lock().lock() {
            let owned = persisted_state()
                .lock()
                .is_ok_and(|s| s.touch == state.touch);
            if owned {
                let _ = apply_touch_profile_locked(state.touch);
            }
        }
    }

    if let Ok(_guard) = display_mode_apply_lock().lock() {
        if (0..=2).contains(&state.display_color)
            && persisted_state()
                .lock()
                .is_ok_and(|s| s.display_color == state.display_color)
        {
            let _ = apply_display_color_locked(state.display_color);
        }
        if (1..=3).contains(&state.display_temp)
            && persisted_state()
                .lock()
                .is_ok_and(|s| s.display_temp == state.display_temp)
        {
            let _ = apply_display_temp_locked(state.display_temp);
        }
    }

    restore_sunlight(&state);
    // A pre-reboot brightness value belongs to the previous Android session.
    // Keep the saved hardware intent, but never resurrect that stale value
    // after the profile restore below replaces the in-memory state.
    state.brightness_prev = -1;

    if matches!(state.silky, 0 | 1) {
        let _ = set_display_toggle(57, state.silky == 1, &DISPLAY_SILKY_STATE);
    }

    if matches!(state.video, 0 | 1) {
        let _ = set_display_toggle(27, state.video == 1, &DISPLAY_VIDEO_STATE);
    }

    if matches!(state.dolby, 0 | 1) {
        let _ = set_display_toggle(44, state.dolby == 1, &DISPLAY_DOLBY_STATE);
    }

    let display_guard = display_mode_apply_lock().lock();
    if display_guard.is_ok()
        && state.display_color == 0
        && persisted_state().lock().is_ok_and(|s| s.display_color == 0)
    {
        if matches!(state.expert_gamut, 1..=3) {
            let _ = vendor_binder::set_display_feature(26, state.expert_gamut, 0);
        }

        for channel in 1..=8 {
            let value = state.expert[(channel - 1) as usize];

            if let Some((min, max)) = expert_value_range(channel)
                && (min..=max).contains(&value)
            {
                let _ = vendor_binder::set_display_feature(26, value, channel);
            }
        }
    }

    drop(display_guard);

    if (0..=3).contains(&state.perf) {
        // GPU profiles are isolated from CPU controls. Preserve the exact
        // persisted selection while restoring the requested Mali state.
        let persisted = state.clone();
        let _ = apply_performance_profile(state.perf);
        // Restore only profile-owned preferences. Replacing the whole old
        // snapshot here could undo a concurrent touch/colour OEM selection.
        let _ = mutate_persisted_state(|current| {
            if current.perf == persisted.perf {
                current.gpu_uncap = persisted.gpu_uncap;
                current.gpu_min_freq_mhz = persisted.gpu_min_freq_mhz;
                current.gpu_max_freq_mhz = persisted.gpu_max_freq_mhz;
                current.gpu_ged_boost = persisted.gpu_ged_boost;
                current.gpu = persisted.gpu.clone();
                current.gpu_governor = persisted.gpu_governor.clone();
                current.gpu_power_policy = persisted.gpu_power_policy.clone();
                current.gpu_profile_cpu_isolated = persisted.gpu_profile_cpu_isolated;
            }
        });
        state = persisted;
    }

    if !state.cpu0.is_empty() {
        let _ = set_cpu_governor(0, &state.cpu0);
    }

    if !state.cpu2.is_empty() {
        let _ = set_cpu_governor(2, &state.cpu2);
    }
    if !state.cpu5.is_empty() {
        let _ = set_cpu_governor(5, &state.cpu5);
    }

    if !state.cpu7.is_empty() {
        let _ = set_cpu_governor(7, &state.cpu7);
    }

    let target_gpu = if !state.gpu_governor.is_empty() {
        &state.gpu_governor
    } else if !state.gpu.is_empty() {
        &state.gpu
    } else {
        ""
    };
    if !target_gpu.is_empty() {
        let _ = set_gpu_governor(target_gpu);
    }

    if !state.io.is_empty() {
        let _ = set_io_scheduler(&state.io);
    }

    if state.zram_swappiness >= 0 && zram_get_swappiness() != state.zram_swappiness {
        let _ = set_zram_swappiness(state.zram_swappiness);
    }

    if !state.zram_algorithm.is_empty() && zram_get_algorithm() != state.zram_algorithm {
        let _ = set_zram_algorithm(&state.zram_algorithm);
    }

    // Changing either compression or disksize resets the live swap device.
    // Repeated late-boot restores must therefore be idempotent: a matching
    // saved ZRAM configuration is left completely untouched.
    if state.zram_size_mb >= 0 && zram_get_disksize_mb() != state.zram_size_mb {
        let _ = set_zram_size(state.zram_size_mb);
    }

    let has_persisted_cpu_ranges = persisted_cpu_ranges_active(&state);
    let cpu_mode_ready = if has_persisted_cpu_ranges {
        ensure_cpu_unrestricted_mode().is_ok()
    } else {
        let _ = restore_cpu_thermal_mode();
        true
    };

    if cpu_mode_ready {
        for policy in [0, 2, 5, 7] {
            if let Some((min, max)) = persisted_cpu_range(&state, policy) {
                let _ = apply_cluster_freq_controls(policy, min, max);
            }
        }
    } else {
        CPU_FREQ_WRITE_ACK.store(0, Ordering::Release);
    }

    if state.gpu_uncap == 1 {
        let _ = set_gpu_uncap(true);
    } else {
        if !state.gpu_power_policy.is_empty() {
            let _ = set_gpu_power_policy(&state.gpu_power_policy);
        }
        if state.gpu_min_freq_mhz > 0 {
            let _ = set_gpu_min_freq(state.gpu_min_freq_mhz);
        }
        if state.gpu_max_freq_mhz > 0 {
            let _ = set_gpu_max_freq(state.gpu_max_freq_mhz);
        }
        if !state.gpu_governor.is_empty() {
            let _ = set_gpu_governor(&state.gpu_governor);
        }
        if state.gpu_ged_boost >= 0 {
            let _ = set_gpu_ged_boost(state.gpu_ged_boost == 1);
        }
    }

    // Framework CLI clients can be delayed by a vendor system_server during
    // early boot. Restore the direct kernel/HAL controls first so a blocked
    // display transaction cannot prevent CPU, GPU, touch, charging, UFS, or
    // ZRAM persistence from being applied.
    if state.display_width >= 0 && state.display_height >= 0 {
        let _ = apply_display_resolution(
            state.display_width,
            state.display_height,
            state.display_density,
            false,
        );
    }
}

fn late_boot_restore_loop() {
    // Framework and vendor power services can publish defaults after a root
    // module starts. Repeat controls that those services may overwrite across
    // that settling window, including the selected v1.18.0 touch timing
    // profile. Charging is intentionally restored only once: subsequent
    // fixed-profile reapplication is driven by actual charger reconnect
    // events, never by this timer.
    for (attempt, delay_seconds) in [2u64, 4, 8].into_iter().enumerate() {
        std::thread::sleep(Duration::from_secs(delay_seconds));
        let _gate = service_control::GATE.read().expect("service gate poisoned");
        if !service_control::active() {
            continue;
        }
        restore_persisted_state();
        let runtime_result = reassert_runtime_state(true);
        reassert_persisted_governors();
        eprintln!(
            "PERIDOT_BOOT_RESTORE attempt={} runtime_ack={} runtime_ok={}",
            attempt + 1,
            KEEPALIVE_APPLY_ACK.load(Ordering::Acquire),
            i32::from(runtime_result.is_ok()),
        );
    }
}

fn reassert_persisted_governors() {
    if !service_control::active() {
        return;
    }
    let Ok(_profile_guard) = gpu_profile_apply_lock().try_lock() else {
        return;
    };

    let state = persisted_state()
        .lock()
        .ok()
        .map(|s| s.clone())
        .unwrap_or_default();

    if state.cpu_manual == 1 {
        let desired_mask = (state.cpu_online_mask | 0x01) & 0xFF;
        if live_cpu_mask() != desired_mask {
            let _ = set_core_ctl_enabled(false).and_then(|_| apply_saved_cpu_mask(desired_mask));
        }
    }

    for (policy, desired) in [
        (0, state.cpu0.as_str()),
        (2, state.cpu2.as_str()),
        (5, state.cpu5.as_str()),
        (7, state.cpu7.as_str()),
    ] {
        if desired.is_empty() {
            continue;
        }

        let actual = policy_governor(policy);

        if actual != desired {
            let _ = set_cpu_governor(policy, desired);
        }
    }

    // Observe requested versus effective GPU state without rewriting limits.
    // This prevents an accepted clamped request from becoming a write loop.
    PERFORMANCE_PROFILE_VERIFIED.store(1, Ordering::Release);
    PERFORMANCE_PROFILE_OK.store(
        if gpu_profile_verified(state.perf) {
            1
        } else {
            0
        },
        Ordering::Release,
    );

    if !state.io.is_empty() && io_scheduler() != state.io {
        let _ = set_io_scheduler(&state.io);
    }

    let desired_zram_alg = if !state.zram_algorithm.is_empty() {
        state.zram_algorithm.as_str()
    } else {
        "lz4"
    };
    if !state.zram_algorithm.is_empty() && zram_get_algorithm() != desired_zram_alg {
        let _ = set_zram_algorithm(desired_zram_alg);
    }

    if state.zram_swappiness >= 0 && zram_get_swappiness() != state.zram_swappiness {
        let _ = set_zram_swappiness(state.zram_swappiness);
    }

    if state.zram_size_mb > 0 && zram_get_disksize_mb() != state.zram_size_mb {
        let _ = set_zram_size(state.zram_size_mb);
    }
}

fn cpu_frequency_guard() {
    // Frequency requests are submitted on user action and bounded boot restore.
    // No periodic writes: a lower effective limit may belong to the driver/OEM.
}

fn gaming_dynamic_guard() {
    // msm-adreno-tz owns load-based DVFS. No GED worker on Qualcomm.
}

#[cfg(target_os = "android")]
fn observe_bypass_telemetry() {
    maintain_bypass_policy();
    let enabled = persisted_state()
        .lock()
        .ok()
        .is_some_and(|state| state.bypass_charging == 1);
    if !enabled {
        return;
    }
    if read_trimmed(Path::new("/sys/class/power_supply/usb/online")).as_deref() != Ok("1") {
        return;
    }
    // Keep the kernel's measured confirmation independent of UI readers.
    // This is observation only: no SET command, sysfs write, or reapplication.
    let _ = read_trimmed(Path::new(
        "/sys/class/power_supply/battery/bypass_charging_active",
    ));
}

#[cfg(target_os = "android")]
fn bypass_monitor_loop() {
    let mut interval = Duration::from_secs(60);
    loop {
        let dirty = BYPASS_MONITOR_DIRTY
            .lock()
            .expect("bypass monitor poisoned");
        let (mut dirty, _) = BYPASS_MONITOR_WAKE
            .wait_timeout_while(dirty, interval, |dirty| !*dirty)
            .expect("bypass monitor poisoned");
        *dirty = false;
        drop(dirty);
        let _gate = service_control::GATE.read().expect("service gate poisoned");
        let enabled = persisted_state()
            .lock()
            .ok()
            .is_some_and(|state| state.bypass_charging == 1);
        interval = if enabled && usb("online") == "1" {
            Duration::from_secs(1)
        } else {
            Duration::from_secs(60)
        };
        if service_control::active() && enabled {
            // This independent reader cannot be delayed by unrelated display,
            // touch, or GPU restoration work. Reads never rewrite the request.
            observe_bypass_telemetry();
        }
    }
}

fn maintenance_loop() {
    let mut last_guard = Instant::now()
        .checked_sub(Duration::from_secs(10))
        .unwrap_or_else(Instant::now);

    let mut last_screen_check = Instant::now()
        .checked_sub(Duration::from_secs(5))
        .unwrap_or_else(Instant::now);

    let mut last_keepalive = Instant::now()
        .checked_sub(Duration::from_secs(65))
        .unwrap_or_else(Instant::now);

    let mut last_display_refresh = Instant::now()
        .checked_sub(Duration::from_secs(12))
        .unwrap_or_else(Instant::now);

    let mut screen_was_on: Option<bool> = None;

    loop {
        let gate = service_control::GATE.read().expect("service gate poisoned");
        if !service_control::active() {
            drop(gate);
            service_control::wait_until_active();
            continue;
        }

        if last_guard.elapsed() >= Duration::from_millis(500) {
            reassert_persisted_governors();
            last_guard = Instant::now();
        }

        if last_display_refresh.elapsed() >= Duration::from_secs(10) {
            refresh_display_info();
            last_display_refresh = Instant::now();
        }

        if last_screen_check.elapsed() >= Duration::from_secs(3) {
            if let Some(screen_on) = screen_is_on() {
                let woke = screen_was_on == Some(false) && screen_on;

                if woke {
                    std::thread::sleep(Duration::from_millis(300));
                    let _ = reassert_runtime_state(true);
                    last_keepalive = Instant::now();
                } else if screen_on && last_keepalive.elapsed() >= Duration::from_secs(60) {
                    let _ = reassert_runtime_state(false);
                    last_keepalive = Instant::now();
                }

                screen_was_on = Some(screen_on);
            }

            last_screen_check = Instant::now();
        }

        drop(gate);
        std::thread::sleep(Duration::from_millis(1500));
    }
}

#[cfg(target_os = "android")]
fn charging_power_supply_event_loop() {
    loop {
        let fd = unsafe {
            socket(
                AF_NETLINK,
                SOCK_DGRAM | SOCK_CLOEXEC,
                NETLINK_KOBJECT_UEVENT,
            )
        };
        if fd < 0 {
            eprintln!(
                "PERIDOT_CHARGING_UEVENT_SOCKET_FAIL {}",
                std::io::Error::last_os_error()
            );
            std::thread::sleep(Duration::from_secs(2));
            continue;
        }

        let address = SockAddrNl {
            nl_family: AF_NETLINK as u16,
            nl_pad: 0,
            nl_pid: 0,
            nl_groups: 1,
        };
        let bound = unsafe {
            bind(
                fd,
                &address as *const _ as *const c_void,
                std::mem::size_of::<SockAddrNl>() as u32,
            )
        } == 0;
        if !bound {
            eprintln!(
                "PERIDOT_CHARGING_UEVENT_BIND_FAIL {}",
                std::io::Error::last_os_error()
            );
            unsafe { close(fd) };
            std::thread::sleep(Duration::from_secs(2));
            continue;
        }

        let mut message = [0u8; 4096];
        loop {
            let count = unsafe { recv(fd, message.as_mut_ptr().cast(), message.len(), 0) };
            if count <= 0 {
                break;
            }

            let payload = &message[..count as usize];
            let is_power_supply = payload
                .split(|byte| *byte == 0)
                .any(|field| field == b"SUBSYSTEM=power_supply");
            if !is_power_supply {
                continue;
            }

            let _gate = service_control::GATE.read().expect("service gate poisoned");
            if !service_control::active() {
                continue;
            }
            let (profile, bypass_charging) = persisted_state()
                .lock()
                .ok()
                .map(|state| (state.charging, state.bypass_charging))
                .unwrap_or((0, 0));
            if let Err(error) = maintain_charging_state(profile, bypass_charging) {
                eprintln!("PERIDOT_CHARGING_UEVENT_APPLY_FAIL {error}");
            }
        }

        unsafe { close(fd) };
        std::thread::sleep(Duration::from_millis(500));
    }
}

pub fn start_background_services() {
    #[cfg(target_os = "android")]
    std::thread::spawn(bypass_monitor_loop);
    if std::env::var("PERIDOT_REVERSE_IPC").as_deref() == Ok("1") {
        std::thread::spawn(reverse_ipc_loop);
    }
    if std::env::var("PERIDOT_LOOPBACK_IPC").as_deref() == Ok("1") {
        std::thread::spawn(loopback_ipc_loop);
    }
    let _ = persisted_state();
    subsystem_clocks::start();
    app_runtime::start();
    // The v1.18.0 1000 Hz profile waits for this worker to attach to the
    // TouchFeature event stream, so it must be ready before state restoration.
    touch_resampler::start_background();

    // Start every independent guard before invoking Android framework CLI
    // clients. On some vendor ROMs `wm`/`cmd` can wait indefinitely during
    // early boot; IPC and direct kernel/HAL persistence must remain available
    // even when that framework query is unhealthy.
    std::thread::spawn(late_boot_restore_loop);
    std::thread::spawn(maintenance_loop);
    std::thread::spawn(cpu_frequency_guard);
    std::thread::spawn(gaming_dynamic_guard);
    #[cfg(target_os = "android")]
    std::thread::spawn(charging_power_supply_event_loop);
    std::thread::spawn(|| {
        let _gate = service_control::GATE.read().expect("service gate poisoned");
        if service_control::release_pending() && !service_control::enabled() {
            if restore_released_controls().is_ok() {
                let _ = service_control::set_release_pending(false);
            }
        }
        restore_persisted_state();
    });
    std::thread::spawn(refresh_display_info);
}

fn reverse_ipc_loop() {
    loop {
        match connect_stream(REVERSE_SOCKET_NAME) {
            Ok(stream) => serve_client_with_transport(stream, 2),
            Err(_) => std::thread::sleep(Duration::from_millis(250)),
        }
    }
}

fn loopback_ipc_loop() {
    loop {
        let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, LOOPBACK_PORT)) {
            Ok(listener) => listener,
            Err(error) => {
                // A previous daemon can leave the fixed local port transiently
                // unavailable. Keep the authenticated Unix transports alive
                // and recover this compatibility listener without a reboot.
                eprintln!("PERIDOT_ESSENTIALD_LOOPBACK_BIND_FAIL {error}");
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
        };
        for incoming in listener.incoming() {
            match incoming {
                Ok(stream) => {
                    let _ = stream.set_nodelay(true);
                    std::thread::spawn(move || serve_loopback_client(stream));
                }
                Err(error) => {
                    eprintln!("PERIDOT_ESSENTIALD_LOOPBACK_ACCEPT_FAIL {error}");
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }
}

// ZRAM & MEMORY TUNING HELPERS

fn zram_get_disksize_mb() -> i32 {
    if let Ok(raw) = fs::read_to_string("/sys/block/zram0/disksize")
        && let Ok(bytes) = raw.trim().parse::<u64>()
    {
        return (bytes / (1024 * 1024)) as i32;
    }
    0
}

fn zram_get_swappiness() -> i32 {
    if let Ok(raw) = fs::read_to_string("/proc/sys/vm/swappiness")
        && let Ok(v) = raw.trim().parse::<i32>()
    {
        return v;
    }
    100
}

fn zram_get_algorithm() -> String {
    if let Ok(raw) = fs::read_to_string("/sys/block/zram0/comp_algorithm") {
        for token in raw.split_whitespace() {
            if token.starts_with('[') && token.ends_with(']') {
                return token[1..token.len() - 1].to_string();
            }
        }
    }
    "lz4".to_string()
}

struct ZramMmStat {
    orig_data_mb: i32,
    compr_data_mb: i32,
    mem_used_mb: i32,
}

fn zram_get_mm_stat() -> ZramMmStat {
    if let Ok(raw) = fs::read_to_string("/sys/block/zram0/mm_stat") {
        let parts: Vec<&str> = raw.split_whitespace().collect();
        if parts.len() >= 3 {
            let orig = parts[0].parse::<u64>().unwrap_or(0) / (1024 * 1024);
            let compr = parts[1].parse::<u64>().unwrap_or(0) / (1024 * 1024);
            let used = parts[2].parse::<u64>().unwrap_or(0) / (1024 * 1024);
            return ZramMmStat {
                orig_data_mb: orig as i32,
                compr_data_mb: compr as i32,
                mem_used_mb: used as i32,
            };
        }
    }
    ZramMmStat {
        orig_data_mb: 0,
        compr_data_mb: 0,
        mem_used_mb: 0,
    }
}

fn set_zram_size(size_mb: i32) -> Result<(), String> {
    capture_zram_originals()?;
    if !(0..=32768).contains(&size_mb) {
        return Err("invalid ZRAM size".to_string());
    }

    // 1. Drop pagecache to relieve RAM before swapoff
    let _ = service_control::write("/proc/sys/vm/drop_caches", "3");

    // 2. swapoff /dev/block/zram0
    let _ = ProcessCommand::new("/system/bin/swapoff")
        .arg("/dev/block/zram0")
        .output();

    // 3. Reset zram
    service_control::write("/sys/block/zram0/reset", "1")
        .map_err(|e| format!("zram reset failed: {e}"))?;

    // 3.5. Reapply configured compression algorithm before setting disksize!
    let target_alg = persisted_state()
        .lock()
        .ok()
        .map(|s| {
            if !s.zram_algorithm.is_empty() {
                s.zram_algorithm.clone()
            } else {
                "lz4".to_string()
            }
        })
        .unwrap_or_else(|| "lz4".to_string());
    service_control::write("/sys/block/zram0/comp_algorithm", &target_alg)
        .map_err(|e| format!("zram compression restore failed: {e}"))?;
    if zram_get_algorithm() != target_alg {
        return Err(format!(
            "zram compression verify failed: requested {target_alg}, live {}",
            zram_get_algorithm()
        ));
    }

    if size_mb == 0 {
        mutate_persisted_state(|state| {
            state.zram_size_mb = 0;
        })?;
        return Ok(());
    }

    // 4. Write new disksize
    let bytes = (size_mb as u64) * 1024 * 1024;
    service_control::write("/sys/block/zram0/disksize", bytes.to_string())
        .map_err(|e| format!("zram disksize failed: {e}"))?;
    if zram_get_disksize_mb() != size_mb {
        return Err(format!(
            "zram disksize verify failed: requested {size_mb} MiB, live {} MiB",
            zram_get_disksize_mb()
        ));
    }

    // 5. mkswap
    let mkswap_res = ProcessCommand::new("/system/bin/mkswap")
        .arg("/dev/block/zram0")
        .output()
        .map_err(|e| format!("mkswap failed: {e}"))?;

    if !mkswap_res.status.success() {
        return Err("mkswap failed".to_string());
    }

    // 6. swapon
    let swapon_res = ProcessCommand::new("/system/bin/swapon")
        .args(["-p", "32758", "/dev/block/zram0"])
        .output()
        .map_err(|e| format!("swapon failed to start: {e}"))?;

    if !swapon_res.status.success() {
        let fallback = ProcessCommand::new("/system/bin/swapon")
            .arg("/dev/block/zram0")
            .output()
            .map_err(|e| format!("swapon fallback failed to start: {e}"))?;
        if !fallback.status.success() {
            return Err(format!(
                "swapon failed: {}",
                String::from_utf8_lossy(&fallback.stderr).trim()
            ));
        }
    }

    mutate_persisted_state(|state| {
        state.zram_size_mb = size_mb;
    })?;

    Ok(())
}

fn set_zram_algorithm(alg: &str) -> Result<(), String> {
    capture_zram_originals()?;
    let alg = alg.trim();
    if !matches!(alg, "lz4" | "zstd" | "lzo-rle" | "lzo") {
        return Err("unsupported compression algorithm".to_string());
    }

    let current_size_mb = zram_get_disksize_mb();

    let _ = service_control::write("/proc/sys/vm/drop_caches", "3");
    let _ = ProcessCommand::new("/system/bin/swapoff")
        .arg("/dev/block/zram0")
        .output();

    service_control::write("/sys/block/zram0/reset", "1")
        .map_err(|e| format!("zram reset failed: {e}"))?;

    service_control::write("/sys/block/zram0/comp_algorithm", alg)
        .map_err(|e| format!("comp_algorithm failed: {e}"))?;
    if zram_get_algorithm() != alg {
        return Err(format!(
            "comp_algorithm verify failed: requested {alg}, live {}",
            zram_get_algorithm()
        ));
    }

    if current_size_mb > 0 {
        let bytes = (current_size_mb as u64) * 1024 * 1024;
        service_control::write("/sys/block/zram0/disksize", bytes.to_string())
            .map_err(|e| format!("zram disksize restore failed: {e}"))?;
        if zram_get_disksize_mb() != current_size_mb {
            return Err(format!(
                "zram disksize restore verify failed: requested {current_size_mb} MiB, live {} MiB",
                zram_get_disksize_mb()
            ));
        }
        let mkswap = ProcessCommand::new("/system/bin/mkswap")
            .arg("/dev/block/zram0")
            .output()
            .map_err(|e| format!("mkswap failed to start: {e}"))?;
        if !mkswap.status.success() {
            return Err(format!(
                "mkswap failed: {}",
                String::from_utf8_lossy(&mkswap.stderr).trim()
            ));
        }
        let swapon = ProcessCommand::new("/system/bin/swapon")
            .args(["-p", "32758", "/dev/block/zram0"])
            .output()
            .map_err(|e| format!("swapon failed to start: {e}"))?;
        if !swapon.status.success() {
            return Err(format!(
                "swapon failed: {}",
                String::from_utf8_lossy(&swapon.stderr).trim()
            ));
        }
    }

    mutate_persisted_state(|state| {
        state.zram_algorithm = alg.to_string();
    })?;

    Ok(())
}

fn set_zram_swappiness(val: i32) -> Result<(), String> {
    if !(0..=200).contains(&val) {
        return Err("invalid swappiness value (0-200)".to_string());
    }

    service_control::write("/proc/sys/vm/swappiness", val.to_string())
        .map_err(|e| format!("swappiness write failed: {e}"))?;
    if zram_get_swappiness() != val {
        return Err(format!(
            "swappiness verify failed: requested {val}, live {}",
            zram_get_swappiness()
        ));
    }

    mutate_persisted_state(|state| {
        state.zram_swappiness = val;
    })?;

    Ok(())
}

fn compact_zram() -> Result<(), String> {
    service_control::write("/sys/block/zram0/compact", "1")
        .map_err(|e| format!("compact failed: {e}"))?;
    Ok(())
}

// MALI GPU & MEDIATEK GED HELPERS
// Ponytail: SM8635 (POCO F6 / Peridot) is a Qualcomm/Adreno device.
// These stubs exist only because the upstream NeonEssential also supported
// MediaTek devices. All callers handle Err gracefully; the stubs are dead
// code on Peridot hardware. Remove when upstream drops MediaTek support.

fn gpu_read_file(primary: &str, fallback: &str) -> Option<String> {
    fs::read_to_string(primary)
        .or_else(|_| fs::read_to_string(fallback))
        .ok()
}

fn gpu_write_file(primary: &str, fallback: &str, value: &str) -> Result<(), String> {
    let mut wrote = false;
    let mut errors = Vec::new();

    for path in [primary, fallback] {
        if !Path::new(path).exists() {
            continue;
        }
        match service_control::write(path, value) {
            Ok(()) => wrote = true,
            Err(error) => errors.push(format!("{path}: {error}")),
        }
    }

    if wrote {
        Ok(())
    } else if errors.is_empty() {
        Err(format!("GPU control node missing: {primary} or {fallback}"))
    } else {
        Err(format!("GPU control write failed: {}", errors.join("; ")))
    }
}

fn gpu_read_raw(path: &str) -> Option<String> {
    use std::io::Read;
    let mut f = fs::File::open(path).ok()?;
    let mut buf = [0u8; 256];
    let n = f.read(&mut buf).ok()?;
    if n == 0 {
        return None;
    }
    String::from_utf8(buf[..n].to_vec()).ok()
}

fn gpu_get_loading() -> i32 {
    if let Some(raw) = peridot::root_read("gpu_busy_percentage") {
        if let Ok(value) = raw.parse::<i32>() {
            return value.clamp(0, 100);
        }
    }
    let raw = peridot::root_read("gpubusy").unwrap_or_default();
    let v: Vec<i64> = raw
        .split_whitespace()
        .filter_map(|x| x.parse().ok())
        .collect();
    if v.len() >= 2 && v[1] > 0 {
        ((v[0] * 100 / v[1]).clamp(0, 100)) as i32
    } else {
        -1
    }
}

fn gpu_get_cur_opp_index() -> i32 {
    gpu_read_raw("/sys/kernel/ged/hal/current_freqency")
        .and_then(|value| {
            value
                .split_whitespace()
                .next()
                .and_then(|part| part.parse::<i32>().ok())
        })
        .unwrap_or(40)
}

fn gpu_get_lowest_opp_index() -> i32 {
    gpu_read_raw("/sys/kernel/ged/hal/total_gpu_freq_level_count")
        .and_then(|value| {
            value
                .split_whitespace()
                .next()
                .and_then(|part| part.parse::<i32>().ok())
        })
        .map(|count| count.saturating_sub(1))
        .or_else(|| {
            gpu_read_raw("/sys/class/devfreq/13000000.mali/available_frequencies")
                .map(|value| value.split_whitespace().count().saturating_sub(1) as i32)
        })
        .unwrap_or(40)
        .clamp(0, 255)
}

fn gaming_dynamic_target_opp(load: i32, current_opp: i32, lowest_opp: i32) -> i32 {
    let load = load.clamp(0, 100);
    let current_opp = current_opp.clamp(0, lowest_opp);

    if load >= 85 {
        0
    } else if load >= 70 {
        current_opp.saturating_sub(8)
    } else if load >= 55 {
        current_opp.saturating_sub(4)
    } else if load >= 40 {
        current_opp.saturating_sub(2)
    } else if load <= 10 {
        lowest_opp
    } else if load <= 20 {
        (current_opp + 8).min(lowest_opp)
    } else if load <= 30 {
        (current_opp + 4).min(lowest_opp)
    } else {
        current_opp
    }
}

fn parse_ged_current_frequency_mhz(raw: &str) -> Option<i32> {
    let value = raw
        .split_whitespace()
        .filter_map(|part| part.parse::<i64>().ok())
        .next_back()?;

    if value <= 0 {
        None
    } else if value > 10_000_000 {
        Some((value / 1_000_000) as i32)
    } else if value > 1_300 {
        Some((value / 1_000) as i32)
    } else {
        Some(value as i32)
    }
}

fn gpu_get_cur_freq_mhz() -> i32 {
    peridot::read("cur_freq")
        .or_else(|| peridot::root_read("gpuclk"))
        .and_then(|x| x.parse::<i64>().ok())
        .map(|x| (x / 1_000_000) as i32)
        .unwrap_or(0)
}

fn gpu_get_min_freq_mhz() -> i32 {
    peridot::mhz("min_freq").unwrap_or(-1)
}

fn gpu_get_max_freq_mhz() -> i32 {
    peridot::mhz("max_freq").unwrap_or(-1)
}

fn gpu_get_governor() -> String {
    gpu_governor()
}

fn gpu_get_ged_master_flag() -> i32 {
    -1
}

fn gpu_flag(path: &str) -> i32 {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(-1)
}

fn gpu_get_ged_boost() -> i32 {
    -1
}

fn gpu_boost_pipeline_matches(enabled: bool) -> bool {
    !enabled
}

fn gpu_get_thermal_state() -> i32 {
    peridot::root_read("thermal_pwrlevel")
        .and_then(|v| v.parse().ok())
        .unwrap_or(-1)
}

fn gpu_get_dvfs_enabled() -> i32 {
    1
}

fn gpu_get_uncap_active() -> i32 {
    i32::from(peridot::profile_verified(3))
}

fn gpu_profile_verified(profile: i32) -> bool {
    peridot::profile_verified(profile)
}

fn gpu_profile_configured(profile: i32) -> bool {
    peridot::profile_verified(profile)
}

fn set_gpu_min_freq(mhz: i32) -> Result<(), String> {
    let max = peridot::mhz("max_freq").ok_or("GPU maximum unavailable")?;
    peridot::set_range(mhz, max)?;
    mutate_persisted_state(|state| {
        state.gpu_min_freq_mhz = mhz;
        state.gpu_uncap = 0;
    })
}

fn set_gpu_max_freq(mhz: i32) -> Result<(), String> {
    let min = peridot::mhz("min_freq").ok_or("GPU minimum unavailable")?;
    peridot::set_range(min, mhz)?;
    mutate_persisted_state(|state| {
        state.gpu_max_freq_mhz = mhz;
        state.gpu_uncap = 0;
    })
}

fn set_gpu_ged_boost(enable: bool) -> Result<(), String> {
    let _ = enable;
    Err("GED is MediaTek-only; Peridot uses KGSL".into())
}

fn set_gpu_uncap(enable: bool) -> Result<(), String> {
    apply_performance_profile(if enable { 3 } else { 0 })
}

fn gpu_get_power_policy() -> String {
    "kernel-managed".into()
}

fn set_gpu_power_policy(policy: &str) -> Result<(), String> {
    let _ = policy;
    Err("Mali power policy is unavailable on KGSL".into())
}

fn snapshot_persistence_fields() -> Vec<String> {
    let state = persisted_state()
        .lock()
        .ok()
        .map(|s| s.clone())
        .unwrap_or_default();

    let cpu0_drift = drift_status(&state.cpu0, &policy_governor(0));
    let cpu2_drift = drift_status(&state.cpu2, &policy_governor(2));
    let cpu5_drift = drift_status(&state.cpu5, &policy_governor(5));
    let cpu7_drift = drift_status(&state.cpu7, &policy_governor(7));
    let desired_gpu_governor = if !state.gpu_governor.is_empty() {
        state.gpu_governor.as_str()
    } else {
        state.gpu.as_str()
    };
    let gpu_drift = drift_status(desired_gpu_governor, &gpu_governor());
    let io_drift = drift_status(&state.io, &io_scheduler());
    let cpu_live_min0 = get_cpu_cluster_live_min_freq(0);
    let cpu_live_max0 = get_cpu_cluster_live_max_freq(0);
    let cpu_live_min2 = get_cpu_cluster_live_min_freq(2);
    let cpu_live_min5 = get_cpu_cluster_live_min_freq(5);
    let cpu_live_max2 = get_cpu_cluster_live_max_freq(2);
    let cpu_live_max5 = get_cpu_cluster_live_max_freq(5);
    let cpu_live_min7 = get_cpu_cluster_live_min_freq(7);
    let cpu_live_max7 = get_cpu_cluster_live_max_freq(7);
    let cpu_freq_drift0 = cpu_frequency_drift_status(
        state.cpu_min_freq0,
        state.cpu_max_freq0,
        cpu_live_min0,
        cpu_live_max0,
    );
    let cpu_freq_drift2 = cpu_frequency_drift_status(
        state.cpu_min_freq2,
        state.cpu_max_freq2,
        cpu_live_min2,
        cpu_live_max2,
    );
    let cpu_freq_drift5 = cpu_frequency_drift_status(
        state.cpu_min_freq5,
        state.cpu_max_freq5,
        cpu_live_min5,
        cpu_live_max5,
    );
    let cpu_freq_drift7 = cpu_frequency_drift_status(
        state.cpu_min_freq7,
        state.cpu_max_freq7,
        cpu_live_min7,
        cpu_live_max7,
    );
    let cpu_thermal_mode = read_mi_thermal_config_mode().unwrap_or(-1);
    let cpu_thermal_unrestricted = i32::from(
        persisted_cpu_ranges_active(&state) && cpu_thermal_mode == MI_THERMAL_NO_LIMITS_MODE,
    );

    let zram_stat = zram_get_mm_stat();
    let zram_disk_mb = zram_get_disksize_mb();
    let zram_swappiness = zram_get_swappiness();
    let zram_alg = zram_get_algorithm();

    vec![
        "phase=16".to_string(),
        format!("expert_gamut={}", state.expert_gamut),
        format!("expert_1={}", state.expert[0]),
        format!("expert_2={}", state.expert[1]),
        format!("expert_3={}", state.expert[2]),
        format!("expert_4={}", state.expert[3]),
        format!("expert_5={}", state.expert[4]),
        format!("expert_6={}", state.expert[5]),
        format!("expert_7={}", state.expert[6]),
        format!("expert_8={}", state.expert[7]),
        format!(
            "perf_supported={}",
            PERFORMANCE_PROFILE_SUPPORTED.load(Ordering::Acquire)
        ),
        format!(
            "perf_verified={}",
            PERFORMANCE_PROFILE_VERIFIED.load(Ordering::Acquire)
        ),
        format!(
            "perf_verify_ok={}",
            PERFORMANCE_PROFILE_OK.load(Ordering::Acquire)
        ),
        format!(
            "gpu_profile_cpu_isolated={}",
            state.gpu_profile_cpu_isolated
        ),
        format!("cpu_drift0={cpu0_drift}"),
        format!("cpu_drift2={cpu2_drift}"),
        format!("cpu_drift5={cpu5_drift}"),
        format!("cpu_drift7={cpu7_drift}"),
        format!("gpu_drift={gpu_drift}"),
        format!("io_drift={io_drift}"),
        format!("display_width={}", DISPLAY_WIDTH.load(Ordering::Acquire)),
        format!("display_height={}", DISPLAY_HEIGHT.load(Ordering::Acquire)),
        format!(
            "display_density={}",
            DISPLAY_DENSITY.load(Ordering::Acquire)
        ),
        format!(
            "display_native_density={}",
            DISPLAY_NATIVE_DENSITY.load(Ordering::Acquire)
        ),
        format!("display_hz_x10={}", DISPLAY_HZ_X10.load(Ordering::Acquire)),
        format!(
            "display_max_hz_x10={}",
            DISPLAY_MAX_HZ_X10.load(Ordering::Acquire)
        ),
        format!(
            "persistence_loaded={}",
            PERSISTENCE_LOADED.load(Ordering::Acquire)
        ),
        format!(
            "sunlight_saved={}",
            if state.brightness_prev >= 1 { 1 } else { 0 }
        ),
        format!("display_ack={}", DISPLAY_APPLY_ACK.load(Ordering::Acquire)),
        format!("touch_ack={}", TOUCH_APPLY_ACK.load(Ordering::Acquire)),
        format!("cpu_manual={}", state.cpu_manual),
        format!("cpu_saved_mask={}", state.cpu_online_mask | 0x01),
        format!("cpu_write_ack={}", CPU_WRITE_ACK.load(Ordering::Acquire)),
        format!(
            "core_ctl_nodes={}",
            CORE_CTL_NODE_COUNT.load(Ordering::Acquire)
        ),
        format!(
            "runtime_keepalive_ack={}",
            KEEPALIVE_APPLY_ACK.load(Ordering::Acquire)
        ),
        format!(
            "runtime_keepalive_count={}",
            KEEPALIVE_APPLY_COUNT.load(Ordering::Acquire)
        ),
        format!("zram_size={zram_disk_mb}"),
        format!("zram_orig={}", zram_stat.orig_data_mb),
        format!("zram_compr={}", zram_stat.compr_data_mb),
        format!("zram_used={}", zram_stat.mem_used_mb),
        format!("zram_swappiness={zram_swappiness}"),
        format!("zram_alg={zram_alg}"),
        format!("gpu_load={}", gpu_get_loading()),
        format!("gpu_cur_freq={}", gpu_get_cur_freq_mhz()),
        format!("gpu_requested_min={}", state.gpu_min_freq_mhz),
        format!("gpu_requested_max={}", state.gpu_max_freq_mhz),
        format!("gpu_min_freq={}", gpu_get_min_freq_mhz()),
        format!("gpu_max_freq={}", gpu_get_max_freq_mhz()),
        format!("gpu_gov={}", gpu_get_governor()),
        format!("gpu_ged_boost={}", gpu_get_ged_boost()),
        format!("gpu_thermal_state={}", gpu_get_thermal_state()),
        format!("gpu_uncap_active={}", gpu_get_uncap_active()),
        format!(
            "gpu_power_policy={}",
            if gpu_get_power_policy() == "always_on" {
                1
            } else {
                0
            }
        ),
        format!("gpu_power_policy_str={}", gpu_get_power_policy()),
        format!("cpu_min0={}", state.cpu_min_freq0),
        format!("cpu_max0={}", state.cpu_max_freq0),
        format!("cpu_min2={}", state.cpu_min_freq2),
        format!("cpu_min5={}", state.cpu_min_freq5),
        format!("cpu_max2={}", state.cpu_max_freq2),
        format!("cpu_max5={}", state.cpu_max_freq5),
        format!("cpu_min7={}", state.cpu_min_freq7),
        format!("cpu_max7={}", state.cpu_max_freq7),
        format!("cpu_live_min0={cpu_live_min0}"),
        format!("cpu_live_max0={cpu_live_max0}"),
        format!("cpu_live_min2={cpu_live_min2}"),
        format!("cpu_live_min5={cpu_live_min5}"),
        format!("cpu_live_max2={cpu_live_max2}"),
        format!("cpu_live_max5={cpu_live_max5}"),
        format!("cpu_live_min7={cpu_live_min7}"),
        format!("cpu_live_max7={cpu_live_max7}"),
        format!(
            "cpu_freq_ack={}",
            CPU_FREQ_WRITE_ACK.load(Ordering::Acquire)
        ),
        format!("cpu_freq_drift0={cpu_freq_drift0}"),
        format!("cpu_freq_drift2={cpu_freq_drift2}"),
        format!("cpu_freq_drift5={cpu_freq_drift5}"),
        format!("cpu_freq_drift7={cpu_freq_drift7}"),
        format!("sconfig_supported={}", i32::from(sconfig::supported())),
        format!("sconfig_live={}", sconfig::read().unwrap_or(-2)),
        format!("sconfig_saved={}", state.sconfig_override),
        format!("sconfig_original={}", sconfig::original().unwrap_or(-2)),
        format!("cpu_thermal_mode={cpu_thermal_mode}"),
        format!("cpu_thermal_mode_prev={}", state.cpu_thermal_mode_prev),
        format!("cpu_thermal_unrestricted={cpu_thermal_unrestricted}"),
        format!(
            "cpu_thermal_mode_ack={}",
            CPU_THERMAL_MODE_ACK.load(Ordering::Acquire)
        ),
        format!("cpu_avail0={}", cpu_frequency_table_csv(0)),
        format!("cpu_avail2={}", cpu_frequency_table_csv(2)),
        format!("cpu_avail5={}", cpu_frequency_table_csv(5)),
        format!("cpu_avail7={}", cpu_frequency_table_csv(7)),
        format!(
            "touch_sustained_rate={}",
            TOUCH_SUSTAINED_RATE.load(Ordering::Acquire)
        ),
        format!(
            "touch_instant_rate={}",
            TOUCH_INSTANT_RATE.load(Ordering::Acquire)
        ),
        format!("touch_panel={}", TOUCH_PANEL.load(Ordering::Acquire)),
        format!(
            "touch_control_path={}",
            TOUCH_CONTROL_PATH.load(Ordering::Acquire)
        ),
        format!(
            "touch_measured_rate_x10={}",
            touch_resampler::measured_hz_x10()
        ),
        format!(
            "touch_source_rate_x10={}",
            touch_resampler::source_measured_hz_x10()
        ),
        format!(
            "touch_measurement_active={}",
            touch_resampler::measurement_active()
        ),
        format!("touch_resampler_ready={}", touch_resampler::ready_hz()),
        format!(
            "touch_resampler_path={}",
            touch_resampler::attachment_path()
        ),
        format!("touch_resampler_error={}", touch_resampler::last_error()),
        format!(
            "touch_physical_frames={}",
            touch_resampler::physical_frames()
        ),
        format!(
            "touch_injected_frames={}",
            touch_resampler::injected_frames()
        ),
    ]
}

fn snapshot() -> String {
    let (charging, bypass_saved, bypass_threshold) = persisted_state()
        .lock()
        .ok()
        .map(|state| {
            (
                normalize_charging_profile(state.charging),
                state.bypass_charging,
                state.bypass_threshold,
            )
        })
        .unwrap_or((0, 0, 0));
    let charging_fcc_ua = charging_fcc_path()
        .as_deref()
        .and_then(read_i64)
        .unwrap_or(-1);
    let charging_supported = i32::from(charging_fcc_path().is_some());
    let charging_sic = read_trimmed(charging_path())
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(-1);
    let battery_voltage_uv = battery("voltage_now");
    let battery_current_ua = battery("current_now");
    let bypass_live = bypass_charging_state();
    let bypass_verified = read_trimmed("/sys/class/power_supply/battery/bypass_charging_active")
        .ok()
        .and_then(|value| parse_kernel_bool(&value))
        .map(i32::from)
        .unwrap_or(-1);
    let bypass_phase = bypass_policy::phase(
        bypass_saved == 1,
        bypass_threshold,
        battery("capacity").parse().ok(),
        usb("online").parse().unwrap_or(-1),
        bypass_live,
        bypass_verified,
        battery_current_ua.parse().ok(),
        BYPASS_POLICY_ERROR.load(Ordering::Acquire) == 1,
    );
    let charging_live_mw = battery_voltage_uv
        .parse::<i64>()
        .ok()
        .zip(battery_current_ua.parse::<i64>().ok())
        .map(|(voltage, current)| {
            ((voltage as i128 * (current as i128).abs()) / 1_000_000_000) as i64
        })
        .unwrap_or(-1);
    let touch_hal = if vendor_binder::touch_available()
        || Path::new("/sys/devices/platform/goodix_ts.0/switch_report_rate").exists()
    {
        1
    } else {
        0
    };
    let display_hal = if vendor_binder::display_available() {
        1
    } else {
        0
    };
    let fields = [
        format!("protocol={PROTOCOL_VERSION}"),
        format!(
            "app_client_seen={}",
            APP_CLIENT_SEEN.load(Ordering::Acquire)
        ),
        format!(
            "app_client_transport={}",
            APP_CLIENT_TRANSPORT.load(Ordering::Acquire)
        ),
        format!("charging={charging}"),
        format!("charging_supported={charging_supported}"),
        format!("charging_fcc_ua={charging_fcc_ua}"),
        format!("charging_adapter_w={}", charging_adapter_watts()),
        format!(
            "charging_pd_auth={}",
            usb("pd_authentication").parse::<i32>().unwrap_or(-1)
        ),
        format!(
            "charging_quick_type={}",
            usb("quick_charge_type").parse::<i32>().unwrap_or(-1)
        ),
        format!("charging_sic={charging_sic}"),
        format!("charging_live_mw={charging_live_mw}"),
        format!(
            "bypass_charging_supported={}",
            i32::from(bypass_charging_path().is_some())
        ),
        format!("bypass_charging_state={bypass_live}"),
        format!("bypass_charging_saved={bypass_saved}"),
        format!("bypass_charging_verified={bypass_verified}"),
        format!("bypass_threshold={bypass_threshold}"),
        format!("bypass_phase={bypass_phase}"),
        format!(
            "bypass_error={}",
            BYPASS_POLICY_ERROR.load(Ordering::Acquire)
        ),
        format!("cap={}", sanitize(battery("capacity"))),
        format!("temp={}", sanitize(battery("temp"))),
        format!("voltage={}", sanitize(battery_voltage_uv)),
        format!("current={}", sanitize(battery_current_ua)),
        format!("status={}", sanitize(battery("status"))),
        format!("health={}", sanitize(battery("health"))),
        format!(
            "usb_type={}",
            sanitize({
                let real_type = usb("real_type");
                if real_type != "NA" && !real_type.is_empty() {
                    real_type
                } else {
                    usb("usb_type")
                }
            })
        ),
        format!("service_enabled={}", i32::from(service_control::enabled())),
        format!(
            "service_configured={}",
            i32::from(service_control::configured())
        ),
        format!("usb_online={}", sanitize(usb("online"))),
        format!("cpu_online={}", sanitize(read_cpu_online_mask_string())),
        format!("cpu0={}", sanitize(cpu_freq(0))),
        format!("cpu1={}", sanitize(cpu_freq(1))),
        format!("cpu2={}", sanitize(cpu_freq(2))),
        format!("cpu3={}", sanitize(cpu_freq(3))),
        format!("cpu4={}", sanitize(cpu_freq(4))),
        format!("cpu5={}", sanitize(cpu_freq(5))),
        format!("cpu6={}", sanitize(cpu_freq(6))),
        format!("cpu7={}", sanitize(cpu_freq(7))),
        format!("gov0={}", sanitize(policy_governor(0))),
        format!("gov2={}", sanitize(policy_governor(2))),
        format!("gov5={}", sanitize(policy_governor(5))),
        format!("gov7={}", sanitize(policy_governor(7))),
        format!("gpu_gov={}", sanitize(gpu_governor())),
        format!("io={}", sanitize(io_scheduler())),
        format!("touch_hal={touch_hal}"),
        format!("display_hal={display_hal}"),
        format!("touch={}", TOUCH_STATE.load(Ordering::Acquire)),
        format!(
            "display_color={}",
            DISPLAY_COLOR_STATE.load(Ordering::Acquire)
        ),
        format!(
            "display_temp={}",
            DISPLAY_TEMP_STATE.load(Ordering::Acquire)
        ),
        format!(
            "sunlight={}",
            DISPLAY_SUNLIGHT_STATE.load(Ordering::Acquire)
        ),
        format!("silky={}", DISPLAY_SILKY_STATE.load(Ordering::Acquire)),
        format!("video={}", DISPLAY_VIDEO_STATE.load(Ordering::Acquire)),
        format!("dolby={}", DISPLAY_DOLBY_STATE.load(Ordering::Acquire)),
        format!("perf={}", PERFORMANCE_STATE.load(Ordering::Acquire)),
    ];
    format!(
        "{};{}",
        fields.join(";"),
        snapshot_persistence_fields().join(";")
    )
}

fn capture_zram_originals() -> Result<(), String> {
    service_control::remember(
        "zram.original",
        serde_json::json!({
            "size": zram_get_disksize_mb(), "algorithm": zram_get_algorithm(),
            "swappiness": zram_get_swappiness()
        }),
    )
}

fn window_override(kind: &str) -> Result<String, String> {
    let output =
        run_process_with_timeout("/system/bin/cmd", &["window", kind], Duration::from_secs(3))?;
    if !output.status.success() {
        return Err(format!("cannot capture original display {kind}"));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("Override ")
                .and_then(|line| line.split_once(':'))
                .map(|(_, value)| value.trim().to_string())
        })
        .unwrap_or_else(|| "reset".into()))
}

fn capture_window_originals() -> Result<(), String> {
    if service_control::original("window.original").is_some() {
        return Ok(());
    }
    service_control::remember(
        "window.original",
        serde_json::json!({"size": window_override("size")?, "density": window_override("density")?}),
    )
}

fn capture_feature_original(cmd: &str) -> Result<(), String> {
    if cmd.starts_with("SET system.colors") {
        system_colors::capture_original()?;
    }
    Ok(())
}

fn restore_released_controls() -> Result<(), String> {
    let _restoring = service_control::Restoration::begin();
    let saved = persisted_state()
        .lock()
        .map_err(|_| "settings lock poisoned")?
        .clone();
    let mut errors = Vec::new();
    let mut run = |name: &str, result: Result<(), String>| {
        if let Err(error) = result {
            errors.push(format!("{name}: {error}"));
        }
    };
    run("Xiaomi Sconfig", sconfig::release());
    // Never leave an adapter charge-pause request behind when control is disabled.
    if bypass_charging_path().is_some()
        && (saved.bypass_charging == 1 || bypass_charging_state() == 1)
    {
        run("bypass", apply_bypass_charging(false));
    }
    run("touch output", touch_resampler::set_target_hz(0));
    run("touch game mode", restore_peridot_game_mode());
    if service_control::original("touch.hal").is_some() {
        if vendor_binder::touch_available() {
            run("OEM touch", apply_touch_hal_profile(0));
        }
        if let Some(rate) = service_control::original("touch.thp_rate").and_then(|v| v.as_u64()) {
            // Restore the saved timing, including OEM values outside fixed-rate presets.
            run(
                "touch timing",
                (|| {
                    let layout = locate_touch_thp_layout()?;
                    let mut memory = OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open(format!("/proc/{}/mem", layout.pid))
                        .map_err(|e| e.to_string())?;
                    memory
                        .seek(SeekFrom::Start(layout.configured_rate_addr))
                        .map_err(|e| e.to_string())?;
                    memory
                        .write_all(&(rate as u16).to_le_bytes())
                        .map_err(|e| e.to_string())?;
                    if read_touch_thp_rate(layout, layout.configured_rate_addr)? != rate as u16 {
                        return Err("original timing readback mismatch".into());
                    }
                    Ok(())
                })(),
            );
        }
    }
    if service_control::original("display.hal").is_some() {
        // The vendor HAL provides setters but no reliable cross-ROM getters.
        // Use its neutral/default modes rather than inventing a captured value.
        if vendor_binder::display_available() {
            run("display defaults", reset_expert_display());
            run("display color", set_display_color(1));
            run("display temperature", set_display_temp(2));
            run("sunlight", set_sunlight(false));
            for (case, state) in [
                (57, &DISPLAY_SILKY_STATE),
                (27, &DISPLAY_VIDEO_STATE),
                (44, &DISPLAY_DOLBY_STATE),
            ] {
                run(
                    "display enhancement",
                    set_display_toggle(case, false, state),
                );
            }
        } else {
            run(
                "display defaults",
                Err("display HAL unavailable; defaults were not verified".into()),
            );
        }
    }
    if let Some(original) = service_control::original("zram.original") {
        let size = original["size"].as_i64().unwrap_or(-1) as i32;
        let algorithm = original["algorithm"].as_str().unwrap_or("");
        if size >= 0
            && !algorithm.is_empty()
            && (zram_get_disksize_mb() != size || zram_get_algorithm() != algorithm)
        {
            let result = mutate_persisted_state(|s| s.zram_algorithm = algorithm.into())
                .and_then(|_| set_zram_size(size));
            run("ZRAM", result);
        }
    }
    // Open the range before restoring either end. A prior exact lock can
    // reject the original minimum when it lies above the current maximum.
    for (path, value) in service_control::originals() {
        if path.ends_with("/scaling_min_freq")
            || (path.contains("/devfreq/") && path.ends_with("/min_freq"))
        {
            if let Some(value) = value.as_str() {
                let max_path = if path.ends_with("/scaling_min_freq") {
                    path.replace("/scaling_min_freq", "/scaling_max_freq")
                } else {
                    path.replace("/min_freq", "/max_freq")
                };
                if let (Ok(minimum), Ok(maximum)) = (
                    value.trim().parse::<i64>(),
                    read_trimmed(&max_path)
                        .and_then(|v| v.parse::<i64>().map_err(|e| e.to_string())),
                ) {
                    if minimum > maximum {
                        run(
                            &max_path,
                            fs::write(&max_path, minimum.to_string()).map_err(|e| e.to_string()),
                        );
                    }
                }
                run(&path, fs::write(&path, value).map_err(|e| e.to_string()));
            }
        }
    }
    let mut original_nodes = service_control::originals();
    // Restore manual core states while core_ctl is still paused, then hand
    // automatic topology management back to the kernel last.
    original_nodes
        .sort_by_key(|(path, _)| path.contains("/core_ctl/") && path.ends_with("/enable"));
    for (path, value) in original_nodes {
        if !(path.starts_with("/sys/") || path.starts_with("/proc/")) {
            continue;
        }
        let Some(value) = value.as_str() else {
            continue;
        };
        run(
            &path,
            (|| {
                fs::write(&path, value).map_err(|e| e.to_string())?;
                let actual = read_trimmed(&path)?;
                if service_control::normalized_original(Path::new(&path), &actual) != value {
                    return Err(format!("readback differs: {actual}"));
                }
                Ok(())
            })(),
        );
    }
    if let Some(original) = service_control::original("window.original") {
        for kind in ["size", "density"] {
            if let Some(value) = original[kind].as_str() {
                run(
                    "display canvas",
                    run_process_with_timeout(
                        "/system/bin/cmd",
                        &["window", kind, value],
                        Duration::from_secs(3),
                    )
                    .and_then(|output| {
                        if output.status.success() {
                            let actual = window_override(kind)?;
                            if actual == value {
                                Ok(())
                            } else {
                                Err(format!(
                                    "{kind} restoration mismatch: expected {value}, actual {actual}"
                                ))
                            }
                        } else {
                            Err(String::from_utf8_lossy(&output.stderr).into())
                        }
                    }),
                );
            }
        }
        refresh_display_info();
    }
    run("system colors", system_colors::restore_original());
    // Restoring helpers must not erase preferences retained by a disable action.
    run(
        "saved preferences",
        mutate_persisted_state(|state| *state = saved),
    );
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!("restoration incomplete: {}", errors.join(" | ")))
    }
}

fn cleared_device_state() -> PersistedState {
    PersistedState {
        charging: -1,
        touch: -1,
        display_color: -1,
        display_temp: -1,
        sunlight: -1,
        silky: -1,
        video: -1,
        dolby: -1,
        perf: -1,
        cpu_manual: -1,
        gpu_min_freq_mhz: -1,
        gpu_max_freq_mhz: -1,
        gpu_governor: String::new(),
        gpu_ged_boost: -1,
        gpu_power_policy: String::new(),
        zram_size_mb: -1,
        zram_algorithm: String::new(),
        zram_swappiness: -1,
        ..PersistedState::default()
    }
}

fn clear_released_control_status() {
    // These are applied-selection caches, not live hardware measurements.
    // Never leave a released profile looking active after reset or disable.
    for state in [
        &PERFORMANCE_STATE,
        &PERFORMANCE_PROFILE_VERIFIED,
        &PERFORMANCE_PROFILE_OK,
        &TOUCH_STATE,
        &TOUCH_SUSTAINED_RATE,
        &TOUCH_INSTANT_RATE,
        &TOUCH_APPLY_ACK,
        &DISPLAY_COLOR_STATE,
        &DISPLAY_TEMP_STATE,
        &DISPLAY_SUNLIGHT_STATE,
        &DISPLAY_SILKY_STATE,
        &DISPLAY_VIDEO_STATE,
        &DISPLAY_DOLBY_STATE,
        &DISPLAY_APPLY_ACK,
        &CPU_WRITE_ACK,
        &CPU_FREQ_WRITE_ACK,
    ] {
        state.store(-1, Ordering::Release);
    }
}

fn transition_service(cmd: &str) -> Result<(), String> {
    if cmd != "SET service.enabled 1" {
        subsystem_clocks::restore_all()?;
        app_runtime::release()?;
        if cmd == "ACTION service.reset" {
            app_runtime::reset_all()?;
        }
    }
    if cmd == "SET service.enabled 1" {
        system_colors::restore_resume()?;
        service_control::set_release_pending(false)?;
        service_control::set_intent(true, service_control::configured())?;
        restore_persisted_state();
        app_runtime::wake();
        return Ok(());
    }
    // Persist inactive intent first: a process crash/reboot cannot resume guards
    // halfway through release. Original journal/preferences remain for retries.
    if service_control::enabled() {
        system_colors::capture_resume()?;
    }
    service_control::set_release_pending(true)?;
    service_control::set_intent(false, service_control::configured())?;
    restore_released_controls()?;
    clear_released_control_status();
    service_control::set_release_pending(false)?;
    if cmd == "ACTION service.reset" {
        mutate_persisted_state(|state| *state = cleared_device_state())?;
        service_control::set_intent(false, false)?;
        service_control::clear_originals()?;
        service_control::set_intent(true, false)?;
    }
    Ok(())
}

pub fn handle_command(line: &str) -> String {
    let cmd = line.trim();
    if subsystem_clocks::is_command(cmd) {
        let _gate = match service_control::GATE.write() {
            Ok(gate) => gate,
            Err(_) => return "ERR service transition lock poisoned".into(),
        };
        return match subsystem_clocks::command(cmd) {
            Ok(value) => format!("OK {value}"),
            Err(error) => format!("ERR {error}"),
        };
    }
    if app_runtime::is_command(cmd) {
        if cmd.starts_with("GET ") {
            return match app_runtime::command(cmd) {
                Ok(value) => format!("OK {value}"),
                Err(error) => format!("ERR {error}"),
            };
        }
        let _gate = match service_control::GATE.write() {
            Ok(gate) => gate,
            Err(_) => return "ERR service transition lock poisoned".into(),
        };
        return match app_runtime::command(cmd) {
            Ok(value) => format!("OK {value}"),
            Err(error) => format!("ERR {error}"),
        };
    }
    if cmd == "SET service.enabled 0"
        || cmd == "SET service.enabled 1"
        || cmd == "ACTION service.reset"
    {
        let _gate = match service_control::GATE.write() {
            Ok(gate) => gate,
            Err(_) => return "ERR service transition lock poisoned".into(),
        };
        let result = transition_service(cmd);
        return match result {
            Ok(()) => "OK applied".into(),
            Err(error) => format!("ERR {error}"),
        };
    }
    let _gate = match service_control::GATE.read() {
        Ok(gate) => gate,
        Err(_) => return "ERR service transition lock poisoned".into(),
    };
    if cmd.starts_with("SET ") || cmd.starts_with("ACTION ") {
        if let Err(error) = app_runtime::check_global_command(cmd) {
            return format!("ERR {error}");
        }
        if !service_control::enabled() {
            return "ERR Peridot Essential is disabled; enable it in Settings".into();
        }
        if let Err(error) = capture_feature_original(cmd) {
            return format!("ERR {error}");
        }
        if !service_control::configured() {
            if let Err(error) = service_control::set_intent(true, true) {
                return format!("ERR {error}");
            }
        }
    }
    handle_enabled_command(line)
}

fn handle_enabled_command(line: &str) -> String {
    let cmd = line.trim();
    if cmd == "PING" {
        return format!("OK PONG {PROTOCOL_VERSION}");
    }
    if cmd == "GET snapshot" {
        return format!("OK {}", snapshot());
    }
    // SettingsProvider owns palette persistence. These commands deliberately
    // bypass hardware state recording and all background reassertion guards.
    if cmd == "GET system.colors" {
        return match system_colors::read() {
            Ok(state) => format!("OK {state}"),
            Err(error) => format!("ERR {error}"),
        };
    }
    if cmd == "SET system.colors.wallpaper" {
        return match system_colors::apply_wallpaper() {
            Ok(state) => format!("OK {state}"),
            Err(error) => format!("ERR {error}"),
        };
    }
    if cmd == "GET system.colors.contrast" {
        return match system_colors::read_contrast() {
            Ok(state) => format!("OK {state}"),
            Err(error) => format!("ERR {error}"),
        };
    }
    if let Some(args) = cmd.strip_prefix("SET system.colors.contrast ") {
        return match system_colors::apply_contrast(args) {
            Ok(state) => format!("OK {state}"),
            Err(error) => format!("ERR {error}"),
        };
    }
    if let Some(args) = cmd.strip_prefix("SET system.colors ") {
        return match system_colors::apply_custom(args) {
            Ok(state) => format!("OK {state}"),
            Err(error) => format!("ERR {error}"),
        };
    }

    let result: Result<(), String> = if let Some(arg) = cmd.strip_prefix("SET cpu.sconfig ") {
        arg.parse::<i32>()
            .map_err(|_| "invalid Sconfig selection".to_string())
            .and_then(sconfig::set)
    } else if let Some(arg) = cmd.strip_prefix("SET charging ") {
        let value = match arg.trim() {
            "standard" | "adaptive" => 0,
            "boost" => 90,
            raw => match raw.parse::<i32>() {
                Ok(value) => value,
                Err(_) => return "ERR invalid charging profile".into(),
            },
        };
        let bypass_enabled = persisted_state()
            .lock()
            .ok()
            .is_some_and(|state| state.bypass_charging == 1);
        if bypass_enabled {
            Err("disable bypass charging before changing the charging profile".into())
        } else {
            apply_charging_profile(value)
        }
    } else if let Some(arg) = cmd.strip_prefix("SET charging.bypass_threshold ") {
        arg.trim()
            .parse::<i32>()
            .map_err(|_| "invalid bypass threshold".to_string())
            .and_then(|threshold| configure_bypass(None, Some(threshold)))
    } else if let Some(arg) = cmd.strip_prefix("SET charging.bypass ") {
        match arg.trim() {
            "1" => configure_bypass(Some(true), None),
            "0" => configure_bypass(Some(false), None),
            _ => Err("invalid bypass charging state".into()),
        }
    } else if let Some(arg) = cmd.strip_prefix("SET touch ") {
        arg.trim()
            .parse::<i32>()
            .map_err(|_| "invalid touch profile".to_string())
            .and_then(set_touch_profile)
    } else if let Some(arg) = cmd.strip_prefix("SET display.expert.gamut ") {
        arg.trim()
            .parse::<i32>()
            .map_err(|_| "bad expert gamut".to_string())
            .and_then(set_expert_gamut)
    } else if let Some(rest) = cmd.strip_prefix("SET display.expert.channel ") {
        let mut parts = rest.split_whitespace();
        let channel = parts
            .next()
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(-1);
        let value = parts
            .next()
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(i32::MIN);
        set_expert_channel(channel, value)
    } else if cmd == "SET display.expert.reset" {
        reset_expert_display()
    } else if let Some(arg) = cmd.strip_prefix("SET display.color ") {
        arg.trim()
            .parse::<i32>()
            .map_err(|_| "bad display color".to_string())
            .and_then(set_display_color)
    } else if let Some(arg) = cmd.strip_prefix("SET display.temp ") {
        arg.trim()
            .parse::<i32>()
            .map_err(|_| "bad display temp".to_string())
            .and_then(set_display_temp)
    } else if let Some(arg) = cmd.strip_prefix("SET display.sunlight ") {
        match arg.trim() {
            "1" => set_sunlight(true),
            "0" => set_sunlight(false),
            _ => Err("invalid sunlight state".into()),
        }
    } else if let Some(arg) = cmd.strip_prefix("SET display.silky ") {
        match arg.trim() {
            "1" => set_display_toggle(57, true, &DISPLAY_SILKY_STATE),
            "0" => set_display_toggle(57, false, &DISPLAY_SILKY_STATE),
            _ => Err("invalid silky state".into()),
        }
    } else if let Some(arg) = cmd.strip_prefix("SET display.video ") {
        match arg.trim() {
            "1" => set_display_toggle(27, true, &DISPLAY_VIDEO_STATE),
            "0" => set_display_toggle(27, false, &DISPLAY_VIDEO_STATE),
            _ => Err("invalid video state".into()),
        }
    } else if let Some(arg) = cmd.strip_prefix("SET display.dolby ") {
        match arg.trim() {
            "1" => set_display_toggle(44, true, &DISPLAY_DOLBY_STATE),
            "0" => set_display_toggle(44, false, &DISPLAY_DOLBY_STATE),
            _ => Err("invalid dolby state".into()),
        }
    } else if let Some(arg) = cmd.strip_prefix("SET perf ") {
        arg.trim()
            .parse::<i32>()
            .map_err(|_| "bad profile".to_string())
            .and_then(apply_performance_profile)
    } else if let Some(arg) = cmd.strip_prefix("SET cpu.manual ") {
        match arg.trim() {
            "1" => set_cpu_manual(true),
            "0" => set_cpu_manual(false),
            _ => Err("invalid CPU manual state".into()),
        }
    } else if let Some(rest) = cmd.strip_prefix("SET cpu.core ") {
        let mut parts = rest.split_whitespace();
        let cpu = parts
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(usize::MAX);
        let online = parts.next().unwrap_or("");

        if parts.next().is_some() {
            Err("too many CPU core arguments".into())
        } else {
            match online {
                "1" => set_cpu_core(cpu, true),
                "0" => set_cpu_core(cpu, false),
                _ => Err("invalid CPU core state".into()),
            }
        }
    } else if let Some(rest) = cmd.strip_prefix("SET cpu.gov ") {
        let mut parts = rest.split_whitespace();
        let policy = parts
            .next()
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(-1);
        let governor = parts.next().unwrap_or("");
        set_cpu_governor(policy, governor)
    } else if let Some(rest) = cmd.strip_prefix("SET cpu.min_freq ") {
        let mut parts = rest.split_whitespace();
        let policy = parts
            .next()
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(-1);
        let mhz = parts
            .next()
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(0);
        set_cpu_cluster_min_freq(policy, mhz)
    } else if let Some(rest) = cmd.strip_prefix("SET cpu.max_freq ") {
        let mut parts = rest.split_whitespace();
        let policy = parts
            .next()
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(-1);
        let mhz = parts
            .next()
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(0);
        set_cpu_cluster_max_freq(policy, mhz)
    } else if let Some(rest) = cmd.strip_prefix("SET cpu.freq_range ") {
        let mut parts = rest.split_whitespace();
        let policy = parts
            .next()
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(-1);
        let min_mhz = parts
            .next()
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(0);
        let max_mhz = parts
            .next()
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(0);
        set_cpu_cluster_freq_range(policy, min_mhz, max_mhz)
    } else if let Some(arg) = cmd.strip_prefix("SET cpu.freq_reset ") {
        let policy = arg.trim().parse::<i32>().unwrap_or(-1);
        reset_cpu_cluster_freq_range(policy)
    } else if let Some(arg) = cmd.strip_prefix("SET gpu.gov ") {
        set_gpu_governor(arg.trim())
    } else if let Some(arg) = cmd.strip_prefix("SET io.scheduler ") {
        set_io_scheduler(arg.trim())
    } else if let Some(rest) = cmd.strip_prefix("SET display.res ") {
        let mut parts = rest.split_whitespace();
        let width = parts
            .next()
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(0);
        let height = parts
            .next()
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(0);
        let density = parts
            .next()
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(0);

        apply_display_resolution(width, height, density, true)
    } else if let Some(arg) = cmd.strip_prefix("SET zram.size ") {
        arg.trim()
            .parse::<i32>()
            .map_err(|_| "bad zram size".to_string())
            .and_then(set_zram_size)
    } else if let Some(arg) = cmd.strip_prefix("SET zram.algorithm ") {
        set_zram_algorithm(arg.trim())
    } else if let Some(arg) = cmd.strip_prefix("SET zram.swappiness ") {
        arg.trim()
            .parse::<i32>()
            .map_err(|_| "bad swappiness".to_string())
            .and_then(set_zram_swappiness)
    } else if cmd == "ACTION zram.compact" {
        compact_zram()
    } else if let Some(arg) = cmd.strip_prefix("SET gpu.min_freq ") {
        arg.trim()
            .parse::<i32>()
            .map_err(|_| "bad gpu min freq".to_string())
            .and_then(set_gpu_min_freq)
    } else if let Some(arg) = cmd.strip_prefix("SET gpu.max_freq ") {
        arg.trim()
            .parse::<i32>()
            .map_err(|_| "bad gpu max freq".to_string())
            .and_then(set_gpu_max_freq)
    } else if let Some(arg) = cmd.strip_prefix("SET gpu.governor ") {
        set_gpu_governor(arg.trim())
    } else if let Some(arg) = cmd.strip_prefix("SET gpu.ged_boost ") {
        arg.trim()
            .parse::<i32>()
            .map_err(|_| "bad ged boost flag".to_string())
            .and_then(|v| set_gpu_ged_boost(v == 1))
    } else if let Some(arg) = cmd.strip_prefix("SET gpu.power_policy ") {
        set_gpu_power_policy(arg.trim())
    } else if let Some(arg) = cmd.strip_prefix("SET gpu.uncap ") {
        arg.trim()
            .parse::<i32>()
            .map_err(|_| "bad uncap flag".to_string())
            .and_then(|v| set_gpu_uncap(v == 1))
    } else if cmd == "ACTION gpu.uncap_full_speed" {
        set_gpu_uncap(true)
    } else {
        return "ERR unknown command".into();
    };

    match result {
        Ok(()) => match record_successful_command(cmd) {
            Ok(())
                if cmd.starts_with("SET cpu.min_freq ")
                    || cmd.starts_with("SET cpu.max_freq ")
                    || cmd.starts_with("SET cpu.freq_range ")
                    || cmd.starts_with("SET cpu.freq_reset ") =>
            {
                format!(
                    "OK requested;cpu_freq_ack={}",
                    CPU_FREQ_WRITE_ACK.load(Ordering::Acquire)
                )
            }
            Ok(())
                if cmd.starts_with("SET perf ")
                    || cmd.starts_with("SET gpu.")
                    || cmd == "ACTION gpu.uncap_full_speed" =>
            {
                format!(
                    "OK requested;gpu_exact={};effective_mhz={}-{}",
                    i32::from(gpu_profile_verified(
                        PERFORMANCE_STATE.load(Ordering::Acquire)
                    )),
                    gpu_get_min_freq_mhz(),
                    gpu_get_max_freq_mhz()
                )
            }
            Ok(()) => "OK applied".into(),
            Err(error) => format!("ERR request persistence failed: {error}"),
        },
        Err(e) => format!("ERR {e}"),
    }
}

fn serve_authenticated_client<S: Read + Write>(mut stream: S, peer: u32, transport: i32) {
    if !client_uid_allowed(peer, configured_app_uid_policy()) {
        eprintln!("PERIDOT_ESSENTIALD_PEER_REJECT unauthorized_uid");
        let _ = stream.write_all(b"ERR unauthorized peer\n");
        return;
    }
    record_app_client(peer, transport);

    // New profile RPC uses a bounded length frame. JSON must never be parsed
    // from one arbitrary Unix/TCP packet; legacy commands remain compatible.
    let mut first = [0u8; 1];
    if stream.read_exact(&mut first).is_err() {
        return;
    }
    let bytes = if first[0] == b'@' {
        let mut header = Vec::new();
        loop {
            let mut byte = [0u8; 1];
            if header.len() > 8 || stream.read_exact(&mut byte).is_err() {
                return;
            }
            if byte[0] == b'\n' {
                break;
            }
            header.push(byte[0]);
        }
        let Some(len) = std::str::from_utf8(&header)
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|v| *v > 0 && *v <= 4096)
        else {
            return;
        };
        let mut body = vec![0u8; len];
        if stream.read_exact(&mut body).is_err() {
            return;
        }
        body
    } else {
        let mut buf = [0u8; 4095];
        let Ok(n) = stream.read(&mut buf) else {
            return;
        };
        let mut body = vec![first[0]];
        body.extend_from_slice(&buf[..n]);
        body
    };
    let req = String::from_utf8_lossy(&bytes);
    let response = handle_command(&req);
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(b"\n");
    let _ = stream.flush();
}

fn serve_client_with_transport(mut stream: UnixStream, transport: i32) {
    match peer_uid(&stream) {
        Ok(peer) => serve_authenticated_client(stream, peer, transport),
        Err(error) => {
            eprintln!("PERIDOT_ESSENTIALD_PEER_REJECT {error}");
            let _ = stream.write_all(b"ERR peer credentials unavailable\n");
        }
    }
}

fn serve_loopback_client(mut stream: TcpStream) {
    match tcp_peer_uid(&stream) {
        Ok(peer) => serve_authenticated_client(stream, peer, 3),
        Err(error) => {
            eprintln!("PERIDOT_ESSENTIALD_LOOPBACK_PEER_REJECT {error}");
            let _ = stream.write_all(b"ERR peer credentials unavailable\n");
        }
    }
}

pub fn serve_client(stream: UnixStream) {
    serve_client_with_transport(stream, 1);
}

#[cfg(test)]
mod tests {
    use super::{persisted_cpu_range, resolve_peridot_cpu_khz};
    #[test]
    fn peridot_cpu_mhz_ids_resolve_exact_kernel_khz() {
        let raw = "364800 441600 3014400 2956800";
        assert_eq!(parse_cpu_frequency_table(raw), vec![364, 441, 2956, 3014]);
        assert_eq!(resolve_peridot_cpu_khz(raw, 364).unwrap(), 364800);
        assert_eq!(resolve_peridot_cpu_khz(raw, 3014).unwrap(), 3014400);
        assert!(resolve_peridot_cpu_khz(raw, 3206).is_err());
        assert!(resolve_peridot_cpu_khz("364800 364900", 364).is_err());
        let state = PersistedState {
            cpu_min_freq5: 480,
            cpu_max_freq5: 2803,
            ..Default::default()
        };
        assert_eq!(persisted_cpu_range(&state, 5), Some((480, 2803)));
        assert!(persisted_cpu_ranges_active(&state));
    }

    #[test]
    fn fresh_and_partial_settings_leave_touch_and_display_to_rom() {
        for raw in ["", "charging=0\n", "touch=3\ndisplay_temp=0\ndolby=7\n"] {
            let state = super::parse_persisted_state(raw);
            assert_eq!(state.touch, -1);
            assert_eq!(state.display_color, -1);
            assert_eq!(state.display_temp, -1);
            assert_eq!(state.sunlight, -1);
            assert_eq!(state.silky, -1);
            assert_eq!(state.video, -1);
            assert_eq!(state.dolby, -1);
        }
    }

    #[test]
    fn explicit_touch_and_colour_choices_survive_loading() {
        for profile in 0..=1 {
            let state = super::parse_persisted_state(&format!(
                "touch={profile}\ndisplay_color=2\ndisplay_temp=3\nsilky=1\n"
            ));
            assert_eq!(state.touch, profile);
            assert_eq!(state.display_color, 2);
            assert_eq!(state.display_temp, 3);
            assert_eq!(state.silky, 1);
        }
    }
    #[test]
    fn retired_touch_profiles_are_not_restored() {
        for profile in [2, 3, 4, 7] {
            let state = super::parse_persisted_state(&format!("touch={profile}\n"));
            assert_eq!(state.touch, -1);
            assert!(super::set_touch_profile(profile).is_err());
        }
    }

    #[test]
    fn bypass_threshold_migrates_and_validates_without_losing_saved_intent() {
        let legacy = super::parse_persisted_state("bypass_charging=1\n");
        assert_eq!(legacy.bypass_threshold, 0);
        assert_eq!(legacy.bypass_charging, 1);
        for threshold in [0, 20, 40, 80, 90] {
            let saved = super::parse_persisted_state(&format!(
                "bypass_charging=1\nbypass_threshold={threshold}\n"
            ));
            assert_eq!(saved.bypass_threshold, threshold);
            assert_eq!(saved.bypass_charging, 1);
        }
        assert_eq!(
            super::parse_persisted_state("bypass_threshold=50\n").bypass_threshold,
            0
        );
        let reset = super::cleared_device_state();
        assert_eq!(reset.bypass_charging, 0);
        assert_eq!(reset.bypass_threshold, 0);
    }
    #[test]
    fn reset_sentinels_survive_state_loading() {
        let state = super::parse_persisted_state(
            "charging=-1\ntouch=-1\ndisplay_color=-1\ndisplay_temp=-1\nsunlight=-1\nsilky=-1\nvideo=-1\ndolby=-1\ncpu_manual=-1\nperf=-1\ngpu_profile_cpu_isolated=1\n",
        );
        assert_eq!(state.touch, -1);
        assert_eq!(state.charging, -1);
        assert_eq!(state.cpu_manual, -1);
        assert_eq!(state.display_color, -1);
        assert_eq!(state.perf, -1);
    }
    #[test]
    fn bypass_readback_accepts_explicit_kernel_booleans_only() {
        for value in ["1", " Y\n", "true", "ON", "enabled"] {
            assert_eq!(super::parse_kernel_bool(value), Some(true));
        }
        for value in ["0", " n\n", "false", "OFF", "disabled"] {
            assert_eq!(super::parse_kernel_bool(value), Some(false));
        }
        for value in ["", "2", "-1", "unsupported", "1 failed", "charging"] {
            assert_eq!(super::parse_kernel_bool(value), None);
        }
    }

    #[cfg(not(target_os = "android"))]
    use super::run_process_with_timeout;
    use super::{
        AppUidPolicy, MI_THERMAL_NO_LIMITS_MODE, PersistedState, charging_profile_fcc_ua,
        classify_touch_panel_version, client_uid_allowed, cpu_frequency_drift_status,
        find_touch_thp_config_offset, gaming_dynamic_target_opp, mi_thermal_cpu_limit_request,
        migrate_legacy_gpu_profile_cpu_state, mtk_powerhal_cpu_range_request,
        normalize_charging_profile, normalize_mi_thermal_config_mode, normalize_reported_power_w,
        package_app_id_from_packages_list, parse_cpu_frequency_table, parse_cpu_time_in_state,
        parse_ged_current_frequency_mhz, persisted_cpu_ranges_active, profile_uses_ged_boost,
        scaled_display_density, tcp_client_uid_from_table, touch_hal_profile_sequence,
        valid_charging_profile, valid_mi_thermal_config_mode, validate_cpu_frequency_range_against,
    };

    fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn module_socket_accepts_only_root_or_the_installed_app_uid() {
        let policy = AppUidPolicy::Enforce(10_321);
        assert!(client_uid_allowed(0, policy));
        assert!(client_uid_allowed(10_321, policy));
        assert!(!client_uid_allowed(10_322, policy));
        assert!(!client_uid_allowed(2_000, policy));
        assert!(!client_uid_allowed(10_321, AppUidPolicy::Reject));
        assert!(client_uid_allowed(10_321, AppUidPolicy::SelinuxOnly));
    }

    #[test]
    fn native_socket_resolves_and_accepts_the_package_app_id_for_every_user() {
        let packages = "android 1000 0 /data/user/0/android platform 0 1 1 1 0\n\
io.github.davvv98.peridotessential 10228 0 /data/user/0/io.github.davvv98.peridotessential default 0 1 1 1 0\n";
        assert_eq!(
            package_app_id_from_packages_list(packages, "io.github.davvv98.peridotessential"),
            Some(10_228)
        );
        let policy = AppUidPolicy::EnforceAppId(10_228);
        assert!(client_uid_allowed(10_228, policy));
        assert!(client_uid_allowed(110_228, policy));
        assert!(!client_uid_allowed(10_229, policy));
        assert_eq!(
            package_app_id_from_packages_list(packages, "missing.package"),
            None
        );
    }

    #[test]
    fn maps_every_charging_profile_to_peridots_verified_fcc_ceiling() {
        assert!(valid_charging_profile(0));
        assert!(valid_charging_profile(25));
        assert!(valid_charging_profile(33));
        assert!(valid_charging_profile(65));
        assert!(valid_charging_profile(85));
        assert!(valid_charging_profile(90));
        assert!(!valid_charging_profile(8));
        assert!(!valid_charging_profile(100));

        assert_eq!(charging_profile_fcc_ua(0), None);
        assert_eq!(charging_profile_fcc_ua(25), Some(6_111_000));
        assert_eq!(charging_profile_fcc_ua(33), Some(8_067_000));
        assert_eq!(charging_profile_fcc_ua(65), Some(15_889_000));
        assert_eq!(charging_profile_fcc_ua(85), Some(20_778_000));
        assert_eq!(charging_profile_fcc_ua(90), Some(22_000_000));
    }

    #[test]
    fn migrates_the_legacy_boost_switch_to_the_full_90_watt_profile() {
        assert_eq!(normalize_charging_profile(8), 90);
        assert_eq!(normalize_charging_profile(33), 33);
        assert_eq!(normalize_charging_profile(-1), 0);
    }

    #[test]
    fn normalizes_common_vendor_power_units_to_watts() {
        assert_eq!(normalize_reported_power_w(90), 90);
        assert_eq!(normalize_reported_power_w(90_000), 90);
        assert_eq!(normalize_reported_power_w(90_000_000), 90);
        assert_eq!(normalize_reported_power_w(0), -1);
    }

    #[test]
    fn resolves_the_loopback_client_uid_from_the_kernel_socket_table() {
        let table = concat!(
            "  sl  local_address rem_address   st tx_queue tr tm->when retrnsmt uid timeout inode\n",
            "   0: 0100007F:02DC 0100007F:C350 01 00000000:00000000 00:00000000 00000000 0 0 100\n",
            "   1: 0100007F:C350 0100007F:02DC 01 00000000:00000000 00:00000000 00000000 10321 0 101\n",
        );
        assert_eq!(tcp_client_uid_from_table(table, 732, 50_000), Some(10_321));
        assert_eq!(tcp_client_uid_from_table(table, 733, 50_000), None);
    }

    #[cfg(not(target_os = "android"))]
    #[test]
    fn bounds_external_framework_commands() {
        let output = run_process_with_timeout(
            "/bin/sh",
            &["-c", "printf peridot"],
            std::time::Duration::from_secs(1),
        )
        .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"peridot");

        let started = std::time::Instant::now();
        let error =
            run_process_with_timeout("/bin/sleep", &["1"], std::time::Duration::from_millis(25))
                .unwrap_err();
        assert!(error.contains("timed out"));
        assert!(started.elapsed() < std::time::Duration::from_millis(500));
    }

    #[test]
    fn parses_cpu_frequency_table_as_sorted_unique_mhz_opps() {
        assert_eq!(
            parse_cpu_frequency_table("2100000 2000000 1000000 2000000 bad 3250500"),
            vec![1000, 2000, 2100, 3250]
        );
    }

    #[test]
    fn parses_only_the_frequency_column_from_time_in_state() {
        assert_eq!(
            parse_cpu_time_in_state("2100000 298\n2000000 15718\n2100000 12\ninvalid\n"),
            vec![2000, 2100]
        );
    }

    #[test]
    fn accepts_only_ordered_frequencies_exposed_by_the_policy() {
        let available = [300, 400, 500, 600, 2100];
        assert!(validate_cpu_frequency_range_against(0, 400, 2100, &available).is_ok());
        assert!(validate_cpu_frequency_range_against(0, 400, 400, &available).is_ok());
        assert!(validate_cpu_frequency_range_against(0, 350, 2100, &available).is_err());
        assert!(validate_cpu_frequency_range_against(0, 2100, 400, &available).is_err());
    }

    #[test]
    fn formats_peridot_vendor_cpu_frequency_requests_per_policy() {
        assert_eq!(mi_thermal_cpu_limit_request(0, 2100), "cpu0 2100000");
        assert_eq!(mi_thermal_cpu_limit_request(4, 3000), "cpu4 3000000");
        assert_eq!(mi_thermal_cpu_limit_request(7, 3250), "cpu7 3250000");
        assert_eq!(
            mtk_powerhal_cpu_range_request(4, 400, 3000),
            "4 400000 3000000"
        );
    }

    #[test]
    fn distinguishes_saved_targets_from_live_frequency_drift() {
        assert_eq!(cpu_frequency_drift_status(-1, -1, 300, 2100), -1);
        assert_eq!(cpu_frequency_drift_status(1200, 1800, 1200, 1800), 0);
        assert_eq!(cpu_frequency_drift_status(1800, 1800, 1200, 1800), 1);
    }

    #[test]
    fn detects_both_peridot_touch_panel_families() {
        assert_eq!(classify_touch_panel_version("driver version: gt9916"), 1);
        assert_eq!(classify_touch_panel_version("Goodix GDIX algorithm"), 1);
        assert_eq!(classify_touch_panel_version("FocalTech FT3683G"), 2);
        assert_eq!(classify_touch_panel_version("unknown panel"), 0);
    }

    #[test]
    fn finds_peridot_thp_timing_block_without_a_fixed_address() {
        let mut bytes = vec![0u8; 0x80];
        let offset = 0x10;
        put_u16(&mut bytes, offset, 135);
        put_u16(&mut bytes, offset + 0x04, 135);
        put_u16(&mut bytes, offset + 0x18, 240);
        put_u16(&mut bytes, offset + 0x1c, 240);
        put_u16(&mut bytes, offset + 0x24, 240);
        put_u16(&mut bytes, offset + 0x28, 650);

        assert_eq!(find_touch_thp_config_offset(&bytes), Some(offset));
    }

    #[test]
    fn keeps_native_touch_profiles_on_distinct_vendor_calibrations() {
        let native_240 = touch_hal_profile_sequence(1).unwrap();
        let native_480 = touch_hal_profile_sequence(2).unwrap();

        assert!(native_240.contains(&(2, 99, true)));
        assert!(!native_240.contains(&(2, 4, false)));
        assert!(native_480.contains(&(2, 4, false)));
        assert!(
            !native_480
                .iter()
                .any(|&(mode, value, _)| mode == 2 && value == 99)
        );
        assert!(native_240.contains(&(202, 1, true)));
        assert!(native_480.contains(&(202, 1, true)));
    }

    #[test]
    fn scales_resolution_density_from_the_rom_native_baseline() {
        assert_eq!(scaled_display_density(520, 1220), 520);
        assert_eq!(scaled_display_density(520, 1080), 460);
        assert_eq!(scaled_display_density(520, 720), 307);
        assert_eq!(scaled_display_density(520, 1440), 614);
        assert_eq!(scaled_display_density(440, 1080), 390);
    }

    #[test]
    fn parses_peridot_ged_current_opp() {
        assert_eq!(parse_ged_current_frequency_mhz("40 260000\n"), Some(260));
        assert_eq!(parse_ged_current_frequency_mhz("0 1300000\n"), Some(1300));
    }

    #[test]
    fn accepts_single_value_frequency_units() {
        assert_eq!(parse_ged_current_frequency_mhz("260000000"), Some(260));
        assert_eq!(parse_ged_current_frequency_mhz("26"), Some(26));
        assert_eq!(parse_ged_current_frequency_mhz("unavailable"), None);
    }

    #[test]
    fn gaming_dynamic_policy_uses_the_full_opp_table() {
        assert_eq!(gaming_dynamic_target_opp(100, 40, 40), 0);
        assert_eq!(gaming_dynamic_target_opp(75, 30, 40), 22);
        assert_eq!(gaming_dynamic_target_opp(60, 30, 40), 26);
        assert_eq!(gaming_dynamic_target_opp(0, 0, 40), 40);
    }

    #[test]
    fn peridot_profiles_never_request_ged_boost() {
        assert!(!profile_uses_ged_boost(0));
        assert!(!profile_uses_ged_boost(1));
        assert!(!profile_uses_ged_boost(2));
        assert!(!profile_uses_ged_boost(3));
    }

    #[test]
    fn validates_xiaomi_thermal_config_mode_range() {
        assert!(valid_mi_thermal_config_mode(0));
        assert!(valid_mi_thermal_config_mode(MI_THERMAL_NO_LIMITS_MODE));
        assert!(valid_mi_thermal_config_mode(0x800));
        assert!(!valid_mi_thermal_config_mode(-1));
        assert!(!valid_mi_thermal_config_mode(0x801));

        assert_eq!(normalize_mi_thermal_config_mode(-1), Ok(0));
        assert_eq!(normalize_mi_thermal_config_mode(0), Ok(0));
        assert_eq!(
            normalize_mi_thermal_config_mode(MI_THERMAL_NO_LIMITS_MODE),
            Ok(MI_THERMAL_NO_LIMITS_MODE)
        );
        assert!(normalize_mi_thermal_config_mode(-2).is_err());
        assert!(normalize_mi_thermal_config_mode(0x801).is_err());
    }

    #[test]
    fn detects_only_complete_saved_cpu_ranges() {
        let stock = PersistedState::default();
        assert!(!persisted_cpu_ranges_active(&stock));

        let partial = PersistedState {
            cpu_min_freq2: 1200,
            ..PersistedState::default()
        };
        assert!(!persisted_cpu_ranges_active(&partial));

        let exact = PersistedState {
            cpu_min_freq7: 3250,
            cpu_max_freq7: 3250,
            ..PersistedState::default()
        };
        assert!(persisted_cpu_ranges_active(&exact));
    }

    #[test]
    fn migrates_only_the_cpu_signature_written_by_legacy_gpu_profiles() {
        let mut legacy = PersistedState {
            perf: 3,
            gpu_profile_cpu_isolated: 0,
            cpu0: "performance".into(),
            cpu2: "performance".into(),
            cpu7: "performance".into(),
            cpu_min_freq0: 2100,
            cpu_max_freq0: 2100,
            cpu_min_freq2: 3000,
            cpu_max_freq2: 3000,
            cpu_min_freq7: 3250,
            cpu_max_freq7: 3250,
            ..PersistedState::default()
        };

        assert!(migrate_legacy_gpu_profile_cpu_state(&mut legacy));
        assert_eq!(legacy.gpu_profile_cpu_isolated, 1);
        assert!(legacy.cpu0.is_empty());
        assert_eq!(legacy.cpu_min_freq0, -1);
        assert_eq!(legacy.cpu_max_freq7, -1);

        let mut custom = PersistedState {
            perf: 3,
            gpu_profile_cpu_isolated: 0,
            cpu0: "schedutil".into(),
            cpu2: "schedutil".into(),
            cpu7: "schedutil".into(),
            cpu_min_freq0: 600,
            cpu_max_freq0: 1800,
            cpu_min_freq2: 800,
            cpu_max_freq2: 2200,
            cpu_min_freq7: 1200,
            cpu_max_freq7: 2800,
            ..PersistedState::default()
        };

        assert!(!migrate_legacy_gpu_profile_cpu_state(&mut custom));
        assert_eq!(custom.gpu_profile_cpu_isolated, 1);
        assert_eq!(custom.cpu0, "schedutil");
        assert_eq!(custom.cpu_min_freq0, 600);
        assert_eq!(custom.cpu_max_freq7, 2800);
    }
}
