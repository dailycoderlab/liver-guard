//! Platform smoke test without OpenCV:
//! `cargo run -p liverguard-driver-mac --no-default-features --example probe`

#[cfg(target_os = "macos")]
fn main() {
    use liverguard_driver_mac::capture::{Capturer, Target};
    use liverguard_driver_mac::{permissions, window};

    let state = permissions::current_state();
    println!("permissions: {state:?}");
    println!("owner: {}", permissions::owner_label());
    for problem in state.problems(&permissions::owner_label()) {
        println!("  - {problem}");
    }

    let display = window::main_display_bounds();
    println!("main display: {display:?}");

    let windows = window::list_windows(true, false);
    println!("{} windows", windows.len());
    for w in windows.iter().take(5) {
        println!("  {w}");
    }

    let mut capturer = Capturer::new();
    let started = std::time::Instant::now();
    match capturer.grab(Target::MainDisplay) {
        Ok(image) => println!(
            "display capture: {}x{} (scale {:.2}) in {:?}, first pixel {:?}",
            image.width,
            image.height,
            image.width as f64 / display.w,
            started.elapsed(),
            &image.bgra[..4]
        ),
        Err(error) => println!("display capture failed: {error}"),
    }

    if let Some(target) = windows.first() {
        let started = std::time::Instant::now();
        match capturer.grab(Target::Window(target.number)) {
            Ok(image) => println!(
                "window capture of {target}: {}x{} (expected ~{:.0}x{:.0} points) in {:?}",
                image.width,
                image.height,
                target.bounds.w,
                target.bounds.h,
                started.elapsed()
            ),
            Err(error) => println!("window capture failed: {error}"),
        }
        let started = std::time::Instant::now();
        let again = capturer.grab(Target::Window(target.number));
        println!(
            "second window capture (cached handles): ok={} in {:?}",
            again.is_ok(),
            started.elapsed()
        );
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {}
