// @group Configuration : Build script — forward GH_OAUTH_CLIENT_ID to the compiler
//
// Set GH_OAUTH_CLIENT_ID in your environment (or in a .env.build file loaded by your
// build wrapper) before running `cargo build`. The value is baked into the binary
// at compile time via option_env!("GH_OAUTH_CLIENT_ID") in src/api/routes/ai.rs.
//
// Example:
//   GH_OAUTH_CLIENT_ID=Ov23liXXXXXXXXXXXXXX cargo build --release

fn main() {
    // Re-run this script only when GH_OAUTH_CLIENT_ID changes (not on every build)
    println!("cargo:rerun-if-env-changed=GH_OAUTH_CLIENT_ID");
    println!("cargo:rerun-if-changed=assets/rundock-icon.ico");

    if cfg!(windows) {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/rundock-icon.ico");
        if let Err(error) = resource.compile() {
            let profile = std::env::var("PROFILE").unwrap_or_default();
            let in_ci = std::env::var("CI")
                .map(|value| value.eq_ignore_ascii_case("true") || value == "1")
                .unwrap_or(false);
            if profile == "release" || in_ci {
                panic!("failed to embed the RunDock Windows icon: {error}");
            }
            println!(
                "cargo:warning=Windows resource compiler unavailable; continuing the local {profile} build without an embedded icon: {error}"
            );
        }
    }
}
