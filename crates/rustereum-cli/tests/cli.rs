use rustereum_cli::{
    add_dependency, fetch, generate_bindings, read_manifest, scaffold_new, write_manifest,
    Dependency, Manifest,
};
use std::collections::BTreeMap;
use std::path::Path;

#[test]
fn new_scaffolds_project() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("myproj");
    scaffold_new(&root, "myproj", false).unwrap();
    assert!(root.join("Cargo.toml").exists());
    assert!(root.join("src/lib.rs").exists());
    assert!(root.join("remappings.txt").exists());
    assert!(root.join("src/bindings.rs").exists());
    assert!(root.join("lib").is_dir());
}

#[test]
fn new_refuses_non_empty_without_force() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("myproj");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("existing.txt"), "hi").unwrap();
    assert!(scaffold_new(&root, "myproj", false).is_err());
    // With force it succeeds.
    scaffold_new(&root, "myproj", true).unwrap();
    assert!(root.join("Cargo.toml").exists());
}

#[test]
fn generate_bindings_emits_traits() {
    // Point at the vendored OZ fixture in the rustereum crate.
    let sol_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../rustereum/tests/fixtures/project/lib/openzeppelin-contracts/contracts");
    let out = generate_bindings(&sol_root, "@openzeppelin/contracts/");
    assert!(out.contains("pub trait Ownable"));
    assert!(
        out.contains(r#"SOL_IMPORT: &'static str = "@openzeppelin/contracts/access/Ownable.sol""#)
    );
    assert!(out.contains(r#"SOL_NAME: &'static str = "Ownable""#));
    // Context is an abstract contract → also emitted.
    assert!(out.contains("pub trait Context"));
}

#[test]
#[ignore = "requires network access; run with --ignored"]
fn add_clones_and_generates() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // scaffold so remappings.txt / src/bindings.rs exist
    scaffold_new(root, "netproj", true).unwrap();
    add_dependency(root, "OpenZeppelin/openzeppelin-contracts", Some("v5.1.0")).unwrap();
    assert!(root.join("lib/openzeppelin-contracts").is_dir());
    let remaps = std::fs::read_to_string(root.join("remappings.txt")).unwrap();
    assert!(remaps.contains("@openzeppelin/contracts/=lib/openzeppelin-contracts/contracts/"));
    let bindings = std::fs::read_to_string(root.join("src/bindings.rs")).unwrap();
    assert!(bindings.contains("pub trait Ownable"));
}

#[test]
fn manifest_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let mut deps = BTreeMap::new();
    deps.insert(
        "openzeppelin-contracts".to_string(),
        Dependency {
            git: "https://github.com/OpenZeppelin/openzeppelin-contracts".into(),
            rev: Some("v5.1.0".into()),
            remap: "@openzeppelin/contracts/=contracts/".into(),
        },
    );
    write_manifest(tmp.path(), &Manifest { dependencies: deps }).unwrap();
    let m = read_manifest(tmp.path()).unwrap();
    assert_eq!(
        m.dependencies["openzeppelin-contracts"].git,
        "https://github.com/OpenZeppelin/openzeppelin-contracts"
    );
    assert_eq!(
        m.dependencies["openzeppelin-contracts"].rev.as_deref(),
        Some("v5.1.0")
    );
}

#[test]
fn fetch_generates_remappings_and_skips_existing_clone() {
    let tmp = tempfile::tempdir().unwrap();
    // Pre-create the lib dir so fetch skips the (network) git clone.
    std::fs::create_dir_all(tmp.path().join("lib/openzeppelin-contracts/contracts")).unwrap();
    let mut deps = BTreeMap::new();
    deps.insert(
        "openzeppelin-contracts".to_string(),
        Dependency {
            git: "https://github.com/OpenZeppelin/openzeppelin-contracts".into(),
            rev: Some("v5.1.0".into()),
            remap: "@openzeppelin/contracts/=contracts/".into(),
        },
    );
    write_manifest(tmp.path(), &Manifest { dependencies: deps }).unwrap();

    fetch(tmp.path()).unwrap();

    let remap = std::fs::read_to_string(tmp.path().join("remappings.txt")).unwrap();
    assert!(
        remap.contains("@openzeppelin/contracts/=lib/openzeppelin-contracts/contracts/"),
        "got: {remap}"
    );
}

#[test]
#[ignore = "requires network access; run with --ignored"]
fn fetch_clones_over_network() {
    let tmp = tempfile::tempdir().unwrap();
    let mut deps = BTreeMap::new();
    deps.insert(
        "openzeppelin-contracts".to_string(),
        Dependency {
            git: "https://github.com/OpenZeppelin/openzeppelin-contracts".into(),
            rev: Some("v5.1.0".into()),
            remap: "@openzeppelin/contracts/=contracts/".into(),
        },
    );
    write_manifest(tmp.path(), &Manifest { dependencies: deps }).unwrap();

    fetch(tmp.path()).unwrap();

    assert!(tmp
        .path()
        .join("lib/openzeppelin-contracts/contracts/access/Ownable.sol")
        .exists());
    let remap = std::fs::read_to_string(tmp.path().join("remappings.txt")).unwrap();
    assert!(remap.contains("@openzeppelin/contracts/=lib/openzeppelin-contracts/contracts/"));
}
