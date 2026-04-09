use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const ROOT_PACKAGE_ID: &str = "formal_x2dweb";

fn initializer_symbols(source: &str) -> BTreeSet<String> {
    let mut symbols = BTreeSet::new();
    let mut start = 0;

    while let Some(offset) = source[start..].find("initialize_") {
        let symbol_start = start + offset;
        let mut symbol_end = symbol_start + "initialize_".len();
        let bytes = source.as_bytes();

        while symbol_end < bytes.len() {
            let byte = bytes[symbol_end];
            if byte.is_ascii_alphanumeric() || byte == b'_' {
                symbol_end += 1;
            } else {
                break;
            }
        }

        if bytes.get(symbol_end) == Some(&b'(') {
            symbols.insert(source[symbol_start..symbol_end].to_owned());
        }

        start = symbol_end;
    }

    symbols
}

fn package_ir_dirs(repo_root: &Path) -> BTreeMap<String, PathBuf> {
    let mut dirs = BTreeMap::new();
    let packages_dir = repo_root.join(".lake").join("packages");

    if let Ok(entries) = fs::read_dir(packages_dir) {
        for entry in entries {
            let entry = entry.expect("failed to read package directory entry");
            let package_name = entry.file_name();
            let package_name = package_name
                .into_string()
                .expect("package directory should be valid UTF-8");
            let ir_dir = entry.path().join(".lake").join("build").join("ir");
            if ir_dir.is_dir() {
                dirs.insert(package_name, ir_dir);
            }
        }
    }

    dirs
}

fn c_file_for_initializer(
    symbol: &str,
    root_ir_dir: &Path,
    package_ir_dirs: &BTreeMap<String, PathBuf>,
) -> Option<PathBuf> {
    if symbol == "initialize_Init"
        || symbol.starts_with("initialize_Std_")
        || symbol.starts_with("initialize_Lean_")
        || symbol.starts_with("initialize_Lake_")
    {
        return None;
    }

    let stem = symbol
        .strip_prefix("initialize_")
        .expect("initializer symbols should start with `initialize_`");

    if let Some(module_stem) = stem.strip_prefix(&format!("{ROOT_PACKAGE_ID}_")) {
        return Some(root_ir_dir.join(module_stem.replace('_', "/")).with_extension("c"));
    }

    let (package_name, module_stem) = stem.split_once('_')?;
    let ir_dir = package_ir_dirs
        .get(package_name)
        .or_else(|| package_ir_dirs.get(&package_name.to_lowercase()))?;

    Some(ir_dir.join(module_stem.replace('_', "/")).with_extension("c"))
}

fn collect_dependency_objects(
    entry_c_files: &[PathBuf],
    root_ir_dir: &Path,
    package_ir_dirs: &BTreeMap<String, PathBuf>,
) -> BTreeSet<PathBuf> {
    let mut pending = VecDeque::from(entry_c_files.to_vec());
    let mut visited = BTreeSet::new();
    let mut objects = BTreeSet::new();

    while let Some(c_file) = pending.pop_front() {
        if !visited.insert(c_file.clone()) {
            continue;
        }

        let source = fs::read_to_string(&c_file)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", c_file.display()));

        for symbol in initializer_symbols(&source) {
            let Some(dep_c_file) = c_file_for_initializer(&symbol, root_ir_dir, package_ir_dirs) else {
                continue;
            };

            if dep_c_file == c_file {
                continue;
            }

            let dep_object = dep_c_file.with_extension("c.o.export");
            assert!(
                dep_c_file.is_file(),
                "missing generated Lean C file for {symbol}: {}",
                dep_c_file.display()
            );
            assert!(
                dep_object.is_file(),
                "missing generated Lean object export for {symbol}: {}",
                dep_object.display()
            );

            objects.insert(dep_object);
            pending.push_back(dep_c_file);
        }
    }

    objects
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../FormalWeb");
    println!("cargo:rerun-if-changed=../FormalWeb.lean");
    println!("cargo:rerun-if-changed=../FormalWebRuntime.lean");
    println!("cargo:rerun-if-changed=../lakefile.lean");
    println!("cargo:rerun-if-changed=src/lean_shim.c");
    println!("cargo:rerun-if-changed=src/macos_compat.m");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR should be set"));
    let repo_root = manifest_dir
        .parent()
        .expect("ffi crate should live under the repository root")
        .to_path_buf();

    let lake_status = Command::new("lake")
        .arg("build")
        .arg("FormalWebRuntime")
        .current_dir(&repo_root)
        .status()
        .expect("failed to run `lake build FormalWebRuntime`");
    assert!(
        lake_status.success(),
        "`lake build FormalWebRuntime` exited with status {}",
        lake_status
    );

    let lean_prefix = Command::new("lean")
        .arg("--print-prefix")
        .output()
        .expect("failed to run `lean --print-prefix`");

    assert!(
        lean_prefix.status.success(),
        "`lean --print-prefix` exited with status {}",
        lean_prefix.status
    );

    let lean_prefix = String::from_utf8(lean_prefix.stdout)
        .expect("`lean --print-prefix` returned non-UTF-8 output");
    let lean_prefix = PathBuf::from(lean_prefix.trim());
    let include_dir = lean_prefix.join("include");
    let lean_lib_dir = lean_prefix.join("lib").join("lean");
    let lake_ir_dir = repo_root.join(".lake").join("build").join("ir");
    let formalweb_ir_dir = lake_ir_dir.join("FormalWeb");
    let runtime_entry = lake_ir_dir.join("FormalWebRuntime.c");
    let target = env::var("TARGET").expect("TARGET should be set");
    let dependency_objects = collect_dependency_objects(
        std::slice::from_ref(&runtime_entry),
        &lake_ir_dir,
        &package_ir_dirs(&repo_root),
    );

    println!("cargo:rustc-link-search=native={}", lean_lib_dir.display());
    println!("cargo:rustc-link-lib=dylib=Lake_shared");
    println!("cargo:rustc-link-lib=dylib=leanshared");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lean_lib_dir.display());

    let mut build = cc::Build::new();
    build
        .file("src/lean_shim.c")
        .file(lake_ir_dir.join("FormalWebRuntime.c"))
        .include(&include_dir)
        .include(&lake_ir_dir)
        .include(&formalweb_ir_dir);

    for path in dependency_objects {
        build.object(path);
    }

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let rustc = env::var("RUSTC").unwrap_or_else(|_| String::from("rustc"));
        let sysroot = Command::new(rustc)
            .args(["--print", "sysroot"])
            .output()
            .expect("failed to run `rustc --print sysroot`");
        assert!(
            sysroot.status.success(),
            "`rustc --print sysroot` exited with status {}",
            sysroot.status
        );

        let sysroot = String::from_utf8(sysroot.stdout)
            .expect("`rustc --print sysroot` returned non-UTF-8 output");
        let llvm_ar = PathBuf::from(sysroot.trim())
            .join("lib")
            .join("rustlib")
            .join(&target)
            .join("bin")
            .join("llvm-ar");
        if llvm_ar.is_file() {
            build.archiver(&llvm_ar);
        }

        build.file("src/macos_compat.m");
        build.flag("-mmacosx-version-min=11.0");
    }

    build.compile("ffi_lean_shim");

    println!("cargo:warning=building Lean shim for {target}");
}