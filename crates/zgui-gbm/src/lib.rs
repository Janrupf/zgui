//! Allocating a buffer that a display scans out of and a graphics device draws into.
//!
//! The crate names no zgui crate and is usable on its own. It is written to be named by
//! `zgui-platform-drm` and `zgui-scanout`, one step along the same layer, and by nothing else.
//!
//! The kernel's dumb buffers are the obvious thing to reach for and they are the wrong thing. A
//! dumb buffer is *CPU-writable and scanout-capable*, which is not the same as renderable: a
//! graphics driver handed one as a render target puts it where its own engine wants it, another
//! device pins it somewhere else the moment it imports the descriptor, and the submission that
//! would draw into it is then refused — on nouveau, `kernel rejected pushbuf: Invalid argument`,
//! several calls after the point where anything could have reported the mistake.
//!
//! `gbm_bo_create` states both requirements where the driver can act on them, which is at
//! allocation. It is what every Wayland compositor allocates scanout buffers with.
//!
//! # Implicit layouts only
//!
//! There are two ways to allocate: naming the layouts that are acceptable, or letting the driver
//! choose. The kernel's buffer-exchange documentation is firm that a chain must not mix them —
//! *"the complete chain of operations formed by the producer and all the consumers must be either
//! fully implicit or fully explicit"* — so this takes the implicit form throughout and never sends
//! a modifier to the framebuffer either. Drivers that publish no explicit layout at all, which
//! includes the one this was written against, refuse the other form outright.
//!
//! # Loaded, never linked
//!
//! For the reason every other library here is: a console session has to start on a machine that
//! has none of them. A build needs neither the library nor its headers, and a display whose
//! machine has no `libgbm` falls back to copying each frame.

#![deny(missing_docs)]
// This crate is on the unsafe ledger's allowlist for one reason: libgbm is opened at run time and
// every call into it is a call through a pointer resolved here. A build that linked it would stop a
// console session starting on a machine that has no such library, which is the case the copied
// scanout path exists to answer.
#![allow(unsafe_code)]

use std::ffi::{CStr, c_char, c_int, c_void};
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::sync::Arc;

/// The buffer is written by the graphics device rather than by the processor.
const USE_RENDERING: u32 = 4;
/// The buffer is scanned out by a display engine.
const USE_SCANOUT: u32 = 1;

/// The code a driver answers when a buffer is in no layout it has a name for.
///
/// `DRM_FORMAT_MOD_INVALID`. It means the layout is implicit — agreed between the two ends by
/// something other than this number — and it is what the driver this was written against answers
/// for every buffer it makes.
pub const IMPLICIT: u64 = 0x00ff_ffff_ffff_ffff;

/// `libgbm`, loaded.
///
/// One per process is plenty; a [`Device`] holds a handle to it so that the library outlives every
/// buffer made through it.
#[derive(Debug)]
pub struct Library {
    /// Kept so the symbols below stay mapped.
    #[expect(
        dead_code,
        reason = "holds the mapping the function pointers point into"
    )]
    library: libloading::Library,
    create_device: unsafe extern "C" fn(c_int) -> *mut c_void,
    device_destroy: unsafe extern "C" fn(*mut c_void),
    backend_name: unsafe extern "C" fn(*mut c_void) -> *const c_char,
    bo_create: unsafe extern "C" fn(*mut c_void, u32, u32, u32, u32) -> *mut c_void,
    bo_destroy: unsafe extern "C" fn(*mut c_void),
    bo_get_fd: unsafe extern "C" fn(*mut c_void) -> c_int,
    bo_get_stride: unsafe extern "C" fn(*mut c_void) -> u32,
    bo_get_offset: unsafe extern "C" fn(*mut c_void, c_int) -> u32,
    bo_get_modifier: unsafe extern "C" fn(*mut c_void) -> u64,
    bo_get_plane_count: unsafe extern "C" fn(*mut c_void) -> c_int,
    bo_map: unsafe extern "C" fn(
        *mut c_void,
        u32,
        u32,
        u32,
        u32,
        u32,
        *mut u32,
        *mut *mut c_void,
    ) -> *mut c_void,
    bo_unmap: unsafe extern "C" fn(*mut c_void, *mut c_void),
}

/// `GBM_BO_TRANSFER_READ`.
const TRANSFER_READ: u32 = 1;

impl Library {
    /// Loads it, or says why it could not.
    ///
    /// # Errors
    ///
    /// Returns the loader's own message where the library is absent, and the name of the first
    /// symbol that is missing where it is present but too old.
    pub fn load() -> Result<Arc<Self>, String> {
        // SAFETY: every name is libgbm's own and every signature is the one in `gbm.h`. Each
        // symbol is fetched *as its function type*: fetching one as a pointer would read the first
        // bytes of the function's code and call whatever those happened to spell.
        unsafe {
            let library =
                libloading::Library::new("libgbm.so.1").map_err(|error| error.to_string())?;
            macro_rules! symbol {
                ($name:literal, $kind:ty) => {{
                    let found: libloading::Symbol<'_, $kind> =
                        library.get($name).map_err(|_| {
                            format!(
                                "libgbm has no {}",
                                CStr::from_bytes_with_nul_unchecked($name).to_string_lossy()
                            )
                        })?;
                    *found
                }};
            }
            Ok(Arc::new(Self {
                create_device: symbol!(
                    b"gbm_create_device\0",
                    unsafe extern "C" fn(c_int) -> *mut c_void
                ),
                device_destroy: symbol!(b"gbm_device_destroy\0", unsafe extern "C" fn(*mut c_void)),
                backend_name: symbol!(
                    b"gbm_device_get_backend_name\0",
                    unsafe extern "C" fn(*mut c_void) -> *const c_char
                ),
                bo_create: symbol!(
                    b"gbm_bo_create\0",
                    unsafe extern "C" fn(*mut c_void, u32, u32, u32, u32) -> *mut c_void
                ),
                bo_destroy: symbol!(b"gbm_bo_destroy\0", unsafe extern "C" fn(*mut c_void)),
                bo_get_fd: symbol!(
                    b"gbm_bo_get_fd\0",
                    unsafe extern "C" fn(*mut c_void) -> c_int
                ),
                bo_get_stride: symbol!(
                    b"gbm_bo_get_stride\0",
                    unsafe extern "C" fn(*mut c_void) -> u32
                ),
                bo_get_offset: symbol!(
                    b"gbm_bo_get_offset\0",
                    unsafe extern "C" fn(*mut c_void, c_int) -> u32
                ),
                bo_get_modifier: symbol!(
                    b"gbm_bo_get_modifier\0",
                    unsafe extern "C" fn(*mut c_void) -> u64
                ),
                bo_get_plane_count: symbol!(
                    b"gbm_bo_get_plane_count\0",
                    unsafe extern "C" fn(*mut c_void) -> c_int
                ),
                bo_map: symbol!(
                    b"gbm_bo_map\0",
                    unsafe extern "C" fn(
                        *mut c_void,
                        u32,
                        u32,
                        u32,
                        u32,
                        u32,
                        *mut u32,
                        *mut *mut c_void,
                    ) -> *mut c_void
                ),
                bo_unmap: symbol!(
                    b"gbm_bo_unmap\0",
                    unsafe extern "C" fn(*mut c_void, *mut c_void)
                ),
                library,
            }))
        }
    }
}

/// An allocator over one DRM node.
///
/// The node has to be open for reading **and** writing: `gbm_create_device` reaches the driver
/// through it and a read-only descriptor makes it fault rather than refuse.
#[derive(Debug)]
pub struct Device {
    library: Arc<Library>,
    raw: *mut c_void,
}

impl Device {
    /// Opens an allocator over `node`, which stays open for as long as this does.
    ///
    /// # Errors
    ///
    /// Returns a message where the driver would not give an allocator, which is what a node with
    /// no rendering support answers.
    pub fn new(library: &Arc<Library>, node: BorrowedFd<'_>) -> Result<Self, String> {
        // SAFETY: the descriptor is a DRM node the caller holds open for at least this call, and
        // gbm keeps no reference to it past `gbm_create_device`.
        let raw = unsafe { (library.create_device)(node.as_raw_fd()) };
        if raw.is_null() {
            return Err("gbm_create_device answered nothing".to_owned());
        }
        Ok(Self {
            library: Arc::clone(library),
            raw,
        })
    }

    /// The allocator itself, for an interface that takes one.
    ///
    /// EGL is the caller this exists for: a display over `EGL_PLATFORM_GBM_KHR` is made from this
    /// pointer, and there is no other way to name the device it stands for. The pointer is valid for
    /// as long as this [`Device`] is, and using it after that is the caller's to avoid.
    pub fn as_ptr(&self) -> *mut c_void {
        self.raw
    }

    /// The driver behind this allocator, for a log line that says which card a buffer came from.
    pub fn backend(&self) -> String {
        // SAFETY: `raw` is an allocator this made and has not destroyed.
        let name = unsafe { (self.library.backend_name)(self.raw) };
        if name.is_null() {
            return "unnamed".to_owned();
        }
        // SAFETY: gbm answers a static, nul-terminated name.
        unsafe { CStr::from_ptr(name) }
            .to_string_lossy()
            .into_owned()
    }

    /// Allocates one buffer a display can scan out of and a graphics device can draw into.
    ///
    /// `format` is a fourcc, the same code a framebuffer is registered under.
    ///
    /// # Errors
    ///
    /// Returns a message where the driver refused, which it does for a size, a format or a pair of
    /// uses it cannot satisfy at once. Nothing here can say which of the three it was: gbm answers
    /// a null pointer and no reason.
    pub fn create(&self, width: u32, height: u32, format: u32) -> Result<Allocation, String> {
        // SAFETY: `raw` is an allocator this made, and the arguments are plain values.
        let bo = unsafe {
            (self.library.bo_create)(self.raw, width, height, format, USE_SCANOUT | USE_RENDERING)
        };
        if bo.is_null() {
            return Err(format!(
                "gbm_bo_create refused a {width}x{height} buffer that is both drawn into and \
                 scanned out"
            ));
        }
        // SAFETY: every one of these reads a buffer this call just made and has not destroyed, and
        // each answers a plain scalar. One block rather than four, because they share the reason.
        //
        // `gbm_bo_get_handle` is deliberately absent. It answers a *union* by value, and the i386
        // ABI returns an aggregate through a hidden pointer rather than in registers — so a
        // declaration saying it answers a `u64` hands the buffer pointer over as that hidden
        // pointer, and the driver writes the handle through it. The handle comes from importing
        // the descriptor instead, which is what the Vulkan path does and is one call either way.
        let (planes, stride, offset, modifier) = unsafe {
            (
                (self.library.bo_get_plane_count)(bo).max(0) as usize,
                (self.library.bo_get_stride)(bo),
                (self.library.bo_get_offset)(bo, 0),
                (self.library.bo_get_modifier)(bo),
            )
        };
        let allocation = Allocation {
            library: Arc::clone(&self.library),
            bo,
            planes,
            stride,
            offset,
            modifier,
            descriptor: None,
        };
        if allocation.planes != 1 {
            return Err(format!(
                "gbm_bo_create answered a buffer in {} memory planes, and this carries one",
                allocation.planes
            ));
        }
        Ok(allocation)
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        // SAFETY: `raw` is an allocator this made, and every buffer from it holds an `Arc` on the
        // library rather than on this, so destroying it here frees only the allocator.
        unsafe { (self.library.device_destroy)(self.raw) };
    }
}

/// One buffer, and everything the two ends have to be told about it.
#[derive(Debug)]
pub struct Allocation {
    library: Arc<Library>,
    bo: *mut c_void,
    planes: usize,
    stride: u32,
    offset: u32,
    modifier: u64,
    /// The exported descriptor, once anything has asked for one.
    descriptor: Option<OwnedFd>,
}

impl Allocation {
    /// How long a row is, in bytes.
    pub fn stride(&self) -> u32 {
        self.stride
    }

    /// Where the first memory plane starts.
    pub fn offset(&self) -> u32 {
        self.offset
    }

    /// The layout the driver chose, or [`IMPLICIT`] where it named none.
    pub fn modifier(&self) -> u64 {
        self.modifier
    }

    /// A descriptor for this buffer, exported once and then lent out.
    ///
    /// Exported lazily and kept, because `gbm_bo_get_fd` makes a new descriptor on every call and
    /// a frame loop asking each time would run the process out of them.
    ///
    /// # Errors
    ///
    /// Returns a message where the driver would not export one, which is what a buffer that cannot
    /// leave its device answers.
    pub fn descriptor(&mut self) -> Result<BorrowedFd<'_>, String> {
        if self.descriptor.is_none() {
            // SAFETY: `bo` is a buffer this holds and has not destroyed.
            let raw = unsafe { (self.library.bo_get_fd)(self.bo) };
            if raw < 0 {
                return Err("gbm_bo_get_fd would not export this buffer".to_owned());
            }
            // SAFETY: `gbm_bo_get_fd` answers a descriptor nothing else owns.
            self.descriptor = Some(unsafe { OwnedFd::from_raw_fd(raw) });
        }
        Ok(self
            .descriptor
            .as_ref()
            .expect("just exported")
            .as_fd_borrowed())
    }
}

impl Allocation {
    /// Reads the pixel at the top-left corner, through the allocation's own mapping.
    ///
    /// For a test that has to know whether pixels **arrived**, which is a different question from
    /// whether a draw was accepted. Asking the graphics API instead would let it answer with what
    /// it was told, and that is exactly the failure worth catching: a copy into a buffer the device
    /// cannot reach reports success, runs at an impossible speed, and leaves the memory untouched.
    ///
    /// # Errors
    ///
    /// Returns a message where the driver would not map it, which a buffer in device-private
    /// memory answers.
    pub fn peek(&self) -> Result<[u8; 4], String> {
        let mut stride = 0_u32;
        let mut token: *mut c_void = core::ptr::null_mut();
        // SAFETY: `bo` is a buffer this holds, and one pixel is inside every allocation it makes.
        let address = unsafe {
            (self.library.bo_map)(
                self.bo,
                0,
                0,
                1,
                1,
                TRANSFER_READ,
                &raw mut stride,
                &raw mut token,
            )
        };
        if address.is_null() {
            return Err("gbm_bo_map would not map this buffer".to_owned());
        }
        // SAFETY: the mapping covers at least the one pixel that was asked for.
        let pixel = unsafe { core::slice::from_raw_parts(address.cast::<u8>(), 4) };
        let read = [pixel[0], pixel[1], pixel[2], pixel[3]];
        // SAFETY: the token is the one the mapping answered with.
        unsafe { (self.library.bo_unmap)(self.bo, token) };
        Ok(read)
    }
}

impl Drop for Allocation {
    fn drop(&mut self) {
        // SAFETY: `bo` is a buffer this holds, dropped once. The descriptor goes with it, and the
        // kernel keeps the memory alive for as long as anything that imported it holds a reference.
        unsafe { (self.library.bo_destroy)(self.bo) };
    }
}

/// Borrowing helper, so that the lifetime on [`Allocation::descriptor`] is the allocation's.
trait AsFdBorrowed {
    fn as_fd_borrowed(&self) -> BorrowedFd<'_>;
}

impl AsFdBorrowed for OwnedFd {
    fn as_fd_borrowed(&self) -> BorrowedFd<'_> {
        std::os::fd::AsFd::as_fd(self)
    }
}

// SAFETY: gbm's own objects carry no thread affinity; what they hold is a descriptor and driver
// state reached through it. The `Arc<Library>` keeps the mapping alive across threads, and nothing
// here is shared without an exclusive borrow.
unsafe impl Send for Device {}
// SAFETY: as above.
unsafe impl Send for Allocation {}

#[cfg(test)]
mod tests {
    use super::{IMPLICIT, Library};

    /// The machine running the tests need not have it, so this asserts on the answer rather than
    /// on the outcome: what must not happen is a panic or a fault inside the loader.
    #[test]
    fn loading_answers_rather_than_faults() {
        match Library::load() {
            Ok(_) => {}
            Err(why) => assert!(!why.is_empty(), "a refusal has to say something"),
        }
    }

    #[test]
    fn the_implicit_layout_is_the_kernels_own_code() {
        assert_eq!(IMPLICIT, 0x00ff_ffff_ffff_ffff, "DRM_FORMAT_MOD_INVALID");
    }
}
