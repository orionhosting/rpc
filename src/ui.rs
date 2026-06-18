use std::{
    io::{BufRead, BufReader},
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender, TryRecvError},
    },
    thread,
};

use gpui::*;
use gpui_component::{Disableable, IconName, button::Button, scroll::ScrollableElement};
use gpui_component_assets::Assets;
use interprocess::TryClone;
use serde::{Deserialize, Serialize};
use tracing::{info, instrument, warn};

use crate::config::{APP_NAME, CURRENT_APP_VERSION, REPO_URL};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum UiCommand {
    ShowWindow,
    UpdateFailed { reason: String },
    UpdateAvailable { version: String },
    Quit,
}

const TRAY_EXAMPLE_BYTES: &[u8] = include_bytes!("../tray_example.png");

struct AppUi {
    tray_image: Arc<Image>,
    is_autolaunch: bool,
    autolaunch_loading: bool,
    update_version: Option<SharedString>,
    update_loading: bool,
    out_tx: Sender<String>,
}

impl Render for AppUi {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .bg(rgb(0x1e1e2e))
            .child(self.render_content(window, cx))
    }
}

impl AppUi {
    fn render_content(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .id("content")
            .overflow_y_scrollbar()
            .size_full()
            .mx_auto()
            .max_w(px(550.0))
            .text_color(rgb(0xcdd6f4))
            .child(
                div()
                    .text_center()
                    .text_3xl()
                    .child(APP_NAME)
                )
            .mt_2()
            .mb_4()
            .child(
                div()
                    .flex()
                    .w_full()
                    .justify_between()
                    .child(
                        div()
                            .m_3()
                            .flex()
                            .flex_col()
                            .w(px(240.0))
                            .gap_2()
                            .flex_shrink()
                            .child("Use the system tray to control the RPC.")
                            .child("The RPC will continue running in the background after this window is closed.")
                    )
                    .child(
                        img(self.tray_image.clone())
                            .flex()
                            .m_3()
                            .w(px(240.0))
                            .flex_shrink_0()
                    )
            )
            .child(
                div().w_full().h_0p5().bg(rgb(0xcdd6f4)).my_10()
            )
            .child(
                div().grid().gap_3()
                .child(self.render_autolaunch_card(window, cx))
                .child(self.render_update_card(window, cx))
                .child(self.render_github_card(window, cx))
            )
    }

    fn render_card(&mut self, left: impl IntoElement, right: impl IntoElement) -> impl IntoElement {
        div()
            .w_full()
            .border_1()
            .p_1()
            .flex()
            .items_center()
            .justify_between()
            .child(left)
            .child(right)
    }

    fn render_autolaunch_card(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let label = if self.autolaunch_loading {
            "Updating..."
        } else if self.is_autolaunch {
            "Disable"
        } else {
            "Enable"
        };

        self.render_card(
            "Open in background when your PC starts",
            Button::new("toggle-autolaunch")
                .label(label)
                .disabled(self.autolaunch_loading)
                .on_click(cx.listener(|this: &mut Self, _e, _window, cx| {
                    if this.autolaunch_loading {
                        return;
                    }
                    this.autolaunch_loading = true;
                    cx.notify();

                    let target = !this.is_autolaunch;
                    cx.spawn(async move |view: WeakEntity<AppUi>, cx: &mut AsyncApp| {
                        // Tiny delay
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(300))
                            .await;
                        let success = crate::auto_launch::set_startup(target).is_ok();

                        let _ = view.update(cx, |this, cx| {
                            this.autolaunch_loading = false;
                            if success {
                                this.is_autolaunch = target;
                            }
                            cx.notify();
                        });
                    })
                    .detach();
                })),
        )
    }

    fn render_update_card(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (label, disabled) = if self.update_loading {
            ("Updating...".to_string(), true)
        } else if let Some(ref ver) = self.update_version {
            (format!("Update (v{})", ver), false)
        } else {
            ("No update available".to_string(), true)
        };

        self.render_card(
            format!("Version: v{}", CURRENT_APP_VERSION),
            Button::new("trigger-update")
                .icon(IconName::Info)
                .disabled(disabled)
                .label(label)
                .on_click(cx.listener(|this: &mut Self, _e, _window, cx| {
                    if this.update_loading || this.update_version.is_none() {
                        return;
                    }
                    this.update_loading = true;
                    cx.notify();

                    // Send command to the backend process
                    let _ = this.out_tx.send("ExecuteUpdate".to_string());
                })),
        )
    }

    fn render_github_card(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        self.render_card(
            "App repository",
            Button::new("github")
                .icon(IconName::GitHub)
                .label("GitHub")
                .on_click(cx.listener(|_this: &mut Self, _e, _window, cx| {
                    cx.open_url(REPO_URL);
                })),
        )
    }
}

/// Prepare the UI.
#[instrument(target = "frontend", skip_all)]
pub fn run_ui(ipc_name: String, show_on_start: bool) {
    info!("Starting UI app");

    use interprocess::local_socket::{GenericNamespaced, Stream, prelude::*};

    let name = ipc_name
        .as_str()
        .to_ns_name::<GenericNamespaced>()
        .expect("Invalid IPC name");
    let stream = Stream::connect(name).expect("Failed to connect to the main IPC pipe");

    let mut stream_writer = stream.try_clone().expect("Failed to clone IPC stream");
    let (out_tx, out_rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        use std::io::Write;
        for msg in out_rx {
            let _ = writeln!(stream_writer, "{}", msg);
        }
    });

    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let reader = BufReader::new(stream);

        for line in reader.lines() {
            if let Ok(msg) = line {
                if let Ok(cmd) = serde_json::from_str(&msg) {
                    if tx.send(cmd).is_err() {
                        break;
                    }
                }
            } else {
                // Connection lost
                break;
            }
        }

        let _ = tx.send(UiCommand::Quit);
    });

    start_app(rx, show_on_start, out_tx);
}

/// Start the app.
fn start_app(rx: Receiver<UiCommand>, show_on_start: bool, out_tx: Sender<String>) {
    let tray_image = Arc::new(Image::from_bytes(
        ImageFormat::Png,
        TRAY_EXAMPLE_BYTES.to_vec(),
    ));

    Application::new().with_assets(Assets).run(move |cx| {
        gpui_component::init(cx);

        cx.on_window_closed(|cx| {
            if cx.windows().len() == 0 {
                std::process::exit(0);
            }
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(600.0), px(600.0)), cx);

        let app_view = cx.new(|_| AppUi {
            tray_image,
            is_autolaunch: crate::auto_launch::is_startup_enabled(),
            autolaunch_loading: false,
            update_version: None,
            update_loading: false,
            out_tx,
        });

        let window_handle = cx
            .open_window(
                WindowOptions {
                    window_min_size: Some(size(px(500.0), px(200.0))),
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    show: show_on_start,
                    titlebar: Some(TitlebarOptions {
                        title: Some(APP_NAME.into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |_, _| app_view.clone(),
            )
            .unwrap();

        cx.spawn(async move |cx| {
            loop {
                match rx.try_recv() {
                    Ok(cmd) => match cmd {
                        UiCommand::ShowWindow => {
                            let _ = window_handle.update(cx, |_, window, _| {
                                window.activate_window();
                            });
                        }
                        UiCommand::UpdateAvailable { version } => {
                            info!("Update available: {version}");
                            let _ = app_view.update(cx, |this, cx| {
                                this.update_version = Some(version.into());
                                cx.notify();
                            });
                        }
                        UiCommand::UpdateFailed { reason } => {
                            warn!("Update failed: {reason}");
                            let _ = app_view.update(cx, |this, cx| {
                                this.update_loading = false;
                                cx.notify();
                            });
                        }
                        UiCommand::Quit => {
                            let _ = cx.update(|cx| {
                                cx.quit();
                            });
                            break;
                        }
                    },
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => {
                        let _ = cx.update(|cx| cx.quit());
                        break;
                    }
                }

                cx.background_executor()
                    .timer(std::time::Duration::from_millis(16))
                    .await;
            }
        })
        .detach();
    });
}
