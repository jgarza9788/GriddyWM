//! Plugin ABI — dynamic library loading for out-of-tree compositor extensions.
//!
//! Only compiled when the `plugin-abi` feature is enabled (on by default).
//!
//! Plugins export a single C function:
//!   `GriddyPluginDescriptor* griddy_plugin_init(void);`
//! which returns a static descriptor containing the plugin name, ABI version,
//! and optional function pointers for each lifecycle hook.

#[cfg(feature = "plugin-abi")]
mod inner {
    use std::ffi::{CStr, CString};
    use std::os::unix::fs::MetadataExt;

    pub const GRIDDY_PLUGIN_ABI_VERSION: u32 = 1;

    // ── C-compatible types ────────────────────────────────────────────────────

    /// Opaque handle passed to every plugin hook.  Plugins must not dereference
    /// or cast this pointer — it is reserved for future compositor API calls.
    #[repr(C)]
    pub struct GriddyPluginHandle {
        _opaque: [u8; 0],
    }

    /// Per-window metadata passed to `on_window_pre_open` / `on_window_post_close`.
    #[repr(C)]
    pub struct GriddyWindowInfo {
        pub id: u32,
        pub app_id: *const std::os::raw::c_char,
        pub title: *const std::os::raw::c_char,
        pub x: i32,
        pub y: i32,
        pub w: i32,
        pub h: i32,
    }

    /// Plugin descriptor returned by `griddy_plugin_init`.
    /// All function pointer fields are optional (null = not implemented).
    #[repr(C)]
    pub struct GriddyPluginDescriptor {
        /// Short identifier for the plugin, e.g. `"touchpad-tweaks"`.
        pub name: *const std::os::raw::c_char,
        /// Must equal `GRIDDY_PLUGIN_ABI_VERSION`; refuses to load otherwise.
        pub abi_version: u32,
        /// Arbitrary plugin-private pointer; never read or freed by the compositor.
        pub user_data: *mut std::os::raw::c_void,

        /// Called once after the descriptor is validated.
        /// `config_toml` is the raw TOML string for `[plugin.<name>]` (may be empty).
        /// Return 0 on success, non-zero to abort loading.
        pub on_load: Option<
            unsafe extern "C" fn(
                handle: *mut GriddyPluginHandle,
                config_toml: *const std::os::raw::c_char,
            ) -> i32,
        >,
        /// Called once before the library is unloaded.
        pub on_unload: Option<unsafe extern "C" fn(handle: *mut GriddyPluginHandle)>,

        /// Called at the beginning of every compositor frame, before rendering.
        pub on_frame_begin:
            Option<unsafe extern "C" fn(handle: *mut GriddyPluginHandle)>,
        /// Called at the end of every compositor frame, after submit.
        pub on_frame_end:
            Option<unsafe extern "C" fn(handle: *mut GriddyPluginHandle)>,
        /// Called just before the GLES frame is opened.
        pub on_pre_render:
            Option<unsafe extern "C" fn(handle: *mut GriddyPluginHandle)>,
        /// Called just after the GLES frame is finished (before submit).
        pub on_post_render:
            Option<unsafe extern "C" fn(handle: *mut GriddyPluginHandle)>,
        /// Called when a new window is about to be placed in the grid.
        pub on_window_pre_open: Option<
            unsafe extern "C" fn(
                handle: *mut GriddyPluginHandle,
                info: *const GriddyWindowInfo,
            ),
        >,
        /// Called after a window has been fully removed from the grid.
        pub on_window_post_close: Option<
            unsafe extern "C" fn(
                handle: *mut GriddyPluginHandle,
                info: *const GriddyWindowInfo,
            ),
        >,
    }

    type GriddyPluginInitFn = unsafe extern "C" fn() -> *mut GriddyPluginDescriptor;

    // ── Loaded plugin handle ──────────────────────────────────────────────────

    pub struct LoadedPlugin {
        name: String,
        _lib: libloading::Library,
        descriptor: *mut GriddyPluginDescriptor,
        handle: GriddyPluginHandle,
    }

    // Safety: the compositor is single-threaded; raw pointers are only accessed
    // on the calloop event-loop thread.
    unsafe impl Send for LoadedPlugin {}
    unsafe impl Sync for LoadedPlugin {}

    impl LoadedPlugin {
        pub fn name(&self) -> &str {
            &self.name
        }

        pub fn call_frame_begin(&mut self) {
            if let Some(f) = unsafe { (*self.descriptor).on_frame_begin } {
                unsafe { f(&mut self.handle) };
            }
        }

        pub fn call_frame_end(&mut self) {
            if let Some(f) = unsafe { (*self.descriptor).on_frame_end } {
                unsafe { f(&mut self.handle) };
            }
        }

        pub fn call_pre_render(&mut self) {
            if let Some(f) = unsafe { (*self.descriptor).on_pre_render } {
                unsafe { f(&mut self.handle) };
            }
        }

        pub fn call_post_render(&mut self) {
            if let Some(f) = unsafe { (*self.descriptor).on_post_render } {
                unsafe { f(&mut self.handle) };
            }
        }

        pub fn call_window_pre_open(&mut self, info: &GriddyWindowInfo) {
            if let Some(f) = unsafe { (*self.descriptor).on_window_pre_open } {
                unsafe { f(&mut self.handle, info as *const _) };
            }
        }

        pub fn call_window_post_close(&mut self, info: &GriddyWindowInfo) {
            if let Some(f) = unsafe { (*self.descriptor).on_window_post_close } {
                unsafe { f(&mut self.handle, info as *const _) };
            }
        }
    }

    impl Drop for LoadedPlugin {
        fn drop(&mut self) {
            if let Some(f) = unsafe { (*self.descriptor).on_unload } {
                unsafe { f(&mut self.handle) };
            }
        }
    }

    // ── Plugin config (plugins.toml) ──────────────────────────────────────────

    /// One `[[plugin]]` entry in `plugins.toml`.
    #[derive(Debug, Clone, serde::Deserialize)]
    pub struct PluginConfig {
        /// Absolute path to the `.so` / `.dylib`.
        pub path: String,
        #[serde(default = "default_true")]
        pub enabled: bool,
        /// Raw TOML string forwarded to `on_load` as the plugin's config block.
        #[serde(default)]
        pub config: String,
    }

    fn default_true() -> bool {
        true
    }

    // ── Loader ────────────────────────────────────────────────────────────────

    /// Reject plugin paths that are world-writable or group-writable, or that live in /tmp.
    fn check_plugin_path_safety(path: &str) -> Result<(), String> {
        let p = std::path::Path::new(path);

        if !p.is_absolute() {
            return Err(format!("Plugin path must be absolute: '{path}'"));
        }

        // Refuse obvious world-writable staging directories.
        for prefix in &["/tmp", "/var/tmp", "/dev/shm"] {
            if p.starts_with(prefix) {
                return Err(format!(
                    "Refusing to load plugin from world-writable directory: '{path}'"
                ));
            }
        }

        let meta = std::fs::metadata(p)
            .map_err(|e| format!("Cannot stat plugin '{path}': {e}"))?;

        let mode = meta.mode();
        if mode & 0o002 != 0 {
            return Err(format!(
                "Refusing to load world-writable plugin: '{path}' (mode {mode:04o})"
            ));
        }
        if mode & 0o020 != 0 {
            tracing::warn!("Plugin '{path}' is group-writable (mode {mode:04o}); proceed with caution");
        }

        Ok(())
    }

    /// Attempt to load a single plugin.  Returns the loaded plugin or an error
    /// string suitable for logging and emitting as a `plugin_error` IPC event.
    pub fn load_plugin(cfg: &PluginConfig) -> Result<LoadedPlugin, String> {
        if !cfg.enabled {
            return Err(format!("Plugin disabled: {}", cfg.path));
        }

        check_plugin_path_safety(&cfg.path)?;

        let lib = unsafe {
            libloading::Library::new(&cfg.path)
                .map_err(|e| format!("dlopen '{}': {e}", cfg.path))?
        };

        let descriptor: *mut GriddyPluginDescriptor = {
            let init_fn: libloading::Symbol<GriddyPluginInitFn> = unsafe {
                lib.get(b"griddy_plugin_init\0")
                    .map_err(|e| format!("'{}' has no griddy_plugin_init: {e}", cfg.path))?
            };
            let ptr = unsafe { init_fn() };
            // Symbol is dropped here; lib is still open (we're about to store it).
            ptr
        };

        if descriptor.is_null() {
            return Err(format!("griddy_plugin_init() returned NULL in '{}'", cfg.path));
        }

        let abi = unsafe { (*descriptor).abi_version };
        if abi != GRIDDY_PLUGIN_ABI_VERSION {
            return Err(format!(
                "ABI mismatch in '{}': plugin={abi}, compositor={GRIDDY_PLUGIN_ABI_VERSION}",
                cfg.path
            ));
        }

        let name = unsafe {
            let ptr = (*descriptor).name;
            if ptr.is_null() {
                cfg.path.clone()
            } else {
                CStr::from_ptr(ptr).to_string_lossy().into_owned()
            }
        };

        let mut plugin = LoadedPlugin {
            name: name.clone(),
            _lib: lib,
            descriptor,
            handle: GriddyPluginHandle { _opaque: [] },
        };

        if let Some(f) = unsafe { (*descriptor).on_load } {
            let config_cstr =
                CString::new(cfg.config.as_bytes()).unwrap_or_default();
            let rc = unsafe { f(&mut plugin.handle, config_cstr.as_ptr()) };
            if rc != 0 {
                return Err(format!(
                    "Plugin '{}' on_load returned error code {rc}",
                    name
                ));
            }
        }

        tracing::info!("Plugin loaded: {} (ABI {})", name, abi);
        Ok(plugin)
    }

    // ── plugins.toml loader ───────────────────────────────────────────────────

    #[derive(serde::Deserialize, Default)]
    struct PluginsFile {
        #[serde(default, rename = "plugin")]
        plugins: Vec<PluginConfig>,
    }

    /// Load `plugins.toml` from `config_dir`.  Returns an empty vec if the file
    /// doesn't exist; logs a warning and returns an empty vec on parse error.
    pub fn load_plugins_file(config_dir: &std::path::Path) -> Vec<PluginConfig> {
        let path = config_dir.join("plugins.toml");
        if !path.exists() {
            return Vec::new();
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to read plugins.toml: {e}");
                return Vec::new();
            }
        };
        match toml::from_str::<PluginsFile>(&content) {
            Ok(f) => f.plugins,
            Err(e) => {
                tracing::warn!("Failed to parse plugins.toml: {e}");
                Vec::new()
            }
        }
    }
}

// ── Public re-exports (feature-gated) ────────────────────────────────────────

#[cfg(feature = "plugin-abi")]
#[allow(unused_imports)]
pub use inner::{
    load_plugin, load_plugins_file, GriddyWindowInfo, LoadedPlugin, PluginConfig,
    GRIDDY_PLUGIN_ABI_VERSION,
};

// When the feature is disabled, provide no-op stubs so call sites compile
// unconditionally.  The stubs are zero-size and all calls are eliminated.
#[cfg(not(feature = "plugin-abi"))]
mod stubs {
    pub const GRIDDY_PLUGIN_ABI_VERSION: u32 = 0;

    pub struct LoadedPlugin;
    impl LoadedPlugin {
        pub fn name(&self) -> &str { "" }
        pub fn call_frame_begin(&mut self) {}
        pub fn call_frame_end(&mut self) {}
        pub fn call_pre_render(&mut self) {}
        pub fn call_post_render(&mut self) {}
        pub fn call_window_pre_open(&mut self, _info: &GriddyWindowInfo) {}
        pub fn call_window_post_close(&mut self, _info: &GriddyWindowInfo) {}
    }

    pub struct GriddyWindowInfo {
        pub id: u32, pub app_id: *const i8, pub title: *const i8,
        pub x: i32, pub y: i32, pub w: i32, pub h: i32,
    }
}

#[cfg(not(feature = "plugin-abi"))]
pub use stubs::{GriddyWindowInfo, LoadedPlugin, GRIDDY_PLUGIN_ABI_VERSION};
