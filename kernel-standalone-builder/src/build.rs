// Copyright (C) 2019-2021  Pierre Krieger
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

/// Configuration for building the kernel.
#[derive(Debug)]
pub struct Config<'a> {
    /// Path to the `Cargo.toml` of the standalone kernel library.
    // TODO: once the standalone kernel is on crates.io, make it possible for the kernel builder to run as a completely stand-alone program and pass a build directory instead
    pub kernel_cargo_toml: &'a Path,

    /// If true, compiles with `--release`.
    pub release: bool,

    /// Name of the target to pass as `--target`.
    pub target_name: &'a str,

    /// JSON target specifications.
    pub target_specs: Option<&'a str>,

    /// Link script to pass to the linker.
    pub link_script: Option<&'a str>,
}

/// Successful build.
#[derive(Debug)]
pub struct BuildOutput {
    /// Path to the output of the compilation.
    pub out_kernel_path: PathBuf,
}

/// Error that can happen during the build.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Could not start Cargo: {0}")]
    CargoNotFound(io::Error),

    #[error("Error while building the kernel")]
    BuildError,

    #[error("Failed to get metadata about the kernel Cargo.toml")]
    MetadataFailed,

    #[error("Invalid kernel Cargo.toml path")]
    BadKernelCargoTomlPath,

    #[error("{0}")]
    Io(#[from] io::Error),
}

/// Builds the kernel.
pub fn build(cfg: Config) -> Result<BuildOutput, Error> {
    assert_ne!(cfg.target_name, "debug");
    assert_ne!(cfg.target_name, "release");

    // Determine the path to the file that Cargo will generate.
    let (output_file, target_dir_with_target) = {
        let metadata = cargo_metadata::MetadataCommand::new()
            .manifest_path(&cfg.kernel_cargo_toml)
            .no_deps()
            .exec()
            .map_err(|_| Error::MetadataFailed)?;

        let target_dir_with_target = metadata
            .target_directory
            .join(cfg.target_name)
            .join("project");

        let output_file = target_dir_with_target
            .join("target")
            .join(cfg.target_name)
            .join(if cfg.release { "release" } else { "debug" })
            .join("kernel");

        (output_file, target_dir_with_target)
    };

    // Create and fill the directory where various source files are put.
    // If `cargo_clean_needed` is set to true, the build will later be done from scratch.
    let mut cargo_clean_needed = false;
    fs::create_dir_all(&target_dir_with_target)?;
    if let Some(target_specs) = &cfg.target_specs {
        if write_if_changed(
            (&target_dir_with_target).join(format!("{}.json", cfg.target_name)),
            target_specs.as_bytes(),
        )? {
            cargo_clean_needed = true;
        }
    }
    if let Some(link_script) = &cfg.link_script {
        if write_if_changed(
            (&target_dir_with_target).join("link.ld"),
            link_script.as_bytes(),
        )? {
            // Note: this is overly conservative. Only the linking step needs to be done again, but
            // there isn't any easy way to retrigger only the linking.
            cargo_clean_needed = true;
        }
    }
    {
        let mut cargo_toml_prototype = toml::value::Table::new();
        // TODO: should write `[profile]` in there
        cargo_toml_prototype.insert("package".into(), {
            let mut package = toml::value::Table::new();
            package.insert("name".into(), "kernel".into());
            package.insert("version".into(), "1.0.0".into());
            package.insert("edition".into(), "2018".into());
            package.into()
        });
        cargo_toml_prototype.insert("dependencies".into(), {
            let mut dependencies = toml::value::Table::new();
            dependencies.insert("redshirt-standalone-kernel".into(), {
                let mut wasm_project = toml::value::Table::new();
                wasm_project.insert(
                    "path".into(),
                    cfg.kernel_cargo_toml
                        .parent()
                        .ok_or(Error::BadKernelCargoTomlPath)?
                        .display()
                        .to_string()
                        .into(),
                );
                wasm_project.into()
            });
            dependencies.into()
        });
        cargo_toml_prototype.insert("profile".into(), {
            let mut profiles = toml::value::Table::new();
            profiles.insert("release".into(), {
                let mut profile = toml::value::Table::new();
                profile.insert("panic".into(), "abort".into());
                profile.insert("lto".into(), true.into());
                profile.insert("opt-level".into(), 3.into());
                profile.into()
            });
            profiles.insert("dev".into(), {
                let mut profile = toml::value::Table::new();
                profile.insert("panic".into(), "abort".into());
                profile.insert("opt-level".into(), 2.into());
                profile.into()
            });
            profiles.into()
        });
        cargo_toml_prototype.insert("workspace".into(), toml::value::Table::new().into());
        cargo_toml_prototype.insert("patch".into(), {
            let mut patches = toml::value::Table::new();
            patches.insert("crates-io".into(), {
                let crates = toml::value::Table::new();
                // Uncomment this in order to overwrite dependencies used during the kernel
                // compilation.
                /*crates.insert("foo".into(), {
                    let mut val = toml::value::Table::new();
                    val.insert("path".into(), "/path/to/foot".into());
                    val.into()
                });*/
                crates.into()
            });
            patches.into()
        });
        write_if_changed(
            target_dir_with_target.join("Cargo.toml"),
            toml::to_string_pretty(&cargo_toml_prototype).unwrap(),
        )?;
    }
    {
        fs::create_dir_all(&target_dir_with_target.join("src"))?;
        let src = format!(
            r#"
        #![no_std]
        #![no_main]

        // TODO: these features are necessary because of the fact that we use a macro
        #![feature(naked_functions)] // TODO: https://github.com/rust-lang/rust/issues/32408

        redshirt_standalone_kernel::__gen_boot! {{
            entry: redshirt_standalone_kernel::run,
            memory_zeroing_start: __bss_start,
            memory_zeroing_end: __bss_end,
        }}
        
        extern "C" {{
            static mut __bss_start: u8;
            static mut __bss_end: u8;
        }}
        "#
        );
        write_if_changed(target_dir_with_target.join("src").join("main.rs"), src)?;
    }

    if cargo_clean_needed {
        let status = Command::new("cargo")
            .arg("clean")
            .arg("--manifest-path")
            .arg(target_dir_with_target.join("Cargo.toml"))
            .status()
            .map_err(Error::CargoNotFound)?;
        // TODO: should we make it configurable where the stdout/stderr outputs go?
        if !status.success() {
            return Err(Error::BuildError);
        }
    }

    // Actually build the kernel.
    let build_status = Command::new("cargo")
        .arg("build")
        .args(
            cfg.target_specs
                .is_some()
                .then_some(["-Z", "build-std=core,alloc"])
                .into_iter()
                .flatten(),
        ) // TODO: nightly only; cc https://github.com/tomaka/redshirt/issues/300
        .env("RUST_TARGET_PATH", &target_dir_with_target)
        .envs(cfg.link_script.is_some().then_some((
            format!(
                "CARGO_TARGET_{}_RUSTFLAGS",
                cfg.target_name.replace("-", "_").to_uppercase()
            ),
            format!(
                "-Clink-arg=--script -Clink-arg={}",
                target_dir_with_target.join("link.ld").display()
            ),
        )))
        .arg("--target")
        .arg(cfg.target_name)
        .arg("--manifest-path")
        .arg(target_dir_with_target.join("Cargo.toml"))
        .args(if cfg.release {
            &["--release"][..]
        } else {
            &[][..]
        })
        .status()
        .map_err(Error::CargoNotFound)?;
    // TODO: should we make it configurable where the stdout/stderr outputs go?
    if !build_status.success() {
        return Err(Error::BuildError);
    }

    assert!(output_file.exists());

    Ok(BuildOutput {
        out_kernel_path: output_file,
    })
}

/// Write to the given `file` if the `content` is different.
///
/// Returns `true` if the content was indeed different and a write has been performed.
///
/// This function is used in order to not make Cargo trigger a rebuild by writing over a file
/// with the same content as it already has.
fn write_if_changed(file: impl AsRef<Path>, content: impl AsRef<[u8]>) -> Result<bool, io::Error> {
    if fs::read(file.as_ref()).ok().as_deref() != Some(content.as_ref()) {
        fs::write(file, content.as_ref())?;
        Ok(true)
    } else {
        Ok(false)
    }
}
