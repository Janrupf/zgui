//! Composing a frame straight into a buffer a display scans out of, on a machine with no Vulkan.
//!
//! The copied path reads a whole frame back to system memory and copies it into the display's
//! buffer. This is the path that does neither, built on OpenGL rather than on Vulkan: `libgbm`
//! allocates a buffer the display already owns, EGL imports the descriptor as a texture, and the
//! renderer composes into it where it lies.
//!
//! **The check is the buffer's own mapping, never the graphics API.** A copy into memory the device
//! cannot reach reports success, runs at an impossible speed and leaves the memory untouched; asking
//! GL what it drew answers with what it was told. Reading the allocation is the only question whose
//! answer cannot be a polite fiction.
//!
//! It needs no DRM master — nothing here sets a mode or flips — so it asserts on any machine with a
//! card, an OpenGL adapter and `libgbm`. A machine missing one of the three says so on standard
//! error and returns, the shape `cargo xtask ledger ignored` prescribes for a test that cannot be
//! switched off.

use std::path::PathBuf;

use zgui_platform_drm::import::gbm;
use zgui_platform_drm::import::gl;
use zgui_render_wgpu::{Gpu, SharedGraphics, wgpu};

/// Small, because what is being asserted is that the pixels arrive rather than how many.
const WIDTH: u32 = 256;
const HEIGHT: u32 = 128;

/// The colour a frame is composed in, and the bytes it has to reach memory as.
///
/// `XR24` is blue first, so a fully blue frame is `[0xFF, 0x00, 0x00, _]` — the fourth byte is what
/// the scanout ignores, and nothing here asserts on it.
const BLUE: wgpu::Color = wgpu::Color {
    r: 0.0,
    g: 0.0,
    b: 1.0,
    a: 1.0,
};

fn main() {
    let Some(gpu) = graphics() else {
        return;
    };
    if gpu.adapter().get_info().backend != wgpu::Backend::Gl {
        eprintln!(
            "this adapter is {:?} and this path is the OpenGL one, so there is nothing to assert",
            gpu.adapter().get_info().backend
        );
        return;
    }
    let Some(card) = card() else {
        return;
    };
    let library = match gbm::Library::load() {
        Ok(library) => library,
        Err(why) => {
            eprintln!("no libgbm on this machine, so nothing here can allocate: {why}");
            return;
        }
    };
    let node = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&card)
    {
        Ok(node) => node,
        Err(error) => {
            eprintln!("{} cannot be opened for writing: {error}", card.display());
            return;
        }
    };
    let allocator = match gbm::Device::new(&library, std::os::fd::AsFd::as_fd(&node)) {
        Ok(allocator) => allocator,
        Err(why) => {
            eprintln!("{} gives no allocator: {why}", card.display());
            return;
        }
    };
    a_frame_composed_into_an_imported_buffer_reaches_the_buffer(&gpu, &allocator);
    every_buffer_of_a_set_is_its_own_memory(&gpu, &allocator);
    the_tier_that_signals_a_frame_is_one_this_device_has(&gpu);
    println!("drawn_gl: every assertion held on {}", allocator.backend());
}

/// The whole of it: allocate, import, compose, and read the memory back.
fn a_frame_composed_into_an_imported_buffer_reaches_the_buffer(gpu: &Gpu, allocator: &gbm::Device) {
    let mut buffers = match gl::create(gpu, allocator, WIDTH, HEIGHT, 1) {
        Ok(buffers) => buffers,
        Err(why) => {
            eprintln!("this machine cannot draw into a scanout buffer: {why}");
            std::process::exit(0);
        }
    };
    let drawn = buffers.pop().expect("one was asked for");

    // Nothing about the colour is read back through GL, so it cannot answer with what it was told.
    let before = drawn.peek().expect("a gbm buffer maps");
    assert_ne!(
        before[0], 0xFF,
        "the buffer already held the colour this is about to draw, so nothing would be proved"
    );
    compose(gpu, drawn.texture(), BLUE);
    gl::finish(gpu, gl::signal(gpu, false));

    let after = drawn.peek().expect("a gbm buffer maps");
    assert_eq!(
        after[0], 0xFF,
        "a blue frame reaches XR24 memory blue first, and the buffer holds {after:?}"
    );
    assert_eq!(
        [after[1], after[2]],
        [0x00, 0x00],
        "and nothing else, so the channel order agrees end to end: {after:?}"
    );
}

/// Two buffers of one set have to be two allocations, or a flip shows the frame being drawn.
fn every_buffer_of_a_set_is_its_own_memory(gpu: &Gpu, allocator: &gbm::Device) {
    let Ok(buffers) = gl::create(gpu, allocator, WIDTH, HEIGHT, 2) else {
        return;
    };
    // Both are drawn into below and only one is drawn blue, which is what says they are two
    // allocations rather than two names for one.
    let [first, second] = &buffers[..] else {
        panic!("two were asked for and {} came back", buffers.len());
    };

    compose(gpu, first.texture(), BLUE);
    gl::finish(gpu, gl::signal(gpu, false));

    assert_eq!(
        first.peek().expect("a gbm buffer maps")[0],
        0xFF,
        "the first buffer holds the frame"
    );
    assert_ne!(
        second.peek().expect("a gbm buffer maps")[0],
        0xFF,
        "and the second one does not, so they are separate memory"
    );
}

/// Whichever tier a device lands on, it has to be one it can actually perform.
fn the_tier_that_signals_a_frame_is_one_this_device_has(gpu: &Gpu) {
    // Asked both ways round, because the top tier needs the display to take an in-fence as well as
    // the driver to export one, and a display that cannot must never be told it can.
    let without = gl::signal(gpu, false);
    assert_ne!(
        without,
        gl::Signal::Kernel,
        "a display that takes no in-fence has nowhere to put a descriptor, so the kernel cannot be \
         the one that waits"
    );
    let with = gl::signal(gpu, true);
    assert!(
        with == without || with == gl::Signal::Kernel,
        "offering an in-fence may raise the tier and may change nothing, and it did neither: \
         {without:?} became {with:?}"
    );
    eprintln!("drawn_gl: this device signals a frame by {with:?} where the display takes a fence");
}

/// Composes a frame of one colour into `texture`, through the device the renderer draws on.
fn compose(gpu: &Gpu, texture: &wgpu::Texture, colour: wgpu::Color) {
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu
        .device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("drawn_gl"),
        });
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("drawn_gl"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(colour),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    gpu.queue().submit([encoder.finish()]);
}

/// A graphics device, or a word on standard error about why there is none.
fn graphics() -> Option<std::sync::Arc<Gpu>> {
    match SharedGraphics::new().open_gpu() {
        Ok(gpu) => Some(gpu),
        Err(error) => {
            eprintln!("no graphics device on this machine: {error}");
            None
        }
    }
}

/// The first card, or a word about why there is none.
fn card() -> Option<PathBuf> {
    if let Ok(named) = std::env::var("ZGUI_DRM_DEVICE") {
        return Some(PathBuf::from(named));
    }
    let mut cards: Vec<PathBuf> = std::fs::read_dir("/dev/dri")
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("card"))
        })
        .collect();
    cards.sort();
    if cards.is_empty() {
        eprintln!("no card in /dev/dri, so there is no display to allocate for");
    }
    cards.pop()
}
