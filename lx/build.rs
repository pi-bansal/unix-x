fn main() {
    // libgit2-sys's vendored build references CryptAcquireContextA/CryptGenRandom/
    // GetNamedSecurityInfoW (advapi32) without linking against it on MSVC.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-lib=dylib=advapi32");
    }
}
