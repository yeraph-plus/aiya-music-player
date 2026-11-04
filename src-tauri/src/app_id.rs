//! 应用身份（AppUserModelID）。
//!
//! ## 为什么需要这一层
//!
//! Windows 的系统媒体浮层（音量键弹出的那块）、任务栏分组、跳转列表、通知，
//! 都靠 AUMID 把"一个正在跑的进程"和"一个应用身份"对上号。没有身份时壳层
//! 只能写「未知应用」。
//!
//! 一个容易走错方向的直觉：**exe 的 FileDescription 不管用**。我们实测过——
//! `aiya.exe` 的 FileDescription 本来就是 `AIYA Music`（见 VersionInfo），
//! 浮层照样显示未知应用。原因是壳层不读版本资源，它读的是身份。
//!
//! ## 微软要求标两处，缺一不可
//!
//! 1. **进程级**：`SetCurrentProcessExplicitAppUserModelID`，且必须在
//!    "呈现任何 UI / 操作跳转列表 / 触发 SHAddToRecentDocs" **之前**调用。
//!    这就是它放在 `run()` 最前面而不是 setup 里的原因。
//! 2. **快捷方式属性** `System.AppUserModel.ID`：壳层靠它把 AUMID 解析成
//!    显示名和图标。这一半归安装器管；便携版没有安装器，所以这里额外写一份
//!    `HKCU\Software\Classes\AppUserModelId\<AUMID>`（DisplayName + IconUri）
//!    作为"没有快捷方式的应用"的登记口。
//!
//! ## 为什么登记写在运行期而不是安装期
//!
//! 便携版的判定是"exe 同目录有 portable.txt"（`paths.rs`），它**从不经过
//! NSIS**。身份登记若只写在安装器里，便携版就永远拿不到名字。运行期写的好处
//! 是两条发布轨道走同一段代码；代价是卸载时不会自动清掉这条键（它只是
//! per-user 的一个类键，约 100 字节，不改变任何行为）。

use std::sync::OnceLock;

/// 进程级 AUMID。
///
/// 命名遵循微软文档的三条硬性要求：pascal 分段、不超过 128 字符、不含空格。
/// **不带版本段**——带上就等于升级后变成"另一个应用"，用户固定的任务栏项、
/// 媒体会话历史会全部失联（文档明确要求升级后沿用同一个）。
pub const AUMID: &str = "AIYA.Music.Desktop";

/// 系统界面里显示的名字（浮层标题上方那行、通知的应用名）。
pub const DISPLAY_NAME: &str = "AIYA Music";

/// 一次安装尝试的结果。启动时算一次，日志初始化之后再打出来
/// （install 必须早于日志，因为日志晚于 UI 之前还有别的准备工作）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// AUMID 已生效。`readback` 是回读到的值（不是"没报错"就算成功）；
    /// `registered` 表示身份登记（DisplayName/IconUri）写成功。
    Applied { readback: String, registered: bool },
    /// 调用返回成功，但回读到的不是我们要的值。
    Mismatch { got: String },
    /// 调用返回失败（HRESULT）。
    Failed { hresult: i32 },
    /// 非 Windows：没有这个概念。
    /// 在 Windows 上永远构造不出来，但保留它可以让上层的 match 保持穷尽
    /// （而不是让平台差异爬上业务代码）。
    #[cfg_attr(windows, allow(dead_code))]
    Unsupported,
}

static OUTCOME: OnceLock<Outcome> = OnceLock::new();

/// 快捷方式 AUMID 自愈的结果摘要（log_outcome 时打出来）。
static SHORTCUT_HEAL: OnceLock<&'static str> = OnceLock::new();

/// 设置应用身份。幂等；**必须在任何 UI 出现之前**调用。
pub fn install() -> &'static Outcome {
    OUTCOME.get_or_init(imp::install)
}

/// 把结果写进日志。分两步是因为日志初始化在 `setup` 里，晚于 `install`。
pub fn log_outcome() {
    match install() {
        Outcome::Applied {
            readback,
            registered,
        } => tracing::info!(
            aumid = %readback,
            display_name = DISPLAY_NAME,
            registry = registered,
            shortcut_heal = SHORTCUT_HEAL.get().copied().unwrap_or("skipped"),
            "应用身份已设置"
        ),
        Outcome::Mismatch { got } => tracing::warn!(
            want = AUMID,
            got = %got,
            "应用身份回读不一致：系统浮层仍可能显示「未知应用」"
        ),
        Outcome::Failed { hresult } => tracing::warn!(
            aumid = AUMID,
            hresult = %format!("0x{hresult:08X}"),
            "应用身份设置失败：系统浮层会显示「未知应用」"
        ),
        Outcome::Unsupported => {}
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_WRITE,
        REG_OPTION_NON_VOLATILE, REG_SZ,
    };
    use windows_sys::Win32::UI::Shell::{
        GetCurrentProcessExplicitAppUserModelID, SetCurrentProcessExplicitAppUserModelID,
    };

    pub fn install() -> Outcome {
        let id = wide(AUMID);
        let hr = unsafe { SetCurrentProcessExplicitAppUserModelID(id.as_ptr()) };
        if hr < 0 {
            return Outcome::Failed { hresult: hr };
        }
        // 回读：Fire-and-forget 的 API 很多都是"没报错但没生效"，比如在 UI
        // 之后调用。日志里能看到真实值，判断"设了没"不用靠猜。
        let readback = read_back();
        if readback != AUMID {
            return Outcome::Mismatch { got: readback };
        }
        let registered = register();
        let _ = SHORTCUT_HEAL.set(heal_shortcut_aumid());
        Outcome::Applied {
            readback,
            registered,
        }
    }

    /// 写 `HKCU\Software\Classes\AppUserModelId\<AUMID>` 的 DisplayName / IconUri。
    ///
    /// 失败不算错：没有快捷方式的老系统上这条可能不生效，但进程级 AUMID 已经
    /// 让任务栏分组和跳转列表正常了，不值得为它中断启动。
    fn register() -> bool {
        let sub = wide(&format!("Software\\Classes\\AppUserModelId\\{AUMID}"));
        let mut hkey: HKEY = std::ptr::null_mut();
        let mut disposition = 0u32;
        let rc = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                sub.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                std::ptr::null(),
                &mut hkey,
                &mut disposition,
            )
        };
        if rc != 0 {
            return false;
        }
        let name_ok = set_string(hkey, "DisplayName", DISPLAY_NAME);
        // IconUri 用 exe 自身：便携版换个目录也对得上。
        let icon_ok = std::env::current_exe()
            .map(|p| set_string(hkey, "IconUri", &p.to_string_lossy()))
            .unwrap_or(false);
        unsafe { RegCloseKey(hkey) };
        name_ok && icon_ok
    }

    fn set_string(hkey: HKEY, name: &str, value: &str) -> bool {
        let name_w = wide(name);
        let value_w = wide(value);
        let bytes = (value_w.len() * std::mem::size_of::<u16>()) as u32;
        let rc = unsafe {
            RegSetValueExW(
                hkey,
                name_w.as_ptr(),
                0,
                REG_SZ,
                value_w.as_ptr() as *const u8,
                bytes,
            )
        };
        rc == 0
    }

    fn read_back() -> String {
        let mut p: *mut u16 = std::ptr::null_mut();
        let hr = unsafe { GetCurrentProcessExplicitAppUserModelID(&mut p) };
        if hr < 0 || p.is_null() {
            return String::new();
        }
        let out = unsafe { copy_wide_to_string(p as *const u16) };
        // 这个字符串是 CoTaskMemAlloc 出来的，文档要求调用方释放。
        unsafe { CoTaskMemFree(p as *const core::ffi::c_void) };
        out
    }

    /// 读到 NUL 为止并拷贝。返回前不释放——释放是调用方的事。
    unsafe fn copy_wide_to_string(p: *const u16) -> String {
        let mut len = 0usize;
        while unsafe { *p.add(len) } != 0 {
            len += 1;
        }
        String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(p, len) })
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// 快捷方式 AUMID 自愈：对已存在的开始菜单快捷方式补写
    /// `System.AppUserModel.ID` 属性。
    ///
    /// 为什么需要：壳层解析媒体浮层/任务栏的应用名，认的是**快捷方式上的
    /// 这个属性**——进程级 AUMID 与注册表登记（上面两步）都补不了它。NSIS
    /// 的 `CreateShortCut` 不写属性，所以装出来的版本浮层显示「未知应用」
    /// （docs/35 P2-15 的判据分支被实测踩中）。
    ///
    /// 为什么在运行期做而不是 NSIS hook：属性写入要 IPropertyStore COM 调用，
    /// NSIS 里得手拼 vtable 或引插件；在 Rust 里写有类型、有日志、可测试，
    /// 且升级重装出新的无属性快捷方式时能再次自愈。便携版没有快捷方式，
    /// 候选路径都不存在 → no-op（「未知应用」是便携版的已知取舍，见 P2-15）。
    ///
    /// 只对**已经存在**的 .lnk 补属性，绝不创建文件；失败一律 warn 不中断。
    fn heal_shortcut_aumid() -> &'static str {
        use windows::core::{Interface, PCWSTR};
        use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
        use windows::Win32::System::Com::StructuredStorage::{
            PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
        };
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CoTaskMemFree, IPersistFile, CLSCTX_INPROC_SERVER,
            COINIT_APARTMENTTHREADED, STGM_READ,
        };
        use windows::Win32::System::Variant::VT_LPWSTR;
        use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
        use windows::Win32::UI::Shell::{
            FOLDERID_CommonStartMenu, FOLDERID_StartMenu, IShellLinkW, SHGetKnownFolderPath,
            KNOWN_FOLDER_FLAG,
        };
        // ShellLink 的 CLSID（{00021401-...}）windows 0.62 没有常量，就地定义。
        const CLSID_SHELL_LINK: windows::core::GUID =
            windows::core::GUID::from_u128(0x00021401_0000_0000_c000_000000000046);

        // 主线程多半已被 tao 初始化为 STA；重复初始化返回 S_FALSE 也无害。
        // 不配对 CoUninitialize：进程生命周期与 COM 相同，省掉配对账目。
        let _com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };

        // 候选 = 两种开始菜单（当前用户 / 所有用户）下的 Programs\<产品名>.lnk。
        // installMode 现为 currentUser，Common 一支是给将来换档留的。
        let mut candidates: Vec<String> = Vec::new();
        for folder in [FOLDERID_StartMenu, FOLDERID_CommonStartMenu] {
            let base = unsafe { SHGetKnownFolderPath(&folder, KNOWN_FOLDER_FLAG(0), None) };
            let base = match base {
                Ok(p) => {
                    let s = String::from_utf16_lossy(unsafe { wide_ptr_to_slice(p.as_ptr()) });
                    unsafe { CoTaskMemFree(Some(p.as_ptr() as *const _)) };
                    if s.is_empty() {
                        continue;
                    }
                    s
                }
                Err(_) => continue,
            };
            candidates.push(format!("{base}\\Programs\\{DISPLAY_NAME}.lnk"));
        }

        for path in &candidates {
            let path_w = wide(path);
            if !std::path::Path::new(path).is_file() {
                continue;
            }
            let result = (|| -> Result<(), windows::core::Error> {
                unsafe {
                    let link: IShellLinkW =
                        CoCreateInstance(&CLSID_SHELL_LINK, None, CLSCTX_INPROC_SERVER)?;
                    let persist: IPersistFile = link.cast()?;
                    persist.Load(PCWSTR(path_w.as_ptr()), STGM_READ)?;
                    let store: IPropertyStore = link.cast()?;

                    // 已是目标值就跳过写盘：保持幂等静默，不碰修改时间。
                    let current = store.GetValue(&PKEY_AppUserModel_ID)?;
                    if current.Anonymous.Anonymous.vt == VT_LPWSTR
                        && !current.Anonymous.Anonymous.Anonymous.pwszVal.is_null()
                    {
                        let have = String::from_utf16_lossy(wide_ptr_to_slice(
                            current.Anonymous.Anonymous.Anonymous.pwszVal.as_ptr(),
                        ));
                        CoTaskMemFree(Some(
                            current.Anonymous.Anonymous.Anonymous.pwszVal.as_ptr() as *const _
                        ));
                        if have == AUMID {
                            return Ok(());
                        }
                    }

                    // IPropertyStore::SetValue 按契约**拷贝**值，调用方保留
                    // 所有权——value 在 Commit 后正常 drop，不 forget 也不需要
                    // PropVariantClear（缓冲区是我们自己分配的，不是 COM 的）。
                    let value = wide(AUMID);
                    // 中间层在 windows 0.62 里是 ManuallyDrop 包裹（union 字段）。
                    let pv = PROPVARIANT {
                        Anonymous: PROPVARIANT_0 {
                            Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                                vt: VT_LPWSTR,
                                wReserved1: 0,
                                wReserved2: 0,
                                wReserved3: 0,
                                Anonymous: PROPVARIANT_0_0_0 {
                                    pwszVal: windows::core::PWSTR(value.as_ptr() as *mut _),
                                },
                            }),
                        },
                    };
                    store.SetValue(&PKEY_AppUserModel_ID, &pv)?;
                    store.Commit()?;
                    drop(pv);
                    Ok(())
                }
            })();
            match result {
                Ok(()) => return "fixed",
                Err(e) => {
                    tracing::warn!(path = %path, error = %e, "快捷方式 AUMID 写入失败");
                }
            }
        }
        if candidates.iter().any(|p| std::path::Path::new(p).is_file()) {
            "failed"
        } else {
            "no-shortcut"
        }
    }

    /// 读 NUL 结尾的 UTF-16（属性值取回用）。
    unsafe fn wide_ptr_to_slice<'a>(p: *const u16) -> &'a [u16] {
        let mut len = 0usize;
        while unsafe { *p.add(len) } != 0 {
            len += 1;
        }
        unsafe { std::slice::from_raw_parts(p, len) }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub fn install() -> Outcome {
        Outcome::Unsupported
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 微软对 AUMID 的格式要求是硬性的：超长、带空格、空分段都会被壳层拒绝
    /// ——而且拒绝是静默的（回读能拿到，解析照样失败），所以只能在这里挡。
    #[test]
    fn aumid_follows_the_documented_shape() {
        assert!(
            AUMID.len() <= 128,
            "AUMID 不得超过 128 字符：{}",
            AUMID.len()
        );
        assert!(!AUMID.contains(' '), "AUMID 不得含空格：{AUMID}");
        let parts: Vec<&str> = AUMID.split('.').collect();
        assert!(parts.len() >= 2, "至少要 CompanyName.ProductName：{AUMID}");
        for part in parts {
            assert!(!part.is_empty(), "不得有空分段：{AUMID}");
            let first = part.chars().next().unwrap();
            assert!(first.is_ascii_uppercase(), "每段应 pascal-cased：{part}");
            assert!(
                part.chars().all(|c| c.is_ascii_alphanumeric()),
                "只允许字母数字：{part}"
            );
        }
        assert!(!DISPLAY_NAME.is_empty());
    }

    /// 回读必须是我们要的值。只断言"没返回错误"是没用的——在 UI 之后调用
    /// 同样返回 S_OK，但值不生效。
    #[cfg(windows)]
    #[test]
    fn install_reads_back_the_same_id() {
        match install() {
            Outcome::Applied { readback, .. } => assert_eq!(readback, AUMID),
            other => panic!("应用身份未生效：{other:?}"),
        }
    }
}
