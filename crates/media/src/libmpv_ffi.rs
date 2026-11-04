#![allow(unsafe_code)]

use anyhow::{Result, anyhow};
use libloading::Library;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::{Path, PathBuf};
use std::ptr;

const MPV_RENDER_PARAM_INVALID: c_int = 0;
const MPV_RENDER_PARAM_API_TYPE: c_int = 1;
const MPV_RENDER_PARAM_SW_SIZE: c_int = 17;
const MPV_RENDER_PARAM_SW_FORMAT: c_int = 18;
const MPV_RENDER_PARAM_SW_STRIDE: c_int = 19;
const MPV_RENDER_PARAM_SW_POINTER: c_int = 20;

#[repr(C)]
struct MpvRenderParam {
    type_: c_int,
    data: *mut c_void,
}

type FnMpvCreate = unsafe extern "C" fn() -> *mut c_void;
type FnMpvInitialize = unsafe extern "C" fn(*mut c_void) -> c_int;
type FnMpvTerminateDestroy = unsafe extern "C" fn(*mut c_void);
type FnMpvSetOptionString = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int;
type FnMpvSetPropertyString =
    unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int;
type FnMpvGetPropertyString = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_char;
type FnMpvFree = unsafe extern "C" fn(*mut c_void);
type FnMpvCommand = unsafe extern "C" fn(*mut c_void, *const *const c_char) -> c_int;
type FnMpvRenderContextCreate =
    unsafe extern "C" fn(*mut *mut c_void, *mut c_void, *mut MpvRenderParam) -> c_int;
type FnMpvRenderContextRender = unsafe extern "C" fn(*mut c_void, *mut MpvRenderParam) -> c_int;
type FnMpvRenderContextFree = unsafe extern "C" fn(*mut c_void);

/// Dynamically loaded `libmpv` instance (`libmpv-2.dll` / `mpv-2.dll`) with software RGBA render context.
pub struct LibMpvPlayer {
    _lib: Library,
    handle: *mut c_void,
    render_ctx: *mut c_void,
    fn_terminate: FnMpvTerminateDestroy,
    fn_set_prop: FnMpvSetPropertyString,
    fn_get_prop: FnMpvGetPropertyString,
    fn_free: FnMpvFree,
    fn_command: FnMpvCommand,
    fn_render: FnMpvRenderContextRender,
    fn_render_free: FnMpvRenderContextFree,
}

unsafe impl Send for LibMpvPlayer {}

impl LibMpvPlayer {
    pub fn try_new() -> Result<Self> {
        let lib = load_mpv_library()?;

        unsafe {
            let fn_create: FnMpvCreate = *lib
                .get(b"mpv_create\0")
                .map_err(|e| anyhow!("missing mpv_create: {e}"))?;
            let fn_init: FnMpvInitialize = *lib
                .get(b"mpv_initialize\0")
                .map_err(|e| anyhow!("missing mpv_initialize: {e}"))?;
            let fn_terminate: FnMpvTerminateDestroy = *lib
                .get(b"mpv_terminate_destroy\0")
                .map_err(|e| anyhow!("missing mpv_terminate_destroy: {e}"))?;
            let fn_set_opt: FnMpvSetOptionString = *lib
                .get(b"mpv_set_option_string\0")
                .map_err(|e| anyhow!("missing mpv_set_option_string: {e}"))?;
            let fn_set_prop: FnMpvSetPropertyString = *lib
                .get(b"mpv_set_property_string\0")
                .map_err(|e| anyhow!("missing mpv_set_property_string: {e}"))?;
            let fn_get_prop: FnMpvGetPropertyString = *lib
                .get(b"mpv_get_property_string\0")
                .map_err(|e| anyhow!("missing mpv_get_property_string: {e}"))?;
            let fn_free: FnMpvFree = *lib
                .get(b"mpv_free\0")
                .map_err(|e| anyhow!("missing mpv_free: {e}"))?;
            let fn_command: FnMpvCommand = *lib
                .get(b"mpv_command\0")
                .map_err(|e| anyhow!("missing mpv_command: {e}"))?;
            let fn_render_create: FnMpvRenderContextCreate = *lib
                .get(b"mpv_render_context_create\0")
                .map_err(|e| anyhow!("missing mpv_render_context_create: {e}"))?;
            let fn_render: FnMpvRenderContextRender = *lib
                .get(b"mpv_render_context_render\0")
                .map_err(|e| anyhow!("missing mpv_render_context_render: {e}"))?;
            let fn_render_free: FnMpvRenderContextFree = *lib
                .get(b"mpv_render_context_free\0")
                .map_err(|e| anyhow!("missing mpv_render_context_free: {e}"))?;

            let handle = fn_create();
            if handle.is_null() {
                return Err(anyhow!("mpv_create returned null"));
            }

            set_opt_cstr(fn_set_opt, handle, "vo", "libmpv");
            set_opt_cstr(fn_set_opt, handle, "hwdec", "auto");
            set_opt_cstr(fn_set_opt, handle, "keep-open", "yes");
            set_opt_cstr(fn_set_opt, handle, "terminal", "no");

            let init_rc = fn_init(handle);
            if init_rc < 0 {
                fn_terminate(handle);
                return Err(anyhow!("mpv_initialize failed with code {init_rc}"));
            }

            let api_sw = CString::new("sw").unwrap();
            let mut params = [
                MpvRenderParam {
                    type_: MPV_RENDER_PARAM_API_TYPE,
                    data: api_sw.as_ptr() as *mut c_void,
                },
                MpvRenderParam {
                    type_: MPV_RENDER_PARAM_INVALID,
                    data: ptr::null_mut(),
                },
            ];

            let mut render_ctx: *mut c_void = ptr::null_mut();
            let r_rc = fn_render_create(&mut render_ctx, handle, params.as_mut_ptr());
            if r_rc < 0 || render_ctx.is_null() {
                fn_terminate(handle);
                return Err(anyhow!("mpv_render_context_create failed with code {r_rc}"));
            }

            Ok(Self {
                _lib: lib,
                handle,
                render_ctx,
                fn_terminate,
                fn_set_prop,
                fn_get_prop,
                fn_free,
                fn_command,
                fn_render,
                fn_render_free,
            })
        }
    }

    pub fn load_file(&mut self, path: &str, loop_playback: bool, mute: bool) -> Result<()> {
        self.set_property("loop-file", if loop_playback { "inf" } else { "no" });
        self.set_property("mute", if mute { "yes" } else { "no" });
        self.set_property("pause", "no");

        let c_cmd = CString::new("loadfile")?;
        let c_path = CString::new(path)?;
        let args = [c_cmd.as_ptr(), c_path.as_ptr(), ptr::null()];
        let rc = unsafe { (self.fn_command)(self.handle, args.as_ptr()) };
        if rc < 0 {
            return Err(anyhow!("mpv loadfile failed with code {rc}"));
        }
        Ok(())
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.set_property("pause", if paused { "yes" } else { "no" });
    }

    pub fn set_volume(&mut self, volume_0_100: u8) {
        let val = volume_0_100.min(100).to_string();
        self.set_property("volume", &val);
    }

    pub fn stop(&mut self) {
        let c_cmd = CString::new("stop").unwrap();
        let args = [c_cmd.as_ptr(), ptr::null()];
        unsafe {
            let _ = (self.fn_command)(self.handle, args.as_ptr());
        }
    }

    pub fn duration_secs(&self) -> Option<f64> {
        let name = CString::new("duration").ok()?;
        unsafe {
            let raw = (self.fn_get_prop)(self.handle, name.as_ptr());
            if raw.is_null() {
                return None;
            }
            let val = CStr::from_ptr(raw).to_string_lossy().parse::<f64>().ok();
            (self.fn_free)(raw as *mut c_void);
            val
        }
    }

    pub fn position_secs(&self) -> Option<f64> {
        let name = CString::new("time-pos").ok()?;
        unsafe {
            let raw = (self.fn_get_prop)(self.handle, name.as_ptr());
            if raw.is_null() {
                return None;
            }
            let val = CStr::from_ptr(raw).to_string_lossy().parse::<f64>().ok();
            (self.fn_free)(raw as *mut c_void);
            val
        }
    }

    /// Posição absoluta em segundos.
    pub fn seek_secs(&mut self, secs: f64) {
        self.set_property("time-pos", &format!("{:.3}", secs.max(0.0)));
    }

    pub fn render_rgba(&mut self, width: u32, height: u32, out_rgba: &mut [u8]) -> Result<()> {
        let expected = (width as usize) * (height as usize) * 4;
        if out_rgba.len() < expected {
            return Err(anyhow!("output RGBA buffer too small"));
        }
        let mut size: [c_int; 2] = [width as c_int, height as c_int];
        let fmt = CString::new("rgba").unwrap();
        let mut stride: usize = (width as usize) * 4;
        let ptr_pixels = out_rgba.as_mut_ptr() as *mut c_void;

        let mut params = [
            MpvRenderParam {
                type_: MPV_RENDER_PARAM_SW_SIZE,
                data: size.as_mut_ptr() as *mut c_void,
            },
            MpvRenderParam {
                type_: MPV_RENDER_PARAM_SW_FORMAT,
                data: fmt.as_ptr() as *mut c_void,
            },
            MpvRenderParam {
                type_: MPV_RENDER_PARAM_SW_STRIDE,
                data: &mut stride as *mut usize as *mut c_void,
            },
            MpvRenderParam {
                type_: MPV_RENDER_PARAM_SW_POINTER,
                data: ptr_pixels,
            },
            MpvRenderParam {
                type_: MPV_RENDER_PARAM_INVALID,
                data: ptr::null_mut(),
            },
        ];

        let rc = unsafe { (self.fn_render)(self.render_ctx, params.as_mut_ptr()) };
        if rc < 0 {
            return Err(anyhow!("mpv_render_context_render returned {rc}"));
        }
        Ok(())
    }

    fn set_property(&mut self, key: &str, value: &str) {
        if let (Ok(k), Ok(v)) = (CString::new(key), CString::new(value)) {
            unsafe {
                let _ = (self.fn_set_prop)(self.handle, k.as_ptr(), v.as_ptr());
            }
        }
    }
}

impl Drop for LibMpvPlayer {
    fn drop(&mut self) {
        unsafe {
            if !self.render_ctx.is_null() {
                (self.fn_render_free)(self.render_ctx);
                self.render_ctx = ptr::null_mut();
            }
            if !self.handle.is_null() {
                (self.fn_terminate)(self.handle);
                self.handle = ptr::null_mut();
            }
        }
    }
}

pub fn probe_libmpv_path() -> Option<PathBuf> {
    let candidates = candidate_dll_paths();
    for p in candidates {
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn load_mpv_library() -> Result<Library> {
    for candidate in candidate_dll_paths() {
        if candidate.exists()
            && let Ok(lib) = unsafe { Library::new(&candidate) }
        {
            return Ok(lib);
        }
    }
    for name in ["libmpv-2.dll", "mpv-2.dll", "libmpv.dll"] {
        if let Ok(lib) = unsafe { Library::new(name) } {
            return Ok(lib);
        }
    }
    Err(anyhow!(
        "libmpv-2.dll not found in application folder, redist/, or PATH"
    ))
}

fn candidate_dll_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        for name in ["libmpv-2.dll", "mpv-2.dll", "libmpv.dll"] {
            out.push(dir.join(name));
            out.push(dir.join("redist").join(name));
        }
    }
    for name in ["libmpv-2.dll", "mpv-2.dll"] {
        out.push(Path::new("redist").join(name));
    }
    out
}

unsafe fn set_opt_cstr(
    func: FnMpvSetOptionString,
    handle: *mut c_void,
    name: &str,
    val: &str,
) {
    if let (Ok(n), Ok(v)) = (CString::new(name), CString::new(val)) {
        let _ = unsafe { func(handle, n.as_ptr(), v.as_ptr()) };
    }
}
