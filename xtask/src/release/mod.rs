//! What has to be true for the tree to be released.
//!
//! Two questions, and they are separate. *Can this be released at all* — one version everywhere,
//! every dependency expressed so that a registry can resolve it, and every published crate built
//! from the package a registry would receive. And *may this be released under that version* —
//! whether the public surface still keeps the promise the last release made.

pub(crate) mod lockstep;
pub(crate) mod semver;

use std::path::Path;

use crate::error::{Error, Result};
use crate::ledger::report::Report;
use crate::ledger::tree::Tree;
use crate::ledger::tree::features::FeatureSource;
use crate::process;

/// How far the packaging step goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Packaging {
    /// Builds every published crate from its package. A full build of the tree, so it runs when a
    /// release is checked on its own and not in every gate run.
    Built,
    /// Makes the packages and builds none of them.
    Listed,
}

/// Runs the lockstep check, packages every published crate, and runs the compatibility gate.
pub(crate) fn run(root: &Path, packaging: Packaging) -> Result<()> {
    let tree = Tree::gather(root, FeatureSource::Cargo)?;
    let report = lockstep::check(&tree);
    print(&report);
    if !report.is_clean() {
        return Err(Error::failed(format!(
            "{} lockstep violation(s); the tree cannot be published as it stands",
            report.violations.len()
        )));
    }
    package(root, &tree, packaging)?;
    semver::run(root)
}

/// Packages every published crate and, for [`Packaging::Built`], builds each one from its package.
///
/// This catches what the manifests alone do not show: a manifest a registry would refuse, a file
/// the package leaves out, or a build that reads outside its own directory. Cargo resolves the
/// dependencies between unpublished versions of this tree's crates against the packages it made
/// itself.
fn package(root: &Path, tree: &Tree, packaging: Packaging) -> Result<()> {
    let mut args = vec!["package", "--workspace", "--locked"];
    // `cargo package` makes a package of an unpublished member too, and then cannot resolve that
    // member's own unpublished dependencies.
    for member in &tree.members {
        if !member.manifest.is_published() {
            args.extend(["--exclude", member.name.as_str()]);
        }
    }
    match packaging {
        Packaging::Built => println!("package    building every published crate from its package"),
        Packaging::Listed => {
            println!("package    packaging every published crate, building none");
            // The gate runs before a change is committed, and a publish is made from a clean
            // checkout anyway.
            args.extend(["--no-verify", "--allow-dirty"]);
        }
    }
    process::run(root, &process::cargo(), &args, &[])
}

/// Prints the lockstep report.
fn print(report: &Report) {
    if report.is_clean() {
        println!("lockstep   ok    one version, and every internal dependency states it");
    } else {
        println!("lockstep   FAIL");
    }
    for violation in &report.violations {
        println!("    {violation}");
    }
    for skipped in &report.skipped {
        println!("    (skipped: {skipped})");
    }
}
