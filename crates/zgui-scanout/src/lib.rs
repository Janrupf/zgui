//! Copying between the buffers a display scans out of, on the device that owns them.
//!
//! A program that presents through a rotating set of buffers owes each one the damage it missed
//! while it was not the one being written. Those pixels are already correct in whichever buffer was
//! presented last, so they need not be produced again — and where the renderer is on a *different*
//! device from the display, producing them again means sending them across the link a second time.
//! This crate copies them where they already are.
//!
//! It is worth having only where those two devices differ. On one device the copy costs what the
//! draw costs and the whole exercise is a wash. On a machine whose link is narrow it is the
//! difference between sending a frame's damage and sending several frames of it: the machine this
//! was written for has a single PCIe lane, where a copy runs about five times the rate the link
//! writes at.
//!
//! # What it does not decide
//!
//! Which buffer to draw into, how old each one is, and which rectangles each is owed. That is
//! arithmetic over the set and it needs no device, so it belongs to whoever presents. This crate
//! answers one question — *copy these rectangles from that buffer to this one* — and answers it on
//! the device the buffers live on.
//!
//! # Who waits
//!
//! [`Signalled`] is the answer, and the tiers are the point rather than an implementation detail. A
//! copy that hands back a descriptor lets the **kernel** wait for it, so the commit that follows
//! carries the wait and this program carries none. A copy that cannot is already finished by the
//! time it answers, because the wait had to happen here. See [`Copier::copy`].
//!
//! # Loading
//!
//! Everything is opened at run time. A build needs neither EGL nor libgbm nor a card, and a machine
//! that has none of them still starts: the absence arrives as an [`Error`], which a caller reads and
//! answers by sending the pixels the long way round, exactly as it did before.

#![deny(missing_docs)]
// This crate is on the unsafe ledger's allowlist for one reason: EGL and libgbm are opened at run
// time and every call into either is a call through a pointer this crate resolved itself. There is
// no safe spelling of that, and the alternative — linking them — is what stops the console session
// starting on a machine that has neither.
#![allow(unsafe_code)]

mod buffer;
mod copier;
mod error;

pub mod egl;

pub use crate::buffer::{Buffer, Rect};
pub use crate::copier::{Copier, Signalled};
pub use crate::error::Error;
