//! Winit [`ApplicationHandler`] implementation and the public
//! [`run_gui`] entry point.
//!
//! The main thread owns the winit event loop, the window, and the
//! GPU renderer. A background thread owns the tokio runtime +
//! [`arx_driver::Driver`].

use std::path::PathBuf;
use std::sync::{mpsc as std_mpsc, Arc};

use crossterm::event::Event as XtermEvent;
use tokio::sync::mpsc as tokio_mpsc;
use tracing::warn;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowAttributes, WindowId};

use arx_driver::{Driver, SharedTerminalSize};
use arx_keymap::profiles::Profile;

use crate::backend::{BackendFrame, GpuBackend, WakeFn};
use crate::input;
use crate::renderer::GuiRenderer;

/// Custom user events pushed via [`EventLoopProxy`].
#[derive(Debug, Clone)]
pub enum UserEvent {
    /// The render task pushed one or more `BackendFrame`s — time to
    /// request a redraw.
    FrameReady,
    /// The driver's tokio thread exited — time to close the window
    /// and quit the event loop.
    EditorExited,
}

/// Errors from running the GUI.
#[derive(Debug, thiserror::Error)]
pub enum GuiError {
    #[error("winit event loop error: {0}")]
    EventLoop(#[from] winit::error::EventLoopError),
    #[error("driver error: {0}")]
    Driver(String),
}

/// Run the GUI client. This function **blocks** the calling thread
/// (it must be the main thread on macOS).
///
/// * `file` — optional path to open in the editor.
/// * `profile` — keymap profile (`Emacs`, `Vim`, `Kedit`).
pub fn run_gui(file: Option<PathBuf>, profile: Profile) -> Result<(), GuiError> {
    let event_loop: EventLoop<UserEvent> = EventLoop::with_user_event()
        .build()
        .expect("failed to build winit event loop");
    let proxy = event_loop.create_proxy();
    let mut app = GuiApp::new(proxy, file, profile);
    event_loop.run_app(&mut app)?;
    Ok(())
}

/// State kept across the winit event loop.
struct GuiApp {
    proxy: EventLoopProxy<UserEvent>,
    /// Set once the window is created.
    state: Option<RunningState>,
    /// Init args — consumed once during `resumed`.
    init: Option<InitArgs>,
}

struct InitArgs {
    file: Option<PathBuf>,
    profile: Profile,
}

/// Everything that exists once the window is open.
struct RunningState {
    window: Arc<Window>,
    renderer: GuiRenderer,
    /// Receiver for [`BackendFrame`]s from the editor's render task.
    frame_rx: std_mpsc::Receiver<BackendFrame>,
    /// Sender for input events going *into* the driver's input task.
    /// When dropped, the driver's input stream ends and it shuts down.
    input_tx: tokio_mpsc::UnboundedSender<std::io::Result<XtermEvent>>,
    /// Shared cell-size so the driver adapts to window resizes.
    term_size: SharedTerminalSize,
    /// Current modifier state.
    modifiers: ModifiersState,
    /// Last known cursor position in window-local pixels.
    cursor_pos: (f64, f64),
    /// Whether the left mouse button is currently held.
    left_pressed: bool,
}

impl GuiApp {
    fn new(proxy: EventLoopProxy<UserEvent>, file: Option<PathBuf>, profile: Profile) -> Self {
        Self {
            proxy,
            state: None,
            init: Some(InitArgs { file, profile }),
        }
    }
}

impl ApplicationHandler<UserEvent> for GuiApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            // Already initialised (resume from suspend on mobile);
            // nothing to do on desktop.
            return;
        }
        let Some(init) = self.init.take() else {
            return;
        };

        // ---- Create window ----
        let attrs = WindowAttributes::default()
            .with_title("arx")
            .with_inner_size(PhysicalSize::new(1024u32, 768));
        let window = Arc::new(event_loop.create_window(attrs).expect("create_window failed"));

        // ---- Create renderer (blocking async init) ----
        let (cols, rows) = (80u16, 24);
        let renderer = pollster::block_on(GuiRenderer::new(window.clone(), cols, rows));

        // Recompute grid based on actual window size.
        let (cols, rows) = renderer.grid_size_cells();

        // ---- Channels ----
        let wake: WakeFn = {
            let proxy = self.proxy.clone();
            Arc::new(move || {
                let _ = proxy.send_event(UserEvent::FrameReady);
            })
        };
        let (backend, frame_rx) = GpuBackend::new(cols, rows, wake);
        let (input_tx, input_rx) = tokio_mpsc::unbounded_channel::<std::io::Result<XtermEvent>>();

        let term_size = SharedTerminalSize::new(cols, rows);

        // ---- Spawn tokio runtime + driver on a background thread ----
        let proxy_bg = self.proxy.clone();
        let term_size_bg = term_size.clone();
        let file = init.file;
        let profile = init.profile;
        std::thread::Builder::new()
            .name("arx-tokio".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("tokio runtime");
                rt.block_on(async move {
                    // Wrap the tokio mpsc receiver into a futures Stream.
                    use futures_util::StreamExt;

                    let events =
                        tokio_stream::wrappers::UnboundedReceiverStream::new(input_rx);
                    // Identity map preserves the io::Result<Event> type.
                    let events = events.map(|ev| ev);

                    let driver = Driver::new(|_editor| {})
                        .with_profile(profile)
                        .with_async_hook({
                            let file = file.clone();
                            move |bus| async move {
                                if let Some(path) = file {
                                    if let Err(e) = arx_core::open_file(&bus, path).await {
                                        warn!(%e, "failed to open file in GUI");
                                    }
                                }
                            }
                        });

                    let result = driver
                        .run_with(events, backend, term_size_bg, |_bus| async {})
                        .await;
                    if let Err(e) = &result {
                        warn!(%e, "driver exited with error");
                    }
                    let _ = proxy_bg.send_event(UserEvent::EditorExited);
                });
            })
            .expect("spawn tokio thread");

        // Request keyboard focus so the window receives mouse-wheel
        // events immediately — many Linux WMs (X11 click-to-focus,
        // Wayland) only route scroll to the focused surface.
        window.focus_window();
        window.request_redraw();

        self.state = Some(RunningState {
            window,
            renderer,
            frame_rx,
            input_tx,
            term_size,
            modifiers: ModifiersState::empty(),
            cursor_pos: (0.0, 0.0),
            left_pressed: false,
        });
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::FrameReady => {
                if let Some(state) = &self.state {
                    state.window.request_redraw();
                }
            }
            UserEvent::EditorExited => {
                if let Some(state) = self.state.take() {
                    drop(state);
                }
                // Nothing left — winit will exit the loop on its own
                // when there are no windows.
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(state) = self.state.as_mut() else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => {
                // Drop the input sender — driver will see the stream
                // end and shut down.
                self.state = None;
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                state.renderer.resize(size.width, size.height);
                let (cols, rows) = state.renderer.grid_size_cells();
                state.term_size.set(cols, rows);
                // Tell the driver about the resize so it rebuilds the
                // layout.
                let _ = state
                    .input_tx
                    .send(Ok(XtermEvent::Resize(cols, rows)));
                state.window.request_redraw();
            }
            WindowEvent::ModifiersChanged(mods) => {
                state.modifiers = mods.state();
            }
            WindowEvent::KeyboardInput {
                event: key_event, ..
            } => {
                if let Some(xterm) = input::translate_key(&key_event, state.modifiers) {
                    let _ = state.input_tx.send(Ok(xterm));
                }
            }
            WindowEvent::RedrawRequested => {
                // Drain all pending frames.
                while let Ok(frame) = state.frame_rx.try_recv() {
                    state.renderer.apply_ops(&frame.ops);
                }
                if !state.renderer.render() {
                    // Surface needs reconfiguring.
                    let size = state.window.inner_size();
                    state.renderer.resize(size.width, size.height);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                state.cursor_pos = (position.x, position.y);
                if state.left_pressed {
                    let (col, row) =
                        state.renderer.pixel_to_cell(position.x, position.y);
                    let ev = input::translate_mouse_drag(col, row, state.modifiers);
                    let _ = state.input_tx.send(Ok(ev));
                }
            }
            WindowEvent::MouseInput { state: btn_state, button, .. } => {
                if button == winit::event::MouseButton::Left {
                    state.left_pressed = btn_state == ElementState::Pressed;
                }
                let (col, row) =
                    state.renderer.pixel_to_cell(state.cursor_pos.0, state.cursor_pos.1);
                if let Some(ev) = input::translate_mouse_button(
                    button, btn_state, col, row, state.modifiers,
                ) {
                    let _ = state.input_tx.send(Ok(ev));
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (col, row) =
                    state.renderer.pixel_to_cell(state.cursor_pos.0, state.cursor_pos.1);
                if let Some(ev) =
                    input::translate_scroll(delta, col, row, state.modifiers)
                {
                    let _ = state.input_tx.send(Ok(ev));
                }
            }
            _ => {}
        }
    }

    fn new_events(&mut self, _event_loop: &ActiveEventLoop, _cause: StartCause) {}

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {}
}
