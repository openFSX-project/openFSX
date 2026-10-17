#![cfg(target_os = "windows")]
//! Independent menu host. Original UI artwork is read only from the user's FSX.
use fsx_cfg::{decode, Config};
use std::{
    borrow::Cow,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    event::{StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    window::Window,
};
mod preview;
use wry::{
    http::{Request, Response},
    WebView, WebViewBuilder,
};

#[derive(Debug, Clone)]
struct Entry {
    folder: PathBuf,
    variant: u32,
    title: String,
    manufacturer: String,
    kind: String,
    variation: String,
    preview: Option<PathBuf>,
}
fn entries(root: &Path) -> Result<Vec<Entry>, String> {
    let mut folders = fs::read_dir(root.join("SimObjects/Airplanes"))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    folders.sort_by_key(|f| f.file_name());
    let mut result = Vec::new();
    for folder in folders {
        let folder = folder.path().canonicalize().map_err(|e| e.to_string())?;
        if !folder.starts_with(root) {
            continue;
        }
        let path = folder.join("aircraft.cfg");
        if !path.is_file() {
            continue;
        }
        let path = path.canonicalize().map_err(|e| e.to_string())?;
        if !path.starts_with(root) {
            continue;
        }
        let mut bytes = Vec::new();
        fs::File::open(&path)
            .map_err(|e| e.to_string())?
            .take(2 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 2 * 1024 * 1024 {
            return Err("Aircraft CFG exceeds menu limit".into());
        }
        let c = Config::parse(&decode(&bytes, true)?)?;
        for variant in 0..1024 {
            let section = format!("fltsim.{variant}");
            let Some(title) = c.get(&section, "title") else {
                continue;
            };
            let texture = c.get(&section, "texture").unwrap_or("");
            if texture.contains(['/', '\\', ':', '\0']) || texture == ".." {
                continue;
            }
            let preview = folder
                .join(if texture.is_empty() {
                    "texture".into()
                } else {
                    format!("texture.{texture}")
                })
                .join("thumbnail.jpg");
            let preview = preview.canonicalize().ok().filter(|p| p.starts_with(root));
            result.push(Entry {
                folder: folder.clone(),
                variant,
                title: title.into(),
                manufacturer: c.get(&section, "ui_manufacturer").unwrap_or("").into(),
                kind: c.get(&section, "ui_type").unwrap_or(title).into(),
                variation: c.get(&section, "ui_variation").unwrap_or("").into(),
                preview,
            });
            if result.len() > 4096 {
                return Err("Aircraft menu exceeds 4096 variants".into());
            }
        }
    }
    if result.is_empty() {
        return Err("No aircraft variants available".into());
    }
    Ok(result)
}
const SKINS: &[&str] = &[
    "dlgstd.bmp",
    "btnstd.bmp",
    "btnstd_short.bmp",
    "btnFLYNOW.bmp",
    "btncheckbox.bmp",
    "btnclose.bmp",
    "btnminimize.bmp",
];
fn asset(
    root: &Path,
    catalog: &[Entry],
    request: &Request<Vec<u8>>,
) -> Response<Cow<'static, [u8]>> {
    let path = request.uri().path();
    let content:Option<(&str,Vec<u8>)>=match path {
        "/"|"/index.html"=>Some(("text/html; charset=utf-8",include_bytes!("../../../launcher/index.html").to_vec())),
        "/catalog.json"=>Some(("application/json",serde_json::to_vec(&catalog.iter().enumerate().map(|(i,e)|serde_json::json!({"id":i,"title":e.title,"manufacturer":e.manufacturer,"kind":e.kind,"variation":e.variation,"preview":e.preview.is_some(),"default":e.folder.file_name().is_some_and(|s|s.eq_ignore_ascii_case("C172"))&&e.variant==0})).collect::<Vec<_>>()).unwrap())),
        _=>{
            let file=if let Some(name)=path.strip_prefix("/uires/") {
                SKINS.contains(&name).then(||root.join("uires").join(name))
            }else if let Some(index)=path.strip_prefix("/preview/").and_then(|s|s.parse::<usize>().ok()) {
                catalog.get(index).and_then(|e|e.preview.clone())
            }else{None};
            file.and_then(|p|p.canonicalize().ok()).filter(|p|p.starts_with(root))
                .and_then(|p|fs::metadata(&p).ok().filter(|m|m.len()<=4*1024*1024).and_then(|_|fs::read(&p).ok()))
                .map(|data|(if path.starts_with("/preview/"){"image/jpeg"}else{"image/bmp"},data))
        }
    };
    if let Some((mime, body)) = content {
        Response::builder()
            .header("Content-Type", mime)
            .header("Cache-Control", "no-store")
            .body(Cow::Owned(body))
            .unwrap()
    } else {
        Response::builder()
            .status(404)
            .body(Cow::Borrowed(&b"Not found"[..]))
            .unwrap()
    }
}
#[derive(Debug)]
enum Action {
    Preview(usize),
    PreviewVisible(bool),
    Fly(usize),
    Exit,
    Minimize,
    Drag,
}
struct Shell {
    preview: Option<preview::Preview>,
    preview_visible: bool,
    menu_visible: bool,
    root: PathBuf,
    catalog: Arc<Vec<Entry>>,
    proxy: EventLoopProxy<Action>,
    webview: Option<WebView>,
    window: Option<Arc<Window>>,
    child: Option<Child>,
    error: Option<String>,
}
impl Shell {
    fn show_error(&self, message: &str) {
        if let Some(webview) = &self.webview {
            let _ = webview.evaluate_script(&format!(
                "window.showError({})",
                serde_json::to_string(message).unwrap()
            ));
        }
    }
}
impl ApplicationHandler<Action> for Shell {
    fn new_events(&mut self, _: &ActiveEventLoop, cause: StartCause) {
        if matches!(cause, StartCause::ResumeTimeReached { .. })
            && self.menu_visible
            && self.preview_visible
        {
            if let Some(preview) = &self.preview {
                preview.window.request_redraw();
            }
        }
    }
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let result = (|| {
            let window = Arc::new(
                event_loop
                    .create_window(
                        Window::default_attributes()
                            .with_title("openFSX")
                            .with_decorations(false)
                            .with_resizable(false)
                            .with_inner_size(winit::dpi::LogicalSize::new(1024., 738.)),
                    )
                    .map_err(|e| e.to_string())?,
            );
            let root = self.root.clone();
            let catalog = self.catalog.clone();
            let proxy = self.proxy.clone();
            let webview = WebViewBuilder::new()
                .with_incognito(true)
                .with_hotkeys_zoom(false)
                .with_devtools(false)
                .with_custom_protocol("openfsx-ui".into(), move |_, r| asset(&root, &catalog, &r))
                .with_url("openfsx-ui://localhost/")
                .with_navigation_handler(|url| {
                    url.starts_with("http://openfsx-ui.localhost/")
                        || url.starts_with("openfsx-ui://localhost/")
                })
                .with_ipc_handler(move |request| {
                    let message = request.body();
                    let action = match message.as_str() {
                        "exit" => Some(Action::Exit),
                        "minimize" => Some(Action::Minimize),
                        "drag" => Some(Action::Drag),
                        "preview-visible:0" => Some(Action::PreviewVisible(false)),
                        "preview-visible:1" => Some(Action::PreviewVisible(true)),
                        _ if message.starts_with("preview:") => message
                            .strip_prefix("preview:")
                            .and_then(|s| s.parse().ok())
                            .map(Action::Preview),
                        _ => message
                            .strip_prefix("fly:")
                            .and_then(|s| s.parse().ok())
                            .map(Action::Fly),
                    };
                    if let Some(action) = action {
                        let _ = proxy.send_event(action);
                    }
                })
                .build(window.as_ref())
                .map_err(|e| e.to_string())?;
            self.webview = Some(webview);
            self.preview = Some(preview::Preview::new(window.clone(), event_loop)?);
            self.window = Some(window);
            Ok::<_, String>(())
        })();
        if let Err(error) = result {
            self.error = Some(error);
            event_loop.exit();
        }
    }
    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        if let Some(preview) = &mut self.preview {
            if id == preview.window.id() {
                if let Err(error) = preview.event(&event) {
                    preview.visible(false);
                    self.show_error(&format!("Aircraft preview failed: {error}"));
                }
                return;
            }
        }
        match &event {
            WindowEvent::Resized(size) => {
                self.menu_visible = size.width > 0 && size.height > 0 && self.child.is_none();
                if let (Some(preview), Some(window)) = (&mut self.preview, &self.window) {
                    preview.layout(window);
                }
            }
            WindowEvent::Occluded(occluded) => {
                self.menu_visible = !occluded && self.child.is_none()
            }
            _ => {}
        }
        if matches!(event, WindowEvent::CloseRequested) && self.child.is_none() {
            event_loop.exit();
        }
    }
    fn user_event(&mut self, event_loop: &ActiveEventLoop, action: Action) {
        match action {
            Action::Preview(index) => {
                let Some(entry) = self.catalog.get(index) else {
                    return;
                };
                let Some(preview) = &mut self.preview else {
                    return;
                };
                let result = preview.load(&self.root, &entry.folder, entry.variant);
                let message = match result {
                    Ok(()) => {
                        preview.visible(self.preview_visible && self.menu_visible);
                        String::new()
                    }
                    Err(error) => format!("3D preview unavailable: {error}"),
                };
                if let Some(webview) = &self.webview {
                    let _ = webview.evaluate_script(&format!(
                        "window.previewStatus({})",
                        serde_json::to_string(&message).unwrap()
                    ));
                }
            }
            Action::PreviewVisible(visible) => {
                self.preview_visible = visible;
                if let Some(preview) = &mut self.preview {
                    preview.visible(visible && self.menu_visible);
                }
            }
            Action::Exit => {
                if self.child.is_none() {
                    event_loop.exit();
                }
            }
            Action::Minimize => {
                if let Some(w) = &self.window {
                    w.set_minimized(true);
                }
            }
            Action::Drag => {
                if let Some(w) = &self.window {
                    let _ = w.drag_window();
                }
            }
            Action::Fly(index) => {
                if self.child.is_some() {
                    return;
                }
                let Some(entry) = self.catalog.get(index) else {
                    return;
                };
                let result = (|| {
                    use std::os::windows::process::CommandExt;
                    Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
                        .arg(&entry.folder)
                        .args([
                            "--fly",
                            "--ground",
                            "--windows-1252",
                            "--lod",
                            "150",
                            "--variant",
                        ])
                        .arg(entry.variant.to_string())
                        .stdout(Stdio::null())
                        .stderr(Stdio::piped())
                        .creation_flags(0x08000000)
                        .spawn()
                        .map_err(|e| e.to_string())
                })();
                match result {
                    Ok(child) => {
                        self.child = Some(child);
                        self.menu_visible = false;
                        if let Some(preview) = &mut self.preview {
                            preview.visible(false);
                        }
                        if let Some(w) = &self.window {
                            w.set_visible(false);
                        }
                    }
                    Err(error) => self.show_error(&error),
                }
            }
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let result = self.child.as_mut().map(|child| child.try_wait());
        if let Some(Ok(Some(status))) = result {
            let mut child = self.child.take().unwrap();
            if let Some(w) = &self.window {
                w.set_visible(true);
                w.focus_window();
            }
            self.menu_visible = true;
            if let Some(preview) = &mut self.preview {
                preview.visible(self.preview_visible);
            }
            if !status.success() {
                let mut message = String::new();
                if let Some(stderr) = child.stderr.take() {
                    let _ = stderr.take(16384).read_to_string(&mut message);
                }
                self.show_error(&message);
            }
        }
        event_loop.set_control_flow(if self.child.is_some() {
            ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(250))
        } else if self.menu_visible
            && self.preview_visible
            && self.preview.as_ref().is_some_and(|p| p.ready())
        {
            ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(33))
        } else {
            ControlFlow::Wait
        });
    }
}
pub fn run(folder: &Path) -> Result<(), String> {
    let root = folder.canonicalize().map_err(|e| e.to_string())?;
    if !root.join("uires/dlgstd.bmp").is_file() {
        return Err("Menu requires the user's FSX uires/dlgstd.bmp".into());
    }
    let catalog = Arc::new(entries(&root)?);
    let event_loop = EventLoop::<Action>::with_user_event()
        .build()
        .map_err(|e| e.to_string())?;
    let mut shell = Shell {
        preview: None,
        preview_visible: true,
        menu_visible: true,
        root,
        catalog,
        proxy: event_loop.create_proxy(),
        window: None,
        webview: None,
        child: None,
        error: None,
    };
    event_loop.run_app(&mut shell).map_err(|e| e.to_string())?;
    shell.error.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "openfsx-shell-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(path.join("SimObjects/Airplanes/C172/texture.1")).unwrap();
            fs::create_dir_all(path.join("uires")).unwrap();
            Self(path.canonicalize().unwrap())
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn request(path: &str) -> Request<Vec<u8>> {
        Request::builder().uri(path).body(vec![]).unwrap()
    }
    #[test]
    fn catalog_preserves_labels_without_exposing_paths() {
        let f = Fixture::new();
        fs::write(f.0.join("SimObjects/Airplanes/C172/aircraft.cfg"), b"[fltsim.0]\ntitle=<script>fixture</script>\ntexture=1\nui_manufacturer=Test\nui_type=Fixture\n").unwrap();
        fs::write(
            f.0.join("SimObjects/Airplanes/C172/texture.1/thumbnail.jpg"),
            b"synthetic",
        )
        .unwrap();
        let catalog = entries(&f.0).unwrap();
        assert_eq!(catalog.len(), 1);
        let response = asset(&f.0, &catalog, &request("/catalog.json"));
        let json: serde_json::Value = serde_json::from_slice(response.body()).unwrap();
        assert_eq!(json[0]["title"], "<script>fixture</script>");
        assert_eq!(json[0]["default"], true);
        assert!(json[0].get("folder").is_none());
        assert!(json[0].get("path").is_none());
    }
    #[test]
    fn assets_use_an_exact_allowlist_and_indexed_previews() {
        let f = Fixture::new();
        fs::write(f.0.join("uires/dlgstd.bmp"), b"synthetic bitmap").unwrap();
        fs::write(f.0.join("secret.txt"), b"private fixture").unwrap();
        assert_eq!(
            asset(&f.0, &[], &request("/uires/dlgstd.bmp"))
                .body()
                .as_ref(),
            b"synthetic bitmap"
        );
        for path in [
            "/secret.txt",
            "/uires/../secret.txt",
            "/uires/%2e%2e/secret.txt",
            "/preview/0",
            "/preview/../../secret.txt",
            "/FSX/fsx.exe",
        ] {
            assert_eq!(asset(&f.0, &[], &request(path)).status(), 404, "{path}");
        }
    }
    #[test]
    fn catalog_rejects_oversized_cfg_before_parsing() {
        let f = Fixture::new();
        fs::write(
            f.0.join("SimObjects/Airplanes/C172/aircraft.cfg"),
            vec![b' '; 2 * 1024 * 1024 + 1],
        )
        .unwrap();
        assert!(entries(&f.0).unwrap_err().contains("limit"));
    }
}
