//! 启动描述：一次外部工具调用的「怎么起」。
//!
//! 这是 client 与"环境知识"的分界线。crate 只知道要起一个程序、带什么参数、
//! 带什么环境变量；**这个程序是什么、为什么是它、环境变量从哪来，全由调用方
//! 决定**。因此这里不会出现解释器布局、安装目录、包管理器之类的概念——
//! 将来换成原生 exe、换成容器、换成远端，这个结构一个字都不用改。

use std::path::PathBuf;
use std::process::{Command, Stdio};

/// 一次外部工具调用的启动参数。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Launcher {
    /// 可执行入口（绝对路径或裸命令名）。
    pub program: String,
    /// 传给入口的参数（脚本路径、子命令……）。
    pub args: Vec<String>,
    /// 追加的环境变量（键值对；调用方算好，crate 不解释语义）。
    pub env: Vec<(String, String)>,
    /// 工作目录（None = 继承当前进程）。
    pub working_dir: Option<PathBuf>,
}

impl Launcher {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            ..Self::default()
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    pub fn envs<I, K, V>(mut self, envs: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.env
            .extend(envs.into_iter().map(|(k, v)| (k.into(), v.into())));
        self
    }

    pub fn working_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    /// 拼一个可用的 `Command`：stdin/stdout 走管道（协议通道），stderr 丢弃
    /// （工具自己的诊断已通过协议回传；stderr 混进管道会污染 NDJSON 行）。
    ///
    /// stderr 为什么不留：它和 stdout 是两条流，宿主只解析 stdout。留个管道
    /// 却没人读，写满缓冲区就会把子进程阻塞死——这是最经典的假死来源。
    /// 要留诊断只能重定向到文件，M1 保持原有行为（丢弃）。
    pub fn command(&self) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        if let Some(dir) = &self.working_dir {
            cmd.current_dir(dir);
        }
        // 这个工具是控制台程序（解释器或套件包里的可执行文件都一样），从
        // 播放器（GUI 子系统）拉起时 Windows 会单独分配一个黑窗，一直杵到
        // 操作结束。CREATE_NO_WINDOW 压掉它。
        //
        // 上面的 stdio 三项都显式赋值了（管道/null），不依赖继承控制台 ——
        // 该标志恰好也会掐掉继承，所以不用额外处理。
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        cmd
    }

    /// 排障用的一行描述（不含环境变量：里面可能有用户路径，日志没必要全量抄）。
    pub fn describe(&self) -> String {
        if self.args.is_empty() {
            self.program.clone()
        } else {
            format!("{} {}", self.program, self.args.join(" "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_keeps_order_and_envs() {
        let l = Launcher::new("tool.exe")
            .arg("serve")
            .args(["--flag", "value"])
            .env("A", "1")
            .envs([("B", "2")]);
        assert_eq!(l.args, vec!["serve", "--flag", "value"]);
        assert_eq!(
            l.env,
            vec![("A".into(), "1".into()), ("B".into(), "2".into())]
        );
        assert_eq!(l.describe(), "tool.exe serve --flag value");
    }

    #[test]
    fn describe_without_args_is_bare_program() {
        assert_eq!(Launcher::new("tool").describe(), "tool");
    }

    /// 各字段落到 Command 上不 panic、顺序不变（`Command` 没有 getter，
    /// 只能验构造路径本身；真进程行为在 src-tauri 的 dev 测试里覆盖）。
    #[test]
    fn command_carries_all_fields() {
        let cmd = Launcher::new("tool")
            .args(["a", "b"])
            .envs([("K", "V")])
            .working_dir(".")
            .command();
        assert_eq!(cmd.get_program().to_string_lossy(), "tool");
        assert_eq!(
            cmd.get_args()
                .map(|s| s.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
    }
}
