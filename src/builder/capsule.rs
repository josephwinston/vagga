/// The "capsule" module is a module handling alpine linux distribution that
/// is used in a build shell.
///
/// Usually we only use busybox from capsule to download initial image, but we
/// may need real wget and ca-certificates for https. An other features may
/// need more things.

use std::rand::{thread_rng, Rng};
use std::old_io::fs::{File, mkdir, mkdir_recursive};
use std::old_io::fs::PathExtensions;
use std::old_io::ALL_PERMISSIONS;
use std::collections::HashSet;
use std::old_io::process::{Command, Ignored, InheritFd, ExitStatus};

use container::mount::bind_mount;
use super::context::BuildContext;
use super::commands::alpine::LATEST_VERSION;

pub use self::Feature::*;

static MIRRORS: &'static str = include_str!("../../alpine/MIRRORS.txt");

#[derive(Copy)]
pub enum Feature {
    Https,
    AlpineInstaller,
    Git,
}

#[derive(Default)]
pub struct State {
    capsule_base: bool,
    alpine_ready: bool,
    installed_packages: HashSet<String>,
}

// Also used in alpine
pub fn apk_run(args: &[&str], packages: &[String]) -> Result<(), String> {
    let mut cmd = Command::new("/vagga/bin/apk");
    cmd.stdin(Ignored).stdout(InheritFd(1)).stderr(InheritFd(2))
        .env("PATH", "/vagga/bin")
        .args(args)
        .args(packages);
    debug!("Running APK {:?}", cmd);
    return match cmd.output()
        .map_err(|e| format!("Can't run apk: {}", e))
        .map(|o| o.status)
    {
        Ok(ExitStatus(0)) => Ok(()),
        Ok(val) => Err(format!("Apk exited with status: {}", val)),
        Err(x) => Err(format!("Error running tar: {}", x)),
    }
}

fn choose_mirror() -> String {
    let repos = MIRRORS
        .split('\n')
        .map(|x| x.trim())
        .filter(|x| x.len() > 0 && !x.starts_with("#"))
        .collect::<Vec<&str>>();
    let mirror = thread_rng().choose(repos.as_slice())
        .expect("At least one mirror should work");
    debug!("Chosen mirror {}", mirror);
    return mirror.to_string();
}

pub fn ensure_features(ctx: &mut BuildContext, features: &[Feature])
    -> Result<(), String>
{
    if features.len() == 0 {
        return Ok(());
    }
    if !ctx.capsule.capsule_base {
        let cache_dir = Path::new("/vagga/cache/alpine-cache");
        if !cache_dir.exists() {
            try!(mkdir(&cache_dir, ALL_PERMISSIONS)
                 .map_err(|e| format!("Error creating cache dir: {}", e)));
        }
        let path = Path::new("/etc/apk/cache");
        try!(mkdir_recursive(&path, ALL_PERMISSIONS)
             .map_err(|e| format!("Error creating cache dir: {}", e)));
        try!(bind_mount(&cache_dir, &path));

        try!(apk_run(&[
            "--allow-untrusted",
            "--initdb",
            "add",
            "--force",
            "/vagga/bin/alpine-keys.apk",
            ], &[]));
        let mirror = ctx.settings.alpine_mirror.clone()
            .unwrap_or(choose_mirror());
        try!(File::create(&Path::new("/etc/apk/repositories"))
            .and_then(|mut f| write!(&mut f, "{}{}/main\n",
                mirror, LATEST_VERSION))
            .map_err(|e| format!("Can't write repositories file: {}", e)));
        ctx.capsule.capsule_base = true;
    }
    let mut pkg_queue = vec!();
    for value in features.iter() {
        match *value {
            AlpineInstaller => {}  // basically capsule_base
            Https => {
                pkg_queue.push("wget".to_string());
                pkg_queue.push("ca-certificates".to_string());
            }
            Git => {
                pkg_queue.push("git".to_string());
                pkg_queue.push("ca-certificates".to_string());
            }
        }
    }
    if pkg_queue.len() > 0 {
        pkg_queue = pkg_queue.into_iter()
            .filter(|x| !ctx.capsule.installed_packages.contains(x))
            .collect();
    }
    if pkg_queue.len() > 0 {
        if ctx.capsule.installed_packages.len() == 0 { // already have indexes
            try!(apk_run(&[
                "--update-cache",
                "add",
                ], &pkg_queue[0..]));
        } else {
            try!(apk_run(&[
                "add",
                ], &pkg_queue[0..]));
        }
        ctx.capsule.installed_packages.extend(pkg_queue.into_iter());
    }
    Ok(())
}

