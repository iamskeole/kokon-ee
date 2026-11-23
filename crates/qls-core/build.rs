// Linking the ONNX Runtime that `ort` downloads (AI watermark, fast stickers) into release builds.
//
// macOS: its CoreML code calls `___isPlatformVersionAtLeast` (Objective-C `@available`), which lives in
// clang's runtime library. The release build (thin LTO) did not get it and every binary with qls-core
// failed to link (0.12.0, 2026-11-04; dev builds linked). Linking libclang_rt.osx.a fixes it.
//
// Linux: it is built with GCC 13 and needs its libstdc++ (GLIBCXX_3.4.32); Ubuntu 22.04, which the
// .deb, .rpm and Docker image are built on so they run on glibc 2.35, has GCC 11's. A GCC 13 or
// newer libstdc++.a is therefore linked in statically and users need nothing new. Where it comes
// from:
//   - the Docker image (docker/Dockerfile) fetches it from Ubuntu 24.04's main repos (the
//     ubuntu-toolchain-r PPA that used to serve it is on Launchpad and flakes — 503s, and a
//     build that misses it fails deep in the linker with a cryptic C++ symbol, 2026-11-23) and
//     names it in KOKON_LIBSTDCXX_A, which wins over everything below;
//   - otherwise any g++-13..20 on the host (a dev box with the PPA, or Ubuntu 24.04+, whose
//     default g++ is 13). If none is found the system's libstdc++ is used and a loud warning is
//     printed — on a 24.04+ host that is enough, on an older one the release link fails (loudly,
//     at the link, with the warning above it) rather than silently.
use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("macos") => clang_runtime(),
        Ok("linux") => static_libstdcxx(),
        _ => {}
    }
}

fn clang_runtime() {
    let Ok(out) = Command::new("clang").arg("--print-runtime-dir").output() else { return };
    let dir = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if Path::new(&dir).join("libclang_rt.osx.a").is_file() {
        println!("cargo:rustc-link-search=native={dir}");
        println!("cargo:rustc-link-lib=static=clang_rt.osx");
    } else {
        println!("cargo:warning=libclang_rt.osx.a not found in {dir}: a release build may fail to link ONNX Runtime");
    }
}

fn static_libstdcxx() {
    // The image's own copy: it was built to be there; a missing file is a broken build, not a fallback.
    if let Ok(explicit) = std::env::var("KOKON_LIBSTDCXX_A") {
        if Path::new(&explicit).is_file() {
            use_lib(&explicit);
        } else {
            panic!("KOKON_LIBSTDCXX_A={explicit} is set but the file is missing — the build environment is broken");
        }
        return;
    }
    for v in (13..=20).rev() {
        let Ok(out) = Command::new(format!("g++-{v}")).arg("-print-file-name=libstdc++.a").output() else { continue };
        let lib = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !out.status.success() || !Path::new(&lib).is_absolute() || !Path::new(&lib).is_file() {
            continue;
        }
        use_lib(&lib);
        return;
    }
    println!(
        "cargo:warning=no GCC 13+ libstdc++.a found (g++-13..20): using the system's libstdc++ — the ONNX Runtime link needs GCC 13's (GLIBCXX_3.4.32) and will fail on an older system (Ubuntu 22.04 and earlier); Ubuntu 24.04+ is fine"
    );
}

fn use_lib(lib: &str) {
    let dir = Path::new(&std::env::var("OUT_DIR").unwrap()).join("static-libstdcxx");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(lib, dir.join("libstdc++.a")).unwrap();
    println!("cargo:rustc-link-search=native={}", dir.display());
}
