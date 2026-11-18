//! Windows AppContainer launcher. No secret-handling or network client code.
#![cfg(windows)]
use std::{
    ffi::OsStr,
    fs::File,
    io,
    mem::{size_of, zeroed},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Path, PathBuf},
    ptr::{null, null_mut},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    NetworkManagement::WindowsFirewall::{
        NetworkIsolationGetAppContainerConfig, NetworkIsolationSetAppContainerConfig,
    },
    Security::{Authorization::*, Isolation::*, *},
    Storage::FileSystem::*,
    System::{
        Com::CoTaskMemFree,
        JobObjects::*,
        Memory::{GetProcessHeap, HeapFree},
        Pipes::CreatePipe,
        Threading::*,
    },
};
pub mod network_config;

fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}
fn check(ok: i32) -> io::Result<()> {
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
fn hresult(code: i32) -> io::Result<()> {
    if code < 0 {
        Err(io::Error::from_raw_os_error(code & 0xffff))
    } else {
        Ok(())
    }
}
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
struct Sid(PSID);
impl Drop for Sid {
    fn drop(&mut self) {
        unsafe {
            FreeSid(self.0);
        }
    }
}

/// A per-user identity; creation does not alter firewall or loopback exemptions.
pub struct Container {
    sid: Sid,
    proxy_network: bool,
    pub directory: PathBuf,
}
impl Container {
    pub fn open(name: &str) -> io::Result<Self> {
        let proxy_network = name == "Web3Rust.ProxyNetwork.v1";
        if !name.starts_with("Web3Rust.")
            || name.len() > 64
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        {
            return Err(io::Error::other("invalid container name"));
        }
        let name = wide(name);
        let mut raw = null_mut();
        unsafe {
            let result = CreateAppContainerProfile(
                name.as_ptr(),
                name.as_ptr(),
                name.as_ptr(),
                null(),
                0,
                &mut raw,
            );
            if result as u32 == 0x800700b7 {
                hresult(DeriveAppContainerSidFromAppContainerName(
                    name.as_ptr(),
                    &mut raw,
                ))?;
            } else {
                hresult(result)?;
            }
            let sid = Sid(raw);
            let mut text = null_mut();
            check(ConvertSidToStringSidW(sid.0, &mut text))?;
            let mut folder = null_mut();
            let result = GetAppContainerFolderPath(text, &mut folder);
            LocalFree(text.cast());
            hresult(result)?;
            let mut len = 0;
            while *folder.add(len) != 0 {
                len += 1;
            }
            let directory = PathBuf::from(String::from_utf16_lossy(std::slice::from_raw_parts(
                folder, len,
            )));
            CoTaskMemFree(folder.cast());
            Ok(Self {
                sid,
                directory,
                proxy_network,
            })
        }
    }

    /// Grant only read/execute on a caller-owned artifact. Never applied to user data.
    pub fn grant_execute(&self, path: &Path) -> io::Result<()> {
        let path = wide(path);
        unsafe {
            let mut descriptor = null_mut();
            let mut old_acl = null_mut();
            let error = GetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                &mut old_acl,
                null_mut(),
                &mut descriptor,
            );
            if error != 0 {
                return Err(io::Error::from_raw_os_error(error as i32));
            }
            let entry = EXPLICIT_ACCESS_W {
                grfAccessPermissions: FILE_GENERIC_READ | FILE_GENERIC_EXECUTE,
                grfAccessMode: GRANT_ACCESS,
                grfInheritance: 0,
                Trustee: TRUSTEE_W {
                    pMultipleTrustee: null_mut(),
                    MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
                    TrusteeForm: TRUSTEE_IS_SID,
                    TrusteeType: TRUSTEE_IS_UNKNOWN,
                    ptstrName: self.sid.0.cast(),
                },
            };
            // Normal desktop launches may read/execute these administrator-owned
            // artifacts without WRITE_DAC. Do not rewrite an already sufficient ACL.
            let mut rights = 0;
            let rights_error = GetEffectiveRightsFromAclW(old_acl, &entry.Trustee, &mut rights);
            let required = FILE_GENERIC_READ | FILE_GENERIC_EXECUTE;
            if rights_error == 0 && rights & required == required {
                LocalFree(descriptor.cast());
                return Ok(());
            }
            let mut acl = null_mut();
            let mut error = SetEntriesInAclW(1, &entry, old_acl, &mut acl);
            if error == 0 {
                error = SetNamedSecurityInfoW(
                    path.as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    null_mut(),
                    null_mut(),
                    acl,
                    null(),
                );
            }
            if !acl.is_null() {
                LocalFree(acl.cast());
            }
            LocalFree(descriptor.cast());
            if error != 0 {
                return Err(io::Error::from_raw_os_error(error as i32));
            }
        }
        Ok(())
    }

    /// Launch with zero capabilities, explicit pipe inheritance and atomic job assignment.
    pub fn spawn_offline(&self, executable: &Path, arguments: &[&str]) -> io::Result<Child> {
        self.check_no_loopback_exemption()?;
        self.spawn(executable, arguments, false, 1, None)
    }

    /// GUI and its signer share an offline identity; both remain in the broker's job.
    pub fn spawn_offline_gui(&self, executable: &Path, arguments: &[&str]) -> io::Result<Child> {
        self.check_no_loopback_exemption()?;
        self.spawn(executable, arguments, false, 4, None)
    }

    /// Public network channel only; the broker never forwards secret GUI/signer data.
    pub fn spawn_paired_gui(
        &self,
        executable: &Path,
        arguments: &[&str],
        input: File,
        output: File,
    ) -> io::Result<Child> {
        self.check_no_loopback_exemption()?;
        self.spawn(executable, arguments, false, 4, Some((input, output)))
    }

    pub fn spawn_internet(&self, executable: &Path, arguments: &[&str]) -> io::Result<Child> {
        if !self.proxy_network {
            self.check_no_loopback_exemption()?;
        }
        self.spawn(executable, arguments, true, 1, None)
    }

    /// Only the dedicated public proxy worker may reach a local proxy. Offline
    /// identities still reject every loopback exemption. Preserve other entries.
    pub fn enable_proxy_loopback(&self) -> io::Result<()> {
        if !self.proxy_network {
            return Err(io::Error::other("not a proxy network identity"));
        }
        unsafe {
            let mut count = 0;
            let mut entries = null_mut();
            let status = NetworkIsolationGetAppContainerConfig(&mut count, &mut entries);
            if status != 0 {
                return Err(io::Error::from_raw_os_error(status as i32));
            }
            let mut configured = Vec::new();
            let mut found = false;
            for index in 0..count as usize {
                let entry = *entries.add(index);
                found |= EqualSid(entry.Sid, self.sid.0) != 0;
                configured.push(entry);
            }
            let status = if found {
                0
            } else {
                configured.push(SID_AND_ATTRIBUTES {
                    Sid: self.sid.0,
                    Attributes: 0,
                });
                NetworkIsolationSetAppContainerConfig(configured.len() as u32, configured.as_ptr())
            };
            for index in 0..count as usize {
                HeapFree(GetProcessHeap(), 0, (*entries.add(index)).Sid);
            }
            if !entries.is_null() {
                HeapFree(GetProcessHeap(), 0, entries.cast());
            }
            if status != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("首次启用本机代理需要以管理员身份启动一次钱包 (Windows {status})"),
                ));
            }
        }
        Ok(())
    }

    fn check_no_loopback_exemption(&self) -> io::Result<()> {
        unsafe {
            let mut count = 0;
            let mut entries = null_mut();
            let result = NetworkIsolationGetAppContainerConfig(&mut count, &mut entries);
            if result != 0 {
                return Err(io::Error::from_raw_os_error(result as i32));
            }
            let mut exempt = false;
            for index in 0..count as usize {
                let sid = (*entries.add(index)).Sid;
                exempt |= EqualSid(sid, self.sid.0) != 0;
                HeapFree(GetProcessHeap(), 0, sid);
            }
            if !entries.is_null() {
                HeapFree(GetProcessHeap(), 0, entries.cast());
            }
            if exempt {
                return Err(io::Error::other(
                    "loopback exemption present; refusing launch",
                ));
            }
        }
        Ok(())
    }

    fn spawn(
        &self,
        executable: &Path,
        arguments: &[&str],
        internet: bool,
        process_limit: u32,
        channel: Option<(File, File)>,
    ) -> io::Result<Child> {
        if !executable.is_absolute() {
            return Err(io::Error::other("absolute executable required"));
        }
        let (child_in, child_out, parent_in, parent_out) = match channel {
            Some((input, output)) => {
                unsafe {
                    check(SetHandleInformation(
                        input.as_raw_handle(),
                        HANDLE_FLAG_INHERIT,
                        HANDLE_FLAG_INHERIT,
                    ))?;
                    check(SetHandleInformation(
                        output.as_raw_handle(),
                        HANDLE_FLAG_INHERIT,
                        HANDLE_FLAG_INHERIT,
                    ))?;
                }
                (input, output, None, None)
            }
            None => {
                let (input, parent_input) = pipe(true)?;
                let (parent_output, output) = pipe(false)?;
                (input, output, Some(parent_input), Some(parent_output))
            }
        };
        let job = unsafe { Handle(CreateJobObjectW(null(), null())) };
        if job.0.is_null() {
            return Err(io::Error::last_os_error());
        }
        unsafe {
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            limits.BasicLimitInformation.LimitFlags =
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
            limits.BasicLimitInformation.ActiveProcessLimit = process_limit;
            check(SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of_val(&limits) as u32,
            ))?;
        }
        let mut attrs = Attributes::new(3)?;
        let mut capability_sid = [0u32; 17];
        let mut sid_size = size_of_val(&capability_sid) as u32;
        if internet {
            unsafe {
                check(CreateWellKnownSid(
                    WinCapabilityInternetClientSid,
                    null_mut(),
                    capability_sid.as_mut_ptr().cast(),
                    &mut sid_size,
                ))?;
            }
        }
        let mut capability = SID_AND_ATTRIBUTES {
            Sid: capability_sid.as_mut_ptr().cast(),
            Attributes: 4, /* SE_GROUP_ENABLED */
        };
        let mut caps = SECURITY_CAPABILITIES {
            AppContainerSid: self.sid.0,
            Capabilities: if internet {
                &mut capability
            } else {
                null_mut()
            },
            CapabilityCount: u32::from(internet),
            Reserved: 0,
        };
        let mut handles = [child_in.as_raw_handle(), child_out.as_raw_handle()];
        let mut jobs = [job.0];
        attrs.set(PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, &mut caps)?;
        attrs.set(PROC_THREAD_ATTRIBUTE_HANDLE_LIST, &mut handles)?;
        attrs.set(PROC_THREAD_ATTRIBUTE_JOB_LIST, &mut jobs)?;
        let mut startup: STARTUPINFOEXW = unsafe { zeroed() };
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = handles[0];
        startup.StartupInfo.hStdOutput = handles[1];
        startup.StartupInfo.hStdError = handles[1];
        startup.lpAttributeList = attrs.pointer();
        let mut line = quote(executable.as_os_str())?;
        for argument in arguments {
            line.push(' ');
            line.push_str(&quote(OsStr::new(argument))?);
        }
        let mut line = wide(line);
        let exe = wide(executable);
        let cwd = wide(&self.directory);
        // No inherited proxy variables, credentials, diagnostic hooks or secret environment.
        let root =
            std::env::var_os("SystemRoot").ok_or_else(|| io::Error::other("SystemRoot missing"))?;
        let mut environment = Vec::new();
        for name in [
            "APPDATA",
            "LOCALAPPDATA",
            "SystemRoot",
            "USERPROFILE",
            "windir",
        ] {
            let value = std::env::var_os(name)
                .ok_or_else(|| io::Error::other(format!("missing {name}")))?;
            environment.extend(wide(format!("{name}={}", value.to_string_lossy())));
        }
        environment.extend(wide(format!("PATH={}\\System32", root.to_string_lossy())));
        environment.extend(wide(format!("TEMP={}", self.directory.display())));
        environment.extend(wide(format!("TMP={}", self.directory.display())));
        environment.push(0);
        let mut process: PROCESS_INFORMATION = unsafe { zeroed() };
        unsafe {
            check(CreateProcessW(
                exe.as_ptr(),
                line.as_mut_ptr(),
                null(),
                null(),
                1,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW,
                environment.as_ptr().cast(),
                cwd.as_ptr(),
                &startup.StartupInfo,
                &mut process,
            ))?;
        }
        let _thread = Handle(process.hThread);
        Ok(Child {
            process: Handle(process.hProcess),
            job,
            input: parent_in,
            output: parent_out,
            id: process.dwProcessId,
        })
    }
}

fn quote(argument: &OsStr) -> io::Result<String> {
    let text = argument
        .to_str()
        .ok_or_else(|| io::Error::other("argument Unicode"))?;
    if text.contains('\0') {
        return Err(io::Error::other("argument NUL"));
    }
    let mut out = String::from("\"");
    let mut slashes = 0;
    for ch in text.chars() {
        if ch == '\\' {
            slashes += 1;
            continue;
        }
        out.extend(std::iter::repeat_n(
            '\\',
            if ch == '"' { slashes * 2 + 1 } else { slashes },
        ));
        out.push(ch);
        slashes = 0;
    }
    out.extend(std::iter::repeat_n('\\', slashes * 2));
    out.push('"');
    Ok(out)
}

fn pipe(parent_writes: bool) -> io::Result<(File, File)> {
    unsafe {
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: null_mut(),
            bInheritHandle: 1,
        };
        let (mut read, mut write) = (null_mut(), null_mut());
        check(CreatePipe(&mut read, &mut write, &attributes, 4096))?;
        let read = File::from_raw_handle(read);
        let write = File::from_raw_handle(write);
        check(SetHandleInformation(
            if parent_writes {
                write.as_raw_handle()
            } else {
                read.as_raw_handle()
            },
            HANDLE_FLAG_INHERIT,
            0,
        ))?;
        Ok((read, write))
    }
}
struct Attributes {
    memory: Vec<usize>,
    initialized: bool,
}
impl Attributes {
    fn new(count: u32) -> io::Result<Self> {
        let mut bytes = 0;
        unsafe {
            InitializeProcThreadAttributeList(null_mut(), count, 0, &mut bytes);
        }
        if bytes == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut result = Self {
            memory: vec![0; bytes.div_ceil(size_of::<usize>())],
            initialized: false,
        };
        unsafe {
            check(InitializeProcThreadAttributeList(
                result.pointer(),
                count,
                0,
                &mut bytes,
            ))?;
        }
        result.initialized = true;
        Ok(result)
    }
    fn pointer(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.memory.as_mut_ptr().cast()
    }
    fn set<T>(&mut self, kind: u32, value: &mut T) -> io::Result<()> {
        unsafe {
            check(UpdateProcThreadAttribute(
                self.pointer(),
                0,
                kind as usize,
                (value as *mut T).cast(),
                size_of::<T>(),
                null_mut(),
                null(),
            ))
        }
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        unsafe {
            if self.initialized {
                DeleteProcThreadAttributeList(self.pointer());
            }
        }
    }
}

pub struct Child {
    process: Handle,
    job: Handle,
    pub input: Option<File>,
    pub output: Option<File>,
    pub id: u32,
}
impl Child {
    pub fn try_wait(&self) -> io::Result<Option<u32>> {
        match unsafe { WaitForSingleObject(self.process.0, 0) } {
            WAIT_OBJECT_0 => {
                let mut code = 0;
                unsafe {
                    check(GetExitCodeProcess(self.process.0, &mut code))?;
                }
                Ok(Some(code))
            }
            WAIT_TIMEOUT => Ok(None),
            _ => Err(io::Error::last_os_error()),
        }
    }
    pub fn wait(&self, timeout: Duration) -> io::Result<u32> {
        let deadline = Instant::now() + timeout;
        loop {
            let result = unsafe { WaitForSingleObject(self.process.0, 20) };
            if result == WAIT_OBJECT_0 {
                let mut code = 0;
                unsafe {
                    check(GetExitCodeProcess(self.process.0, &mut code))?;
                }
                return Ok(code);
            }
            if result == WAIT_FAILED {
                return Err(io::Error::last_os_error());
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "sandbox timeout"));
            }
        }
    }
}

/// One trusted launcher per Windows user, including across terminal sessions.
pub struct SingleInstance {
    _handle: Handle,
}
impl SingleInstance {
    pub fn acquire() -> io::Result<Self> {
        unsafe {
            let mut token = null_mut();
            check(OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_QUERY,
                &mut token,
            ))?;
            let token = Handle(token);
            let mut size = 0;
            GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut size);
            if size == 0 || size > 4096 {
                return Err(io::Error::other("user token size"));
            }
            let mut bytes = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
            check(GetTokenInformation(
                token.0,
                TokenUser,
                bytes.as_mut_ptr().cast(),
                size,
                &mut size,
            ))?;
            let user = &*bytes.as_ptr().cast::<TOKEN_USER>();
            let mut text = null_mut();
            check(ConvertSidToStringSidW(user.User.Sid, &mut text))?;
            let mut count = 0;
            while *text.add(count) != 0 {
                count += 1;
            }
            let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, count));
            LocalFree(text.cast());
            let name = wide(format!("Global\\Web3Rust.Wallet.{sid}"));
            Self::acquire_named(&name)
        }
    }

    fn acquire_named(name: &[u16]) -> io::Result<Self> {
        unsafe {
            // CreateMutexW requests MUTEX_ALL_ACCESS when the object exists.
            // A normal launch cannot obtain write access to an elevated owner's
            // mutex. We only keep a handle alive: SYNCHRONIZE is sufficient and
            // lets both integrity levels detect the existing instance.
            let raw = CreateMutexExW(null(), name.as_ptr(), 0, SYNCHRONIZE);
            let error = GetLastError();
            if raw.is_null() {
                return Err(io::Error::from_raw_os_error(error as i32));
            }
            let handle = Handle(raw);
            if error == ERROR_ALREADY_EXISTS {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "wallet already running for this user",
                ));
            }
            Ok(Self { _handle: handle })
        }
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        unsafe {
            TerminateJobObject(self.job.0, 2);
            WaitForSingleObject(self.process.0, 5000);
        }
    }
}

/// Check the actual process token; absence of a capability is not inferred from config.
pub fn verify_offline_token() -> io::Result<()> {
    verify_token(false)
}
pub fn verify_internet_token() -> io::Result<()> {
    verify_token(true)
}
fn verify_token(internet: bool) -> io::Result<()> {
    unsafe {
        let mut token = null_mut();
        check(OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY,
            &mut token,
        ))?;
        let token = Handle(token);
        let mut present: u32 = 0;
        let mut needed = 0;
        check(GetTokenInformation(
            token.0,
            TokenIsAppContainer,
            (&mut present as *mut u32).cast(),
            4,
            &mut needed,
        ))?;
        if present != 1 {
            return Err(io::Error::other("not AppContainer"));
        }
        GetTokenInformation(token.0, TokenCapabilities, null_mut(), 0, &mut needed);
        if !(4..=4096).contains(&needed) {
            return Err(io::Error::other("capability token size"));
        }
        let mut buffer = vec![
            0usize;
            (needed as usize)
                .max(size_of::<TOKEN_GROUPS>())
                .div_ceil(size_of::<usize>())
        ];
        check(GetTokenInformation(
            token.0,
            TokenCapabilities,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        ))?;
        let groups = &*buffer.as_ptr().cast::<TOKEN_GROUPS>();
        if groups.GroupCount != u32::from(internet) {
            return Err(io::Error::other("unexpected capabilities"));
        }
        if internet {
            let mut sid = [0u32; 17];
            let mut size = size_of_val(&sid) as u32;
            check(CreateWellKnownSid(
                WinCapabilityInternetClientSid,
                null_mut(),
                sid.as_mut_ptr().cast(),
                &mut size,
            ))?;
            if EqualSid(groups.Groups[0].Sid, sid.as_mut_ptr().cast()) == 0
                || groups.Groups[0].Attributes & 4 == 0
            {
                return Err(io::Error::other("wrong network capability"));
            }
        }
    }
    Ok(())
}

/// Derive storage from the OS token, not from caller-supplied paths or environment.
pub fn current_container_directory() -> io::Result<PathBuf> {
    unsafe {
        let mut token = null_mut();
        check(OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY,
            &mut token,
        ))?;
        let token = Handle(token);
        let mut needed = 0;
        GetTokenInformation(token.0, TokenAppContainerSid, null_mut(), 0, &mut needed);
        if needed == 0 || needed > 4096 {
            return Err(io::Error::other("invalid token size"));
        }
        let mut bytes = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
        check(GetTokenInformation(
            token.0,
            TokenAppContainerSid,
            bytes.as_mut_ptr().cast(),
            needed,
            &mut needed,
        ))?;
        let info = &*bytes.as_ptr().cast::<TOKEN_APPCONTAINER_INFORMATION>();
        if info.TokenAppContainer.is_null() {
            return Err(io::Error::other("no container SID"));
        }
        let mut text = null_mut();
        check(ConvertSidToStringSidW(info.TokenAppContainer, &mut text))?;
        let mut folder = null_mut();
        let result = GetAppContainerFolderPath(text, &mut folder);
        LocalFree(text.cast());
        hresult(result)?;
        let mut len = 0;
        while *folder.add(len) != 0 {
            len += 1;
        }
        let path = PathBuf::from(String::from_utf16_lossy(std::slice::from_raw_parts(
            folder, len,
        )));
        CoTaskMemFree(folder.cast());
        Ok(path)
    }
}

#[cfg(test)]
mod single_instance_tests {
    use super::*;

    #[test]
    fn duplicate_needs_only_synchronize_and_release_allows_restart() {
        let name = wide(format!("Local\\Web3Rust.InstanceTest.{}", std::process::id()));
        unsafe {
            // Reproduce an existing object that permits detection, but denies
            // the unnecessary full-access request made by the previous code.
            let sddl = wide("D:(A;;0x00100000;;;WD)");
            let mut descriptor = null_mut();
            check(ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(), 1, &mut descriptor, null_mut(),
            )).unwrap();
            let attributes = SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor,
                bInheritHandle: 0,
            };
            let raw = CreateMutexExW(&attributes, name.as_ptr(), 0, SYNCHRONIZE);
            let error = GetLastError();
            LocalFree(descriptor);
            assert!(!raw.is_null(), "create fixture: {error}");
            let holder = Handle(raw);
            let legacy = CreateMutexW(null(), 0, name.as_ptr());
            let legacy_error = GetLastError();
            if !legacy.is_null() { CloseHandle(legacy); }
            assert!(legacy.is_null());
            assert_eq!(legacy_error, ERROR_ACCESS_DENIED);
            assert!(matches!(SingleInstance::acquire_named(&name), Err(e) if e.kind() == io::ErrorKind::AlreadyExists));
            drop(holder);
        }
        let first = SingleInstance::acquire_named(&name).unwrap();
        assert!(matches!(SingleInstance::acquire_named(&name), Err(e) if e.kind() == io::ErrorKind::AlreadyExists));
        drop(first);
        assert!(SingleInstance::acquire_named(&name).is_ok());
    }
}
