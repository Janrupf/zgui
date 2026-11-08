//! One version for the whole tree.
//!
//! Every crate here is released together at one version. That is a decision about what a consumer
//! has to reason about: with it, `zgui 0.1.0` names exactly one build of everything underneath, and
//! a compatibility matrix between our own crates never comes into existence.
//!
//! Three things have to hold for it, and none of them is visible until a release is attempted.
//! A member that spells its own version instead of inheriting the workspace's drifts silently. A
//! dependency written as a bare path publishes to no registry at all: a path is a fact about this
//! checkout, so a package whose dependency has no version requirement beside the path is rejected
//! the moment somebody runs the publish. That holds for a path into this tree and for a path out
//! of it, written in the member or inherited from `[workspace.dependencies]`. And a published
//! crate cannot depend on a member that is not published, because the registry has nothing to
//! resolve the requirement against. An optional or target-gated dependency is no exception.

use std::collections::BTreeMap;

use crate::ledger::report::Report;
use crate::ledger::tree::Tree;
use crate::ledger::tree::manifest::{Dependency, Manifest, Section};

/// Checks that every member shares the workspace version and can be published against it.
pub(crate) fn check(tree: &Tree) -> Report {
    let mut report = Report::clean();
    let Some(version) = workspace_version(&tree.manifest) else {
        report.skip("the workspace manifest declares no `[workspace.package] version`".to_owned());
        return report;
    };

    // Every member, and whether it is published.
    let members: BTreeMap<&str, bool> = tree
        .members
        .iter()
        .map(|member| (member.name.as_str(), member.manifest.is_published()))
        .collect();
    let inherited: BTreeMap<String, Dependency> = tree
        .manifest
        .workspace_dependencies()
        .into_iter()
        .map(|dependency| (dependency.key.clone(), dependency))
        .collect();

    for member in &tree.members {
        let at = &member.manifest.rel_path;
        if !inherits_version(&member.manifest) {
            report.violation(
                at.clone(),
                "write `version.workspace = true`: every crate here is released at one version"
                    .to_owned(),
            );
        }
        if !member.manifest.is_published() {
            continue;
        }
        for dependency in member.manifest.dependencies() {
            // An inherited entry states its path and version in the workspace manifest.
            let (path, declared) = match inherited.get(&dependency.key) {
                Some(entry) if dependency.inherited => {
                    (entry.path.as_deref(), entry.version.as_deref())
                }
                _ => (dependency.path.as_deref(), dependency.version.as_deref()),
            };
            // A development dependency with no version is stripped from the manifest a registry
            // receives, so a bare path is legal there and only there. One with a version stays,
            // and the registry has to resolve it like any other.
            if dependency.section == Section::Dev && declared.is_none() {
                continue;
            }
            if path.is_some() && declared.is_none() {
                report.violation(
                    at.clone(),
                    format!(
                        "`{}` is a bare path dependency: add a version beside the path, or the \
                         crate cannot be published",
                        dependency.name
                    ),
                );
                continue;
            }
            let Some(&published) = members.get(dependency.name.as_str()) else {
                continue;
            };
            if !published {
                report.violation(
                    at.clone(),
                    format!(
                        "`{}` is not published, so a published crate cannot depend on it, not \
                         even optionally or for one target",
                        dependency.name
                    ),
                );
            }
            if let Some(declared) = declared
                && declared != version
            {
                report.violation(
                    at.clone(),
                    format!(
                        "`{}` asks for `{declared}` while the workspace is at `{version}`: the \
                         tree is released in lockstep",
                        dependency.name
                    ),
                );
            }
        }
    }
    report
}

/// The `[workspace.package] version` every member inherits.
pub(crate) fn workspace_version(manifest: &Manifest) -> Option<String> {
    manifest
        .workspace_table("package")?
        .get("version")?
        .as_str()
        .map(str::to_owned)
}

/// Whether the member inherits its version rather than spelling one.
fn inherits_version(manifest: &Manifest) -> bool {
    manifest
        .text
        .lines()
        .any(|line| line.trim() == "version.workspace = true")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::ledger::tree::Tree;
    use crate::ledger::tree::features::FeatureSource;

    fn check(fixture: &str) -> crate::ledger::report::Report {
        let root = crate::root::workspace_root().expect("workspace root");
        let directory = root.join("xtask/fixtures/lockstep").join(fixture);
        let tree = Tree::gather(Path::new(&directory), FeatureSource::Recorded).expect("fixture");
        super::check(&tree)
    }

    #[test]
    fn accepts_the_workspace() {
        let root = crate::root::workspace_root().expect("workspace root");
        let tree = Tree::gather(&root, FeatureSource::Recorded).expect("workspace");
        let report = super::check(&tree);
        assert!(report.is_clean(), "{:#?}", report.violations);
    }

    #[test]
    fn accepts_its_clean_fixture() {
        let report = check("clean");
        assert!(report.is_clean(), "{:?}", report.violations);
    }

    #[test]
    fn rejects_every_violation_planted_in_its_fixture() {
        let report = check("planted");
        let messages: Vec<String> = report.violations.iter().map(ToString::to_string).collect();
        assert_eq!(messages.len(), 3, "{messages:#?}");
        let names = |needle: &str| messages.iter().any(|message| message.contains(needle));
        assert!(names("`cephal` is a bare path dependency"), "{messages:#?}");
        assert!(
            names("zgui/fixture.Cargo.toml") && names("`zgui-platform-drm` is not published"),
            "{messages:#?}"
        );
        assert!(names("zgui-render/fixture.Cargo.toml"), "{messages:#?}");
    }
}
