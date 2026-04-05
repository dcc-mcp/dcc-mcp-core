//! `dcc-mcp-capture` — GPU framebuffer screenshot / frame-capture for DCC applications.
//!
//! # Architecture
//! ```text
//! Capturer (high-level API)
//!     └── DccCapture trait (backend abstraction)
//!             ├── DxgiBackend    (Windows — DXGI Desktop Duplication API)
//!             ├── X11Backend     (Linux   — X11 XShmGetImage)
//!             └── MockBackend    (all platforms — synthetic checkerboard)
//! ```
//!
//! # Selection Order
//! At runtime [`Capturer::new_auto`] probes each backend in priority order and
//! selects the first one that reports [`DccCapture::is_available`] as `true`.
//!
//! # Quick Start (Rust)
//! ```rust,no_run
//! use dcc_mcp_capture::{Capturer, CaptureConfig};
//!
//! let capturer = Capturer::new_auto();
//! let frame = capturer.capture(&CaptureConfig::default()).unwrap();
//! println!("{}×{} — {} bytes", frame.width, frame.height, frame.byte_len());
//! ```
//!
//! # Quick Start (Python — after maturin build)
//! ```python
//! from dcc_mcp_core import PyCapturer
//! capturer = PyCapturer.new_auto()
//! frame = capturer.capture(format="png")
//! ```
//!
//! # Modules
//! | Module | Purpose |
//! |--------|---------|
//! | [`error`] | `CaptureError` / `CaptureResult` |
//! | [`types`] | `CaptureFormat`, `CaptureConfig`, `CaptureFrame`, `CaptureTarget` |
//! | [`capture`] | `DccCapture` trait |
//! | [`backend`] | Platform-specific backends + `best_available()` selector |
//! | [`window`] | Window / process finder (`WindowFinder`) |
//! | [`capturer`] | `Capturer` high-level wrapper with stats |
//! | [`python`] | PyO3 bindings (feature-gated on `python-bindings`) |

pub mod backend;
pub mod capture;
pub mod capturer;
pub mod error;
pub mod types;
pub mod window;

#[cfg(feature = "python-bindings")]
pub mod python;

// Re-export most-used types at crate root.
pub use capture::DccCapture;
pub use capturer::{CaptureStats, Capturer};
pub use error::{CaptureError, CaptureResult};
pub use types::{CaptureBackendKind, CaptureConfig, CaptureFormat, CaptureFrame, CaptureTarget};
pub use window::{WindowFinder, WindowInfo};

#[cfg(feature = "python-bindings")]
pub use python::{PyCaptureFrame, PyCapturer};
