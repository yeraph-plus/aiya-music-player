//! MMCSS（Multimedia Class Scheduler Service）音频线程优先级。
//!
//! 为什么必须有：Windows 默认把普通线程的调度优先级压得很低，后台有几个
//! 编译任务或杀毒扫描时，渲染线程拿不到 CPU 就是**欠载 → 爆音/咔哒**。
//! 主流播放器（foobar2000、WASAPI 官方示例、所有 ASIO 宿主）都注册
//! "Pro Audio" 任务，这不是优化项，是播放器能不能稳住的基本盘。
//!
//! 这个调用只影响**当前线程**的调度类别，不提升进程权限；失败（某些精简版
//! 系统关闭了 MMCSS 服务）不是致命错误——退化成普通线程，照常出声，只记日志。

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::Threading::{
        AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW,
    };

    /// 已注册的 MMCSS 任务。Drop 时自动退出任务（否则线程退出后计数泄漏）。
    pub struct Registration {
        handle: HANDLE,
    }

    impl Drop for Registration {
        fn drop(&mut self) {
            // SAFETY: handle 来自 AvSetMmThreadCharacteristicsW，且只在此处消费一次
            unsafe {
                AvRevertMmThreadCharacteristics(self.handle);
            }
        }
    }

    /// 把当前线程注册进 "Pro Audio" 任务组。失败返回 `None`（调用方只记日志）。
    pub fn register(task: &str) -> Option<Registration> {
        // MMCSS 的 task name 必须是宽字符 + NUL 结尾
        let mut wide: Vec<u16> = task.encode_utf16().collect();
        wide.push(0);
        let mut index: u32 = 0;
        // SAFETY: wide 以 NUL 结尾且在调用期间存活；index 是合法的可写指针
        let handle = unsafe { AvSetMmThreadCharacteristicsW(wide.as_ptr(), &mut index) };
        // HANDLE 是 *mut c_void：NULL 表示失败
        if handle.is_null() {
            return None;
        }
        Some(Registration { handle })
    }
}

#[cfg(not(windows))]
mod imp {
    pub struct Registration;
    pub fn register(_task: &str) -> Option<Registration> {
        None
    }
}

pub use imp::{register, Registration};

#[cfg(test)]
mod tests {
    /// 注册要么成功、要么干净失败，绝不 panic（精简系统上 MMCSS 服务可能不存在）。
    #[test]
    fn register_is_fallible_not_fatal() {
        let r = super::register("Pro Audio");
        // 本机（Windows）通常成功；非 Windows 恒 None。两者都算通过。
        if cfg!(windows) {
            // 不断言必成功：CI 容器里 MMCSS 服务常被裁掉
            let _ = r;
        } else {
            assert!(r.is_none());
        }
    }
}
