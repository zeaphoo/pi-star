//! `pi-star` 子进程的启动、事件解析、健康确认、崩溃重启与退出清理。
//!
//! GTK 只允许主线程创建/操作窗口，因此所有涉及窗口的公开入口都把动作投递到主线程
//! （`dispatch`）；内部以 `*_now` 结尾的方法只在主线程调用。

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::menu::MenuItem;
use tauri::{AppHandle, Manager, Wry};

use crate::config::{self, AppConfig, Health};
use crate::window;

/// 托盘图标 id。
pub const TRAY_ID: &str = "pi-star";
const MAIN: &str = "main";
const MAX_RESTARTS: u32 = 5;
/// 从 spawn 到主窗口加载完成的总期限。
pub const STARTUP_DEADLINE: Duration = Duration::from_secs(20);
const LOG_ROTATE_BYTES: u64 = 5 * 1024 * 1024;

/// 当前子进程 pid，供信号处理器同步清理。
static CHILD_PID: AtomicI32 = AtomicI32::new(0);

#[cfg(unix)]
pub fn install_signal_handlers() {
    extern "C" fn handle(_signal: libc::c_int) {
        let pid = CHILD_PID.load(Ordering::SeqCst);
        if pid > 0 {
            // kill / _exit 均为 async-signal-safe。
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
        unsafe { libc::_exit(0) }
    }

    unsafe {
        for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
            libc::signal(signal, handle as *const () as libc::sighandler_t);
        }
    }
}

#[cfg(not(unix))]
pub fn install_signal_handlers() {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Starting,
    Running,
    #[allow(dead_code)]
    Stopped,
    Failed,
}

impl State {
    fn label(self, addr: Option<&str>) -> String {
        match self {
            State::Starting => "启动中…".to_string(),
            State::Running => match addr {
                Some(addr) => format!("运行中 · {addr}"),
                None => "运行中".to_string(),
            },
            State::Stopped => "已停止".to_string(),
            State::Failed => "异常".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    MissingBinary,
    NotExecutable,
    SpawnFailed,
    PortConflict,
    PortForbidden,
    ForeignServer,
    UiMissing,
    Crash,
    Timeout,
    LoadTimeout,
}

impl FailureKind {
    pub fn copy(self) -> (&'static str, &'static str) {
        match self {
            Self::MissingBinary => (
                "安装可能不完整",
                "找不到随包分发的 pi-star。常见原因：安装包不完整、解压时漏了文件、被杀毒软件隔离。请重新安装，或检查下方已尝试的路径。",
            ),
            Self::NotExecutable => (
                "pi-star 无法运行",
                "内置的 pi-star 缺少执行权限，通常是解压方式导致。请重新安装。",
            ),
            Self::SpawnFailed => ("服务启动失败", "系统拒绝启动 pi-star，详情见下方。"),
            Self::PortConflict => (
                "端口被占用",
                "监听端口已被其他进程占用。请关闭占用该端口的程序后重试；若占用者是另一个 pi-star，请先退出它，或在 desktop.toml 中修改 port。",
            ),
            Self::PortForbidden => (
                "没有权限使用该端口",
                "1000 以下的端口需要特权。请在 desktop.toml 中改用 1024 以上的端口。",
            ),
            Self::ForeignServer => (
                "端口已被其他程序占用",
                "健康检查没有确认到 pi-star。桌面壳不会加载陌生页面；请释放该端口或修改 desktop.toml 中的 port。",
            ),
            Self::UiMissing => (
                "前端资源缺失",
                "pi-star 已启动，但内置 UI 为空。请重新构建安装包：先执行 `npm run build`，再编译 pi-star。",
            ),
            Self::Crash => ("Pi Star 已停止", "pi-star 进程异常退出，日志末尾见下方。"),
            Self::Timeout => (
                "服务没有按时就绪",
                "等待 20 秒仍未完成监听和健康检查，日志末尾见下方。",
            ),
            Self::LoadTimeout => (
                "页面加载超时",
                "pi-star 已就绪，但主窗口在 15 秒内没有加载完成。请重试，或查看日志。",
            ),
        }
    }
}

/// 传给 splash 的状态快照。
#[derive(Clone)]
pub struct StatusView {
    pub status: String,
    pub hint: Option<String>,
    pub detail: Option<String>,
    pub failed: bool,
    pub log_path: Option<String>,
}

struct Inner {
    state: State,
    status: String,
    detail: Option<String>,
    failure: Option<FailureKind>,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    #[cfg(windows)]
    job: Option<JobHandle>,
    generation: u64,
    restarts: u32,
    started_at: Instant,
    addr: Option<String>,
    assets_files: Option<usize>,
}

pub struct Supervisor {
    config: AppConfig,
    bin: Result<PathBuf, String>,
    status_item: MenuItem<Wry>,
    password: Option<String>,
    quitting: AtomicBool,
    main_ready: AtomicBool,
    inner: Mutex<Inner>,
}

impl Supervisor {
    pub fn new(
        config: AppConfig,
        bin: Result<PathBuf, String>,
        status_item: MenuItem<Wry>,
    ) -> Arc<Self> {
        let password = std::env::var("PI_WEB_PASSWORD")
            .ok()
            .filter(|value| !value.is_empty());
        Arc::new(Self {
            config,
            bin,
            status_item,
            password,
            quitting: AtomicBool::new(false),
            main_ready: AtomicBool::new(false),
            inner: Mutex::new(Inner {
                state: State::Starting,
                status: "正在启动 Pi Star…".to_string(),
                detail: None,
                failure: None,
                child: None,
                stdin: None,
                #[cfg(windows)]
                job: None,
                generation: 0,
                restarts: 0,
                started_at: Instant::now(),
                addr: None,
                assets_files: None,
            }),
        })
    }

    pub fn log_path(&self) -> PathBuf {
        self.config.log_path()
    }

    /// 启动后剩余的总期限。健康确认通常只花几百毫秒，剩余时间都留给页面加载。
    pub fn startup_remaining(&self) -> Duration {
        let elapsed = self.lock().started_at.elapsed();
        STARTUP_DEADLINE.saturating_sub(elapsed)
    }

    pub fn start(self: &Arc<Self>, app: &AppHandle) {
        self.start_checked(app, None);
    }

    fn start_checked(self: &Arc<Self>, app: &AppHandle, expected: Option<u64>) {
        self.dispatch(app, move |supervisor, app| {
            supervisor.start_now(app, expected);
        });
    }

    pub fn restart(self: &Arc<Self>, app: &AppHandle) {
        self.dispatch(app, |supervisor, app| {
            supervisor.kill_child();
            supervisor.lock().restarts = 0;
            supervisor.start_now(app, None);
        });
    }

    pub fn open_console(self: &Arc<Self>, app: &AppHandle) {
        self.dispatch(app, |supervisor, app| supervisor.open_console_now(app));
    }

    pub fn shutdown(&self) {
        self.quitting.store(true, Ordering::SeqCst);
        self.kill_child();
    }

    /// 子进程意外退出/超时后由监控线程调用。
    pub fn fail(self: &Arc<Self>, app: &AppHandle, kind: FailureKind, detail: String) {
        self.dispatch(app, move |supervisor, app| {
            supervisor.fail_now(app, kind, detail);
        });
    }

    pub fn mark_main_ready(&self) {
        self.main_ready.store(true, Ordering::SeqCst);
    }

    pub fn main_ready(&self) -> bool {
        self.main_ready.load(Ordering::SeqCst)
    }

    pub fn status_view(&self) -> StatusView {
        let inner = self.lock();
        let hint = inner.failure.map(|kind| kind.copy().1.to_string());
        StatusView {
            status: inner.status.clone(),
            hint,
            detail: inner.detail.clone(),
            failed: inner.state == State::Failed,
            log_path: inner
                .failure
                .map(|_| self.config.log_path().display().to_string()),
        }
    }

    pub fn apply_status_to(&self, window: &tauri::WebviewWindow) {
        window::inject_status(window, &self.status_view());
    }

    // ---------------------------------------------------------------- 主线程实现

    fn dispatch<F>(self: &Arc<Self>, app: &AppHandle, action: F)
    where
        F: FnOnce(&Arc<Self>, &AppHandle) + Send + 'static,
    {
        let supervisor = Arc::clone(self);
        let handle = app.clone();
        if let Err(error) = app.run_on_main_thread(move || action(&supervisor, &handle)) {
            eprintln!("投递主线程任务失败：{error}");
        }
    }

    fn start_now(self: &Arc<Self>, app: &AppHandle, expected: Option<u64>) {
        if self.quitting.load(Ordering::SeqCst) {
            return;
        }
        if let Some(expected) = expected {
            let inner = self.lock();
            if inner.generation != expected || inner.child.is_some() {
                return;
            }
        }
        if let Some(window) = app.get_webview_window(MAIN) {
            let _ = window.close();
        }
        self.main_ready.store(false, Ordering::SeqCst);
        self.quitting.store(false, Ordering::SeqCst);

        if let Some(url) = self.config.dev_url.clone() {
            self.set_running("运行中 · 开发服务器", Some(url), None);
            self.update_ui(app);
            if !self.config.hidden {
                self.open_console_now(app);
            }
            return;
        }

        if !self.config.hidden || self.lock().failure.is_some() {
            window::ensure_splash(app, self);
        }

        let spawned = self.spawn_child();
        let (child, stdin, stdout) = match spawned {
            Ok(value) => value,
            Err((kind, message)) => {
                self.fail_now(app, kind, message);
                return;
            }
        };

        let generation = {
            let mut inner = self.lock();
            inner.generation += 1;
            inner.state = State::Starting;
            inner.status = "正在启动 Pi Star…".to_string();
            inner.detail = None;
            inner.failure = None;
            inner.child = Some(child);
            inner.stdin = Some(stdin);
            inner.addr = None;
            inner.assets_files = None;
            inner.started_at = Instant::now();
            inner.generation
        };

        if let Some(stdout) = stdout {
            self.spawn_reader(stdout, app.clone(), generation);
        }

        self.update_ui(app);
        self.monitor(app.clone(), generation);
    }

    fn open_console_now(self: &Arc<Self>, app: &AppHandle) {
        if let Some(window) = app.get_webview_window(MAIN) {
            window::bring_to_front(&window);
            return;
        }

        let target = if let Some(url) = self.config.dev_url.clone() {
            url
        } else {
            let (state, addr) = {
                let inner = self.lock();
                (inner.state, inner.addr.clone())
            };
            match (state, addr) {
                (State::Running, Some(addr)) => format!("http://{addr}/"),
                _ => {
                    window::ensure_splash(app, self);
                    self.update_ui(app);
                    return;
                }
            }
        };

        window::open_main(app, self, &target);
    }

    fn spawn_child(
        &self,
    ) -> Result<(Child, ChildStdin, Option<ChildStdout>), (FailureKind, String)> {
        let bin = self
            .bin
            .as_ref()
            .map_err(|error| (FailureKind::MissingBinary, error.clone()))?;
        let log_path = self.config.log_path();
        rotate_log(&log_path);
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(|error| {
                (
                    FailureKind::SpawnFailed,
                    format!("打开日志 {} 失败：{error}", log_path.display()),
                )
            })?;
        let log_err = log.try_clone().map_err(|error| {
            (
                FailureKind::SpawnFailed,
                format!("复制日志句柄失败：{error}"),
            )
        })?;

        let mut command = Command::new(bin);
        command
            .arg("--hostname")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(self.config.port.to_string())
            .arg("--exit-on-stdin-eof")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(log_err));

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            unsafe {
                command.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
        }

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = command.spawn().map_err(|error| {
            (
                classify_spawn_error(error.kind()),
                format!("启动 {} 失败：{error}", bin.display()),
            )
        })?;
        let stdout = child.stdout.take();
        let stdin = child.stdin.take().ok_or_else(|| {
            (
                FailureKind::SpawnFailed,
                "无法取得 pi-star 的 stdin 管道".to_string(),
            )
        })?;
        CHILD_PID.store(child.id() as i32, Ordering::SeqCst);

        #[cfg(windows)]
        {
            let job = unsafe { assign_to_job(&child) };
            let mut inner = self.lock();
            inner.job = job;
        }

        Ok((child, stdin, stdout))
    }

    fn spawn_reader(self: &Arc<Self>, stdout: ChildStdout, app: AppHandle, generation: u64) {
        let supervisor = Arc::clone(self);
        let log_path = self.config.log_path();
        std::thread::spawn(move || {
            let mut log = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
                .ok();
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(file) = log.as_mut() {
                    let _ = writeln!(file, "{line}");
                }
                supervisor.handle_event(&app, generation, &line);
            }
        });
    }

    fn handle_event(self: &Arc<Self>, app: &AppHandle, generation: u64, line: &str) {
        let Some(rest) = line.strip_prefix("pi-star: ") else {
            return;
        };
        let mut parts = rest.split_whitespace();
        let Some(event) = parts.next() else {
            return;
        };
        let mut fields: HashMap<&str, &str> = HashMap::new();
        for token in parts {
            if let Some((key, value)) = token.split_once('=') {
                fields.insert(key, value);
            }
        }

        let bind_failed = {
            let mut inner = self.lock();
            if inner.generation != generation {
                return;
            }
            match event {
                "assets" => {
                    inner.assets_files = fields.get("files").and_then(|v| v.parse().ok());
                    false
                }
                "listening" => {
                    inner.addr = fields.get("addr").map(|value| (*value).to_string());
                    false
                }
                "bind-failed" => {
                    let addr = fields.get("addr").copied().unwrap_or("?");
                    let kind = fields.get("kind").copied().unwrap_or("other");
                    inner.failure = Some(classify_bind(kind));
                    inner.state = State::Failed;
                    inner.status = "端口冲突".to_string();
                    inner.detail = Some(format!(
                        "无法监听 {addr}（{kind}）。请释放该端口，或修改 desktop.toml 中的 port 后重试。"
                    ));
                    true
                }
                _ => false,
            }
        };

        if bind_failed {
            self.dispatch(app, |supervisor, app| {
                let (kind, detail) = {
                    let inner = supervisor.lock();
                    (
                        inner.failure.unwrap_or(FailureKind::PortConflict),
                        inner.detail.clone().unwrap_or_default(),
                    )
                };
                supervisor.fail_now(app, kind, detail);
            });
        }
    }

    fn monitor(self: &Arc<Self>, app: AppHandle, generation: u64) {
        let supervisor = Arc::clone(self);
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(300));
            let (state, exited, addr, elapsed) = {
                let mut inner = supervisor.lock();
                if inner.generation != generation {
                    return;
                }
                let exited = match inner.child.as_mut() {
                    Some(child) => !matches!(child.try_wait(), Ok(None)),
                    None => true,
                };
                (
                    inner.state,
                    exited,
                    inner.addr.clone(),
                    inner.started_at.elapsed(),
                )
            };

            if exited {
                supervisor.on_child_exit(&app, generation);
                return;
            }

            match addr {
                Some(addr) => match config::check_health(&addr, supervisor.password.as_deref()) {
                    Health::Ready { files } => {
                        if state != State::Running {
                            supervisor.on_running(&app, generation, addr, files);
                        }
                    }
                    Health::Foreign { detail } => {
                        supervisor.fail(&app, FailureKind::ForeignServer, detail);
                        return;
                    }
                    Health::Unreachable => {
                        if elapsed > STARTUP_DEADLINE {
                            let detail = timeout_detail(&supervisor, "等待健康检查超时");
                            supervisor.fail(&app, FailureKind::Timeout, detail);
                            return;
                        }
                    }
                },
                None if elapsed > STARTUP_DEADLINE => {
                    let detail = timeout_detail(&supervisor, "未收到 listening 事件");
                    supervisor.fail(&app, FailureKind::Timeout, detail);
                    return;
                }
                None => {}
            }
        });
    }

    fn on_running(self: &Arc<Self>, app: &AppHandle, generation: u64, addr: String, files: usize) {
        if files == 0 {
            self.fail(
                app,
                FailureKind::UiMissing,
                format!(
                    "pi-star 已在 {addr} 监听，但内置 UI 文件数为 0。\n日志：{}",
                    self.config.log_path().display()
                ),
            );
            return;
        }
        self.dispatch(app, move |supervisor, app| {
            {
                let mut inner = supervisor.lock();
                if inner.generation != generation || inner.child.is_none() {
                    return;
                }
                inner.state = State::Running;
                inner.status = format!("运行中 · {addr}");
                inner.detail = None;
                inner.failure = None;
                inner.addr = Some(addr);
                inner.assets_files = Some(files);
            }
            supervisor.update_ui(app);
            if !supervisor.config.hidden {
                supervisor.open_console_now(app);
            }
        });
    }

    fn on_child_exit(self: &Arc<Self>, app: &AppHandle, generation: u64) {
        if self.quitting.load(Ordering::SeqCst) {
            return;
        }

        let (retry, detail) = {
            let mut inner = self.lock();
            if inner.generation != generation {
                return;
            }
            let status = inner
                .child
                .as_mut()
                .and_then(|child| child.try_wait().ok().flatten());
            inner.child = None;
            inner.stdin = None;
            inner.addr = None;
            CHILD_PID.store(0, Ordering::SeqCst);
            let exit_detail = match status {
                Some(status) => format!("pi-star 已退出：{status}"),
                None => "pi-star 已退出".to_string(),
            };

            if inner.failure.is_some() {
                return;
            }

            if self.config.restart_on_failure && inner.restarts < MAX_RESTARTS {
                inner.restarts += 1;
                let delay = 1u64 << (inner.restarts - 1).min(5);
                inner.state = State::Starting;
                inner.status = format!("异常退出，{delay} 秒后重启");
                inner.detail = Some(exit_detail);
                (Some((Duration::from_secs(delay), inner.generation)), None)
            } else {
                let detail = with_log_tail(
                    &self.config.log_path(),
                    format!(
                        "{exit_detail}\n\n日志：{}",
                        self.config.log_path().display()
                    ),
                );
                (None, Some(detail))
            }
        };

        if let Some((delay, generation)) = retry {
            self.dispatch(app, |supervisor, app| {
                if let Some(window) = app.get_webview_window(MAIN) {
                    let _ = window.close();
                }
                window::ensure_splash(app, supervisor);
                supervisor.update_ui(app);
            });
            let supervisor = Arc::clone(self);
            let handle = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(delay);
                supervisor.start_checked(&handle, Some(generation));
            });
            return;
        }

        let detail = detail.unwrap_or_default();
        self.dispatch(app, move |supervisor, app| {
            supervisor.fail_now(app, FailureKind::Crash, detail);
        });
    }

    fn fail_now(self: &Arc<Self>, app: &AppHandle, kind: FailureKind, detail: String) {
        let generation = {
            let mut inner = self.lock();
            inner.state = State::Failed;
            inner.status = kind.copy().0.to_string();
            inner.failure = Some(kind);
            inner.detail = Some(with_log_tail(&self.config.log_path(), detail));
            inner.addr = None;
            inner.generation
        };
        crate::window::close_main(app);
        // 超时/健康失败时子进程可能还活着；在后台清理，避免阻塞主线程。
        // 只清理本次 generation，避免用户点重试后误杀新启动的子进程。
        let supervisor = Arc::clone(self);
        std::thread::spawn(move || supervisor.kill_child_generation(Some(generation)));
        window::ensure_splash(app, self);
        self.update_ui(app);
    }

    fn set_running(self: &Arc<Self>, status: &str, addr: Option<String>, files: Option<usize>) {
        let mut inner = self.lock();
        inner.state = State::Running;
        inner.status = status.to_string();
        inner.detail = None;
        inner.failure = None;
        inner.addr = addr;
        inner.assets_files = files;
    }

    fn update_ui(self: &Arc<Self>, app: &AppHandle) {
        let (state, addr) = {
            let inner = self.lock();
            (inner.state, inner.addr.clone())
        };
        let label = state.label(addr.as_deref());
        let _ = self.status_item.set_text(format!("状态：{label}"));
        if let Some(tray) = app.tray_by_id(TRAY_ID) {
            let _ = tray.set_tooltip(Some(format!("Pi Star · {label}")));
        }
        window::inject_status_to_splash(app, self);
    }

    fn kill_child(&self) {
        self.kill_child_generation(None);
    }

    fn kill_child_generation(&self, expected: Option<u64>) {
        let mut child = {
            let mut inner = self.lock();
            if let Some(expected) = expected {
                if inner.generation != expected {
                    return;
                }
            }
            inner.generation += 1;
            inner.addr = None;
            inner.stdin = None;
            CHILD_PID.store(0, Ordering::SeqCst);
            #[cfg(windows)]
            if let Some(job) = inner.job.take() {
                unsafe { job.terminate() };
            }
            inner.child.take()
        };
        let Some(child) = child.as_mut() else {
            return;
        };
        let pid = child.id() as i32;

        #[cfg(unix)]
        unsafe {
            libc::kill(-pid, libc::SIGTERM);
        }
        #[cfg(windows)]
        {
            let _ = child.kill();
        }

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                _ => {
                    #[cfg(unix)]
                    unsafe {
                        libc::kill(-pid, libc::SIGKILL);
                    }
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
            }
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

fn classify_spawn_error(kind: std::io::ErrorKind) -> FailureKind {
    use std::io::ErrorKind;
    match kind {
        ErrorKind::NotFound => FailureKind::MissingBinary,
        ErrorKind::PermissionDenied => FailureKind::NotExecutable,
        _ => FailureKind::SpawnFailed,
    }
}

fn classify_bind(kind: &str) -> FailureKind {
    if kind == "permission-denied" {
        FailureKind::PortForbidden
    } else {
        FailureKind::PortConflict
    }
}

fn timeout_detail(supervisor: &Supervisor, prefix: &str) -> String {
    with_log_tail(
        &supervisor.config.log_path(),
        format!(
            "{prefix}（20 秒）。\n日志：{}",
            supervisor.config.log_path().display()
        ),
    )
}

fn rotate_log(path: &Path) {
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if meta.len() <= LOG_ROTATE_BYTES {
        return;
    }
    let rotated = path.with_file_name(format!(
        "{}.1",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("pi-star.log")
    ));
    let _ = std::fs::rename(path, rotated);
}

fn with_log_tail(path: &Path, text: String) -> String {
    let tail = log_tail(path, 20);
    if tail.is_empty() {
        text
    } else {
        format!("{text}\n\n—— 日志末尾 ——\n{tail}")
    }
}

fn log_tail(path: &Path, lines: usize) -> String {
    let Ok(text) = std::fs::read_to_string(path) else {
        return String::new();
    };
    let all: Vec<&str> = text.lines().collect();
    let start = all.len().saturating_sub(lines);
    let joined = strip_ansi(&all[start..].join("\n"));
    let count = joined.chars().count();
    if count > 2000 {
        joined.chars().skip(count - 2000).collect()
    } else {
        joined
    }
}

fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            out.push(ch);
            continue;
        }
        if chars.peek() == Some(&'[') {
            chars.next();
            for code in chars.by_ref() {
                if code.is_ascii_alphabetic() {
                    break;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_failure_classification() {
        assert_eq!(
            classify_bind("permission-denied"),
            FailureKind::PortForbidden
        );
        assert_eq!(classify_bind("address-in-use"), FailureKind::PortConflict);
        assert_eq!(classify_bind(""), FailureKind::PortConflict);
    }

    #[test]
    fn spawn_error_classification() {
        use std::io::ErrorKind;
        assert_eq!(
            classify_spawn_error(ErrorKind::NotFound),
            FailureKind::MissingBinary
        );
        assert_eq!(
            classify_spawn_error(ErrorKind::PermissionDenied),
            FailureKind::NotExecutable
        );
        assert_eq!(
            classify_spawn_error(ErrorKind::Other),
            FailureKind::SpawnFailed
        );
    }

    #[test]
    fn ansi_is_stripped() {
        assert_eq!(strip_ansi("\u{1b}[2mabc\u{1b}[0m"), "abc");
        assert_eq!(strip_ansi("plain"), "plain");
    }
}

#[cfg(windows)]
#[derive(Debug, Clone, Copy)]
struct JobHandle(isize);

#[cfg(windows)]
impl JobHandle {
    unsafe fn terminate(self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;
        TerminateJobObject(self.0 as _, 1);
        CloseHandle(self.0 as _);
    }
}

#[cfg(windows)]
unsafe fn assign_to_job(child: &Child) -> Option<JobHandle> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
    if job.is_null() {
        return None;
    }

    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let ok = SetInformationJobObject(
        job,
        JobObjectExtendedLimitInformation,
        &limits as *const _ as *const _,
        std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
    );
    if ok == 0 {
        CloseHandle(job);
        return None;
    }
    if AssignProcessToJobObject(job, child.as_raw_handle() as _) == 0 {
        CloseHandle(job);
        return None;
    }
    Some(JobHandle(job as isize))
}
