//! Caricamento delle funzioni OpenGL per un contesto già reso corrente da GTK.
//!
//! GTK crea il contesto (GLX o EGL secondo il backend); qui si prendono i puntatori
//! alle funzioni da `libGL`, con `eglGetProcAddress` come ripiego.

use std::ffi::{CString, c_void};

use libloading::Library;

pub struct GlLoader {
    gl: Library,
    egl: Option<Library>,
}

impl GlLoader {
    pub fn new() -> Result<GlLoader, String> {
        unsafe {
            let gl = Library::new("libGL.so.1").map_err(|e| format!("libGL.so.1: {e}"))?;
            let egl = Library::new("libEGL.so.1").ok();
            Ok(GlLoader { gl, egl })
        }
    }

    /// Puntatore alla funzione `name`, o nullo se non esiste.
    pub fn get(&self, name: &str) -> *const c_void {
        unsafe {
            if let Ok(sym) = self.gl.get::<*const c_void>(name.as_bytes()) {
                let p = *sym;
                if !p.is_null() {
                    return p;
                }
            }
            if let Some(egl) = &self.egl {
                if let Ok(f) = egl
                    .get::<unsafe extern "C" fn(*const libc::c_char) -> *const c_void>(b"eglGetProcAddress\0")
                {
                    if let Ok(c) = CString::new(name) {
                        let p = f(c.as_ptr());
                        if !p.is_null() {
                            return p;
                        }
                    }
                }
            }
            std::ptr::null()
        }
    }

    pub fn context(&self) -> glow::Context {
        unsafe { glow::Context::from_loader_function(|name| self.get(name)) }
    }
}
