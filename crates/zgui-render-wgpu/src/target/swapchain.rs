//! Where a composed frame is copied to.

use zgui_bits::DamageSet;
use zgui_geom::{Device, Rect, Size};

/// How many disjoint rectangles a supplied texture's outstanding copy is tracked as.
///
/// Stated as a multiple of [`MAX_DAMAGE`] rather than as a number, because below it the set cannot
/// hold even one frame's damage: the rectangles merge, the merged ones cover the gaps between
/// them, and the copy writes pixels no frame ever changed. On a display whose buffers are on the
/// far side of a bus that is the largest thing a frame spends — measured at half as much again as
/// the frame's own damage, for a set one rectangle short of holding it.
///
/// Twice, because a texture is written every second or third frame and is owed what was drawn
/// while it was not. Frame to frame those rectangles mostly land on each other and absorb, so the
/// second copy is headroom rather than a count of anything.
///
/// The other end of the trade is a scissor and a draw per rectangle, inside a pass that is open
/// anyway. A draw costs about seventeen microseconds on the slowest driver this was measured
/// against, and a rectangle merged away wastes far more than that in pixels.
const STALE: usize = zgui_bits::MAX_DAMAGE * 2;

use crate::frame::damage::beyond;
use crate::gpu::device::Gpu;
use crate::gpu::formats::{self, Formats};
use crate::gpu::surface::ConfiguredSurface;
use crate::target::acquire::Acquisition;

/// A texture a frame can be copied into, and the answer that produced it.
pub struct Presented {
    /// Which answer the request got.
    pub acquisition: Acquisition,
    /// The acquired surface texture, when one came back and has to be presented.
    pub surface_texture: Option<wgpu::SurfaceTexture>,
    /// The view to copy into, when there is one.
    pub view: Option<wgpu::TextureView>,
}

/// What a frame is presented to.
///
/// A window's surface, or a texture standing in for one. The second is not a stub: it is
/// configured from the same format rules, copied into by the same pipeline through the same view,
/// and read back byte for byte — which is what lets the encoding decisions above be *measured*
/// rather than argued, on a machine with no window.
///
/// The third is a set of textures the caller owns and points at one of before each frame. It is
/// how a display controller's own scanout buffers are drawn into: the copy lands in the buffer the
/// hardware will read, so no frame passes through the processor on its way to a screen.
#[derive(Debug)]
pub enum Presentation {
    /// A real surface, acquired from and presented to every frame.
    Surface(Box<ConfiguredSurface>),
    /// A texture, standing in for a surface that cannot be created.
    Offscreen(Offscreen),
    /// Textures a caller owns, one of which it chooses before each frame.
    Supplied(Supplied),
}

impl Presentation {
    /// The formats everything is drawn in.
    pub fn formats(&self) -> Formats {
        match self {
            Self::Surface(surface) => surface.formats(),
            Self::Offscreen(offscreen) => offscreen.formats,
            Self::Supplied(supplied) => supplied.formats,
        }
    }

    /// The extent being presented at.
    pub fn size(&self) -> Size<i32, Device> {
        match self {
            Self::Surface(surface) => surface.size(),
            Self::Offscreen(offscreen) => offscreen.size,
            Self::Supplied(supplied) => supplied.size,
        }
    }

    /// The presentation mode a real surface holds; a target with no swap chain has none.
    pub fn present_mode(&self) -> Option<wgpu::PresentMode> {
        match self {
            Self::Surface(surface) => Some(surface.present_mode()),
            Self::Offscreen(_) => None,
            // A caller that supplies the textures also decides when one is shown, so there is no
            // swap chain here to hold a mode.
            Self::Supplied(_) => None,
        }
    }

    /// Whether a frame may be recorded against this at all.
    pub fn is_configured(&self) -> bool {
        match self {
            Self::Surface(surface) => surface.is_configured(),
            // A texture the renderer allocated is ready as soon as it exists: there is no swap
            // chain to negotiate and nothing that waits for a device.
            Self::Offscreen(_) => true,
            Self::Supplied(supplied) => supplied.is_configured(),
        }
    }

    /// Resizes, recording a surface's new extent or reallocating a texture.
    ///
    /// A surface's swap chain is not rebuilt here — [`Presentation::apply_pending`] does that, at
    /// the one point in a frame where the wait it costs overlaps work already done.
    ///
    /// A supplied set is only told, because the renderer did not create those textures and cannot
    /// create another. A display's mode holds still while a program runs, and the caller that owns
    /// the buffers supplies a new set when it changes; until it does, the set and the target
    /// disagree and [`Presentation::is_configured`] answers `false`.
    pub fn resize(&mut self, gpu: &Gpu, size: Size<i32, Device>) {
        match self {
            Self::Surface(surface) => surface.resize(size),
            Self::Offscreen(offscreen) => *offscreen = offscreen.resized(gpu, size),
            Self::Supplied(supplied) => supplied.retarget(size),
        }
    }

    /// Rebuilds a surface's swap chain if one is owed.
    pub fn apply_pending(&mut self, gpu: &Gpu) {
        match self {
            Self::Surface(surface) => surface.apply(gpu),
            // A texture owes nothing, supplied or standing in for a surface: nothing about either
            // waits for a device. A supplied set that disagrees with the target is not waiting for
            // this either — only the caller can end that, by supplying a set at the new extent.
            Self::Offscreen(_) | Self::Supplied(_) => {}
        }
    }

    /// Asks for something to copy this frame into.
    pub fn acquire(&self) -> Presented {
        match self {
            Self::Surface(surface) => {
                let acquired = surface.surface().get_current_texture();
                let acquisition = Acquisition::classify(&acquired);
                let surface_texture = match acquired {
                    wgpu::CurrentSurfaceTexture::Success(texture)
                    | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => Some(texture),
                    _ => None,
                };
                let view = surface_texture
                    .as_ref()
                    .map(|texture| surface.present_view(&texture.texture));
                Presented {
                    acquisition,
                    surface_texture,
                    view,
                }
            }
            Self::Offscreen(offscreen) => Presented {
                acquisition: Acquisition::Success,
                surface_texture: None,
                view: Some(offscreen.view()),
            },
            // The caller already chose which texture this is, so there is nothing to ask for and
            // nothing to hand back afterwards: whoever owns the textures presents them.
            Self::Supplied(supplied) => Presented {
                acquisition: Acquisition::Success,
                surface_texture: None,
                view: Some(supplied.view()),
            },
        }
    }

    /// Notes that the surface asked to be reconfigured at the next opportunity.
    pub fn request_reconfigure(&mut self) {
        match self {
            Self::Surface(surface) => surface.request_reconfigure(),
            // Only a swap chain has a configuration to rebuild.
            Self::Offscreen(_) | Self::Supplied(_) => {}
        }
    }

    /// Whether a reconfiguration is owed before the next frame.
    pub fn reconfigure_pending(&self) -> bool {
        match self {
            Self::Surface(surface) => surface.reconfigure_pending(),
            // Only a swap chain is ever owed a reconfiguration, and neither of these is one.
            Self::Offscreen(_) | Self::Supplied(_) => false,
        }
    }

    /// Marks the configuration as no longer describing the window.
    pub fn invalidate(&mut self) {
        match self {
            Self::Surface(surface) => surface.invalidate(),
            // Neither describes a window, so neither can stop describing one.
            Self::Offscreen(_) | Self::Supplied(_) => {}
        }
    }
}

/// A texture standing in for a surface.
///
/// It exists so that a frame can be composed, copied and read back with no window: the pixel
/// suites, the startup pattern and the encoding measurements all run through this. Its format is
/// chosen by the same rules a real surface's is, so a test can present into an encoded target and
/// watch the fallback tier cancel the encode.
#[derive(Debug)]
pub struct Offscreen {
    /// The texture.
    texture: wgpu::Texture,
    /// Its extent.
    size: Size<i32, Device>,
    /// The formats derived for it.
    formats: Formats,
}

impl Offscreen {
    /// The usage a stand-in surface needs: copied into, and copied out of by a test.
    const USAGE: wgpu::TextureUsages = wgpu::TextureUsages::RENDER_ATTACHMENT
        .union(wgpu::TextureUsages::COPY_SRC)
        .union(wgpu::TextureUsages::TEXTURE_BINDING);

    /// A stand-in surface of `size` presenting in `format`.
    ///
    /// `mutable_texture_formats` is what a device would have reported; passing it explicitly is
    /// what lets both fallbacks for an encoded surface be exercised on one machine.
    pub fn new(
        gpu: &Gpu,
        size: Size<i32, Device>,
        format: wgpu::TextureFormat,
        mutable_texture_formats: bool,
    ) -> Self {
        let formats = formats::choose(
            &[format],
            &[wgpu::CompositeAlphaMode::Opaque],
            true,
            mutable_texture_formats,
        );
        debug_assert!(
            formats.is_sound(),
            "an encoded stand-in surface with nothing to cancel the encode: {formats:?}"
        );
        let view_formats = formats.view_formats();
        let texture = gpu.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("zgui.offscreen"),
            size: wgpu::Extent3d {
                width: size.width.max(1) as u32,
                height: size.height.max(1) as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: formats.surface,
            usage: Self::USAGE,
            view_formats: &view_formats,
        });
        Self {
            texture,
            size: size.non_negative(),
            formats,
        }
    }

    /// The same stand-in surface at a new extent.
    fn resized(&self, gpu: &Gpu, size: Size<i32, Device>) -> Self {
        Self::new(
            gpu,
            size,
            self.formats.surface,
            self.formats.view_format_twin.is_some(),
        )
    }

    /// The texture, for a copy out of it.
    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    /// The formats it was derived with.
    pub fn formats(&self) -> Formats {
        self.formats
    }

    /// The view a frame is copied into, through the unencoded twin where there is one.
    fn view(&self) -> wgpu::TextureView {
        match self.formats.view_format_twin {
            Some(format) => self.texture.create_view(&wgpu::TextureViewDescriptor {
                format: Some(format),
                ..Default::default()
            }),
            None => self
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default()),
        }
    }
}

/// Fills rectangles of one supplied texture from another, without the renderer's device.
///
/// **The pixels a rotated texture is owed are already correct in whichever one was written last.**
/// Where the supplied textures live on a different device from the renderer — a display card on the
/// far side of a link — producing them again means sending them across that link a second time, and
/// they are already on the far side. A caller that can copy between its own textures there installs
/// one of these with [`Supplied::attach_peer`], and what crosses the link falls to what nothing on
/// the far side has yet.
///
/// The copy has finished when `copy` answers, and that is deliberate rather than lazy. Splitting it
/// so that a frame is composed inside it was tried and measured: about three quarters of a copy is
/// the device working, and starting it when the buffer is chosen really does hide a millisecond of
/// that behind the composition — and the frame rate does not move, because the wait simply
/// relocates. What is left is a contract with an ordering hazard in it, for nothing.
pub trait PeerCopy: std::fmt::Debug {
    /// Copies each of `rects` from the texture at `from` to the texture at `to`.
    ///
    /// Answers whether it did. A refusal is not fatal and not fatal to the frame: the caller sends
    /// the rectangles the long way instead, which is what it did before one of these was attached.
    ///
    /// **Whether this waits for the copy is the peer's own business, and it should not.** The
    /// rectangles it fills are the ones this frame will not draw, so nothing here reads them; and
    /// the copy reads a texture the renderer's device may still be writing, so a peer that waits
    /// waits for that device — 11.3 ms of a 20.5 ms frame on the machine this was written for.
    /// A peer whose completion the caller's own presentation already covers answers at once, and
    /// the copy runs while the frame is recorded and submitted.
    fn copy(&mut self, from: usize, to: usize, rects: &[Rect<i32, Device>]) -> bool;
}

/// What the copy at the end of a frame owes, and what a peer serviced instead.
#[derive(Debug, Default)]
pub struct Owed {
    /// The rectangles that have to come from the composed target, which is the renderer's device.
    pub from_composed: Vec<Rect<i32, Device>>,
    /// How many pixels a peer copy filled, and therefore did not cross a link.
    pub repaired: u64,
}

/// How many pixels a list of rectangles covers.
fn area(rects: &[Rect<i32, Device>]) -> u64 {
    rects
        .iter()
        .map(|rect| crate::frame::damage::area(*rect))
        .sum()
}

/// Textures a caller supplies and rotates between.
///
/// What a display controller scans out of. The buffers belong to whatever drives the display, and
/// there are several of them so that the hardware reads one while the next is drawn. The caller
/// points at one before each frame, and the renderer copies its composed target straight into the
/// buffer that reaches the screen, so no frame is read back and none is copied through memory.
#[derive(Debug)]
pub struct Supplied {
    /// The textures, in the order the caller gave them.
    textures: Vec<wgpu::Texture>,
    /// Which one the next frame is copied into.
    selected: usize,
    /// The extent every one of them has.
    size: Size<i32, Device>,
    /// The formats derived from them.
    formats: Formats,
    /// Per texture: what was written into some *other* texture since this one was last written.
    ///
    /// A supplied set is rotated through, so a frame lands in one of them and what it changed is
    /// owed to every one it missed. Without this a copy limited to what changed would show, on
    /// alternate frames, whatever that texture held two frames ago.
    ///
    /// A [`DamageSet`] rather than a list, because it is the same problem the damage set already
    /// solves — keep a bounded number of disjoint rectangles and merge the cheapest pair when one
    /// more arrives — and a second implementation of it would be a second set of edge cases. The
    /// capacity is its own: this set costs a scissor and a draw per rectangle inside one pass,
    /// where the renderer's costs a whole pass, so it can afford to be finer.
    stale: Vec<DamageSet<STALE>>,
    /// Whether the caller reads each texture out and throws it away rather than rotating them.
    ///
    /// A display that composites its own frames hands out staging buffers: it copies exactly what
    /// a frame wrote onto a buffer of its own and never reads one again. There is nothing for such
    /// a texture to owe, and a debt would be pixels sent for nobody. Set by whoever supplied them,
    /// because only it knows what it does with them afterwards.
    consumed: bool,
    /// Which texture was written in full last, where one was.
    ///
    /// The donor a repair reads from. It is the freshest of the set by construction — it was
    /// written more recently than the one being written now, so what it lacks is a subset of what
    /// the slot lacks, and copying the slot's whole debt from it and then writing the donor's own
    /// debt over the top leaves the slot current. See [`Supplied::owed`].
    donor: Option<usize>,
    /// What can copy between these textures without the renderer's device, where anything can.
    peer: Option<Box<dyn PeerCopy>>,
    /// Whether the renderer was configured for an extent these textures do not have.
    ///
    /// The renderer cannot reallocate a supplied set, so this is a state a frame has to stop in
    /// rather than one it can draw through. The copy that ends a frame covers the whole buffer
    /// while reading a composed target of the other extent, which would put a correct corner, a
    /// stretch and a black remainder on a screen, every frame, with nothing to say so.
    diverged: bool,
}

impl Supplied {
    /// Creates a presentation over `textures`, of which the first is written next.
    ///
    /// The extent is the textures' own. Answers `None` when they cannot be presented to as one
    /// set; [`Supplied::unusable`] says why, and this writes that reason to the log.
    pub fn new(textures: Vec<wgpu::Texture>) -> Option<Self> {
        if let Some(reason) = Self::unusable(&textures) {
            tracing::error!(%reason, "the supplied textures were refused");
            return None;
        }
        let first = textures.first()?;
        let size = Size::new(first.width() as i32, first.height() as i32);
        // Every texture holds nothing, so every one of them owes all of it.
        let stale = vec![DamageSet::<STALE>::full(); textures.len()];
        // Derived from the textures themselves. The texture already answers its format, and a
        // second statement of it beside them is a way for the two to disagree.
        //
        // No mutable-format view is claimed, because a texture that exists cannot be given another
        // view format afterwards. An encoded texture therefore has its encode cancelled in the copy
        // that ends a frame, which asks nothing of the texture at all.
        let formats = formats::choose(
            &[first.format()],
            &[wgpu::CompositeAlphaMode::Opaque],
            true,
            false,
        );
        debug_assert!(
            formats.is_sound(),
            "an encoded supplied texture with nothing to cancel the encode: {formats:?}"
        );
        Some(Self {
            stale,
            // Rotated until whoever supplied them says otherwise, which is the safe direction: a
            // set wrongly called consumed shows a stale rectangle, and one wrongly called rotated
            // sends pixels nobody needed.
            consumed: false,
            textures,
            selected: 0,
            size,
            formats,
            donor: None,
            peer: None,
            diverged: false,
        })
    }

    /// Returns why `textures` cannot be presented to as one set, or `None` when they can.
    ///
    /// Every question here is answered by the handle itself, and every answer is otherwise fatal:
    /// wgpu's default uncaptured-error handler panics, so a texture that cannot be a colour
    /// attachment takes the program down inside the first frame's render pass. So the questions
    /// are asked while the caller can still act on the answer.
    ///
    /// The set also has to agree with itself, because one [`Formats`] and one extent are derived
    /// for all of it. A set that disagreed would present the frames landing on one texture
    /// correctly and corrupt the frames landing on another.
    ///
    /// ```
    /// use zgui_render_wgpu::target::swapchain::Supplied;
    ///
    /// // The one refusal that needs no device to reach: a set with nothing in it.
    /// assert!(Supplied::unusable(&[]).is_some());
    /// ```
    pub fn unusable(textures: &[wgpu::Texture]) -> Option<String> {
        let Some(first) = textures.first() else {
            return Some("the set is empty, so it states no format and no extent".to_owned());
        };
        for (slot, texture) in textures.iter().enumerate() {
            if !texture
                .usage()
                .contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
            {
                return Some(format!(
                    "texture {slot} is {:?} and a frame is copied into it, which needs \
                     RENDER_ATTACHMENT",
                    texture.usage()
                ));
            }
            if texture.dimension() != wgpu::TextureDimension::D2 {
                return Some(format!(
                    "texture {slot} is {:?}; a frame is copied into a two-dimensional attachment",
                    texture.dimension()
                ));
            }
            if texture.mip_level_count() != 1 {
                return Some(format!(
                    "texture {slot} has {} mip levels; a frame is copied into one",
                    texture.mip_level_count()
                ));
            }
            if texture.sample_count() != 1 {
                return Some(format!(
                    "texture {slot} takes {} samples; the copy that ends a frame resolves nothing",
                    texture.sample_count()
                ));
            }
            if texture.depth_or_array_layers() != 1 {
                return Some(format!(
                    "texture {slot} has {} layers; a frame is copied into one",
                    texture.depth_or_array_layers()
                ));
            }
            if texture.format() != first.format() {
                return Some(format!(
                    "texture {slot} is {:?} where the first is {:?}; one set is presented in one \
                     format",
                    texture.format(),
                    first.format()
                ));
            }
            if texture.size() != first.size() {
                return Some(format!(
                    "texture {slot} is {}×{} where the first is {}×{}; one set is presented at one \
                     extent",
                    texture.width(),
                    texture.height(),
                    first.width(),
                    first.height()
                ));
            }
        }
        None
    }

    /// Points at the texture the next frame is copied into.
    ///
    /// Answers whether the slot exists. A slot outside the set leaves the selection where it was
    /// and never wraps, because a wrapped slot is a buffer the caller did not choose — and the
    /// caller is the only party that knows which buffers the display controller has finished with.
    /// Guessing one here would put a frame on a buffer for reasons of arithmetic.
    #[must_use = "a refused slot leaves the frame going where it was already going"]
    pub fn select(&mut self, slot: usize) -> bool {
        if slot >= self.textures.len() {
            tracing::warn!(
                slot,
                supplied = self.textures.len(),
                selected = self.selected,
                "a slot outside the supplied textures was asked for, so the selection stands"
            );
            return false;
        }
        self.selected = slot;
        true
    }

    /// Records the extent the renderer was configured for.
    ///
    /// A supplied set cannot be reallocated, so a target of another extent puts the two out of
    /// step rather than resizing anything. That is recorded rather than performed:
    /// [`Supplied::is_configured`] answers `false` until the extents agree again, which stops
    /// frames instead of stretching them across a buffer.
    fn retarget(&mut self, size: Size<i32, Device>) {
        let wanted = Size::new(size.width.max(1), size.height.max(1));
        let diverged = wanted != self.size;
        if diverged != self.diverged {
            if diverged {
                tracing::warn!(
                    width = wanted.width,
                    height = wanted.height,
                    supplied_width = self.size.width,
                    supplied_height = self.size.height,
                    "the renderer was configured for an extent the supplied textures do not have; \
                     frames stop until a set at that extent is supplied"
                );
            } else {
                tracing::info!("the target agrees with the supplied textures again");
            }
        }
        self.diverged = diverged;
    }

    /// Returns `true` while a frame may be copied into this set.
    ///
    /// `false` once the renderer has been configured for an extent these textures do not have. The
    /// caller ends that by supplying a set at the new extent, or by configuring the target back.
    pub fn is_configured(&self) -> bool {
        !self.diverged
    }

    /// Records what this frame wrote against every texture, and takes what `slot` is owed.
    ///
    /// The debt is what the copy at the end of the frame has to cover: the rectangles this frame
    /// drew **and** the ones drawn while this texture was not the one being written. Taking it
    /// leaves the texture owing nothing, because what follows writes exactly those.
    ///
    /// # Where a peer services part of it
    ///
    /// With a [`PeerCopy`] attached the debt is split, and the split is the point. Call the slot
    /// being written *S* and the donor — the one written in full last — *D*.
    ///
    /// * Everything S lacks, D already has, **except what D itself lacks**. D was written more
    ///   recently, so its own debt is a subset of S's.
    /// * So the peer copies S's whole debt out of D, and the copy that follows writes D's debt over
    ///   the top of it. A pixel in S's debt and not in D's was last changed before D was written,
    ///   which is why D's copy of it is current; a pixel in both is written twice and ends current.
    ///
    /// What crosses to the renderer's device therefore falls from *everything S lacks* to
    /// *everything D lacks*, which for a set rotated once a frame is one frame's damage rather than
    /// as many frames as the set is deep. No rectangle arithmetic is needed for it: the second write
    /// covering part of the first is what makes the answer exact.
    ///
    /// A peer that refuses leaves the whole debt to be sent, which is what happens with none
    /// attached.
    pub fn owed(&mut self, slot: usize, rects: &[Rect<i32, Device>]) -> Owed {
        // **A set that is read out and thrown away every frame owes nothing.** Where the caller
        // composites what it was handed onto a buffer of its own, this frame's rectangles are the
        // whole of what it needs and a debt would be pixels sent across a link for nobody.
        if self.consumed {
            return Owed {
                from_composed: rects.to_vec(),
                repaired: 0,
            };
        }
        let whole = Rect::new(zgui_geom::Point::new(0, 0), self.size);
        for held in &mut self.stale {
            for rect in rects {
                held.absorb(*rect);
            }
        }
        let Some(held) = self.stale.get_mut(slot) else {
            return Owed {
                from_composed: vec![whole],
                repaired: 0,
            };
        };
        let debt = if held.is_full() {
            vec![whole]
        } else {
            held.rects().to_vec()
        };
        *held = DamageSet::<STALE>::new();
        // Read after this frame's rectangles were absorbed, so it already covers them. That is what
        // makes it the whole of what the copy has to carry.
        let donor_owes = self
            .donor
            .and_then(|donor| self.stale.get(donor))
            .map_or_else(|| vec![whole], |held| held.rects().to_vec());

        let answer = self.repair(slot, donor_owes, debt);
        self.donor = Some(slot);
        answer
    }

    /// Hands the slot's debt to the peer, and answers what is left to send.
    ///
    /// `donor_owes` is what even the donor lacks, read after this frame's rectangles were absorbed
    /// — so it already covers them, and it is the whole of what has to come from the composed
    /// target once the donor has supplied the rest.
    fn repair(
        &mut self,
        slot: usize,
        donor_owes: Vec<Rect<i32, Device>>,
        debt: Vec<Rect<i32, Device>>,
    ) -> Owed {
        let sent_whole = || Owed {
            from_composed: debt.clone(),
            repaired: 0,
        };
        let Some(donor) = self.donor.filter(|donor| *donor != slot) else {
            return sent_whole();
        };
        if self.stale.get(donor).is_none_or(DamageSet::is_full) {
            return sent_whole();
        }
        // **Only where it saves something.** A repair replaces what the slot lacks with what the
        // donor lacks, so a donor that lacks as much buys nothing and the copy is pure addition.
        // That is not a corner case: a scene whose damage is one rectangle in the same place every
        // frame — a scrolling panel is exactly that — has the two equal, and repairing it would copy
        // the panel locally and then send the panel anyway.
        let (owed_here, owed_there) = (area(&debt), area(&donor_owes));
        if owed_there >= owed_here {
            return sent_whole();
        }
        let Some(peer) = self.peer.as_mut() else {
            return sent_whole();
        };
        // **What the copy that follows will write anyway is not worth copying.** The two overlap by
        // construction — the donor's debt is a subset of the slot's — so the peer carries the
        // difference rather than the whole, cut out rectangle by rectangle. Dropping whole
        // rectangles caught 8% of it; the parts of a rectangle are most of the rest.
        let carried = beyond(&debt, &donor_owes);
        if !peer.copy(donor, slot, &carried) {
            return sent_whole();
        }
        Owed {
            from_composed: donor_owes,
            repaired: area(&carried),
        }
    }

    /// Records that the caller reads each texture out and throws it away.
    ///
    /// See [`Supplied::consumed`]. A caller that composites what it is handed onto a buffer of its
    /// own says so here, and every frame then carries its own rectangles and no debt.
    pub fn is_consumed(&mut self, consumed: bool) {
        self.consumed = consumed;
    }

    /// Installs what can copy between these textures without the renderer's device.
    ///
    /// Answers what was there before. See [`PeerCopy`] for when one is worth having, which is
    /// narrower than it looks: on one device the copy costs what the drawing costs.
    pub fn attach_peer(&mut self, peer: Box<dyn PeerCopy>) -> Option<Box<dyn PeerCopy>> {
        self.peer.replace(peer)
    }

    /// Which texture the next frame is copied into.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Returns how many textures a caller supplied.
    ///
    /// Always one or more: [`Supplied::new`] refuses an empty set.
    #[allow(
        clippy::len_without_is_empty,
        reason = "an empty set is refused at construction, so the question has one answer"
    )]
    pub fn len(&self) -> usize {
        self.textures.len()
    }

    /// Returns the texture the next frame is copied into, for a copy out of it.
    pub fn texture(&self) -> &wgpu::Texture {
        &self.textures[self.selected]
    }

    /// Returns the extent every texture in the set has.
    ///
    /// The textures' own, read off the first of them. A caller comparing this against what it
    /// intends to present is comparing against the buffers themselves.
    pub fn size(&self) -> Size<i32, Device> {
        self.size
    }

    /// Returns the formats derived from the textures.
    pub fn formats(&self) -> Formats {
        self.formats
    }

    /// Returns the view a frame is copied into.
    ///
    /// The texture's own format, always: a supplied set claims no unencoded twin, because the view
    /// format of a texture is fixed when the texture is created.
    fn view(&self) -> wgpu::TextureView {
        self.texture()
            .create_view(&wgpu::TextureViewDescriptor::default())
    }
}

#[cfg(test)]
mod tests {
    //! What a repair may leave to somebody else, checked pixel by pixel.
    //!
    //! [`beyond`] decides what a peer copy carries, and the copy that follows covers the rest. A
    //! pixel in neither is a pixel nothing writes — one frame of the wrong colour, in a place that
    //! depends on where two damage sets happened to overlap, which is the kind of fault that is
    //! never reproduced and never found. So it is checked exhaustively over a small grid rather
    //! than argued about.

    use super::{Rect, beyond};
    use zgui_geom::{Point, Size};

    /// A rectangle from its edges, which is how the cases below read best.
    fn at(left: i32, top: i32, right: i32, bottom: i32) -> Rect<i32, zgui_geom::Device> {
        Rect::new(Point::new(left, top), Size::new(right - left, bottom - top))
    }

    /// Whether `rects` covers the pixel whose top-left corner is `(x, y)`.
    fn covers(rects: &[Rect<i32, zgui_geom::Device>], x: i32, y: i32) -> bool {
        rects.iter().any(|rect| {
            x >= rect.origin.x
                && y >= rect.origin.y
                && x < rect.origin.x + rect.size.width
                && y < rect.origin.y + rect.size.height
        })
    }

    /// The two properties, over every pixel of a grid that contains everything asked about.
    fn holds(rects: &[Rect<i32, zgui_geom::Device>], cut: &[Rect<i32, zgui_geom::Device>]) {
        let carried = beyond(rects, cut);
        for y in -1..14 {
            for x in -1..14 {
                let inside = covers(rects, x, y);
                let written = covers(cut, x, y);
                let taken = covers(&carried, x, y);
                // Owed and unwritten implies carried: nothing may fall between the two copies.
                assert!(
                    !inside || written || taken,
                    "({x}, {y}) is owed and nothing carries or writes it: \
                     rects={rects:?} cut={cut:?} carried={carried:?}"
                );
                // Carried implies owed: the cut may leave too much, never something new.
                assert!(
                    !taken || inside,
                    "({x}, {y}) is carried and was never owed: \
                     rects={rects:?} cut={cut:?} carried={carried:?}"
                );
            }
        }
    }

    #[test]
    fn what_is_carried_covers_everything_the_other_copy_will_not_write() {
        // A miss, a total cover, an edge, a corner, a bite out of the middle — and two bites, which
        // is the case that splinters.
        holds(&[at(2, 2, 10, 10)], &[]);
        holds(&[at(2, 2, 10, 10)], &[at(20, 20, 30, 30)]);
        holds(&[at(2, 2, 10, 10)], &[at(0, 0, 12, 12)]);
        holds(&[at(2, 2, 10, 10)], &[at(2, 2, 10, 6)]);
        holds(&[at(2, 2, 10, 10)], &[at(0, 0, 6, 6)]);
        holds(&[at(2, 2, 10, 10)], &[at(4, 4, 8, 8)]);
        holds(&[at(2, 2, 10, 10)], &[at(4, 4, 8, 8), at(3, 3, 5, 9)]);
        holds(
            &[at(0, 0, 6, 6), at(6, 6, 12, 12)],
            &[at(4, 4, 8, 8), at(0, 5, 12, 7)],
        );
    }

    #[test]
    fn a_rectangle_that_splinters_is_carried_whole_rather_than_in_pieces() {
        // Five separate bites out of one rectangle would leave more pieces than are worth tracking.
        // Carrying it whole is bigger and still correct, which the properties above also check.
        let rect = at(0, 0, 12, 12);
        let cut = [
            at(1, 1, 2, 2),
            at(4, 4, 5, 5),
            at(7, 7, 8, 8),
            at(9, 2, 10, 3),
            at(2, 9, 3, 10),
        ];
        holds(&[rect], &cut);
        assert_eq!(
            beyond(&[rect], &cut),
            vec![rect],
            "a rectangle in too many pieces is kept whole"
        );
    }
}
