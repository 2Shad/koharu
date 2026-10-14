use std::path::PathBuf;

use anyhow::{Result, bail};
use strum::EnumProperty;

use crate::{
    Hardware, Store, download,
    runtime::{Package, packages::Cuda, sealed},
    source::extract,
};

/// Built from JarodMica/audio.cpp `drama-3-native` @ 9277495.
const RELEASE: &str = "drama3-9277495";
const REPOSITORY: &str = "karrot0/audio.cpp";
const FILES: &[&str] = &[
    "**/audiocpp_server",
    "**/audiocpp_server.exe",
    "**/LICENSE*",
];
/// CUDA libraries the server links; must match the release's toolkit major.
const LIBRARIES: [Cuda; 3] = [Cuda::Runtime13, Cuda::Blas13, Cuda::Rtc13];

/// Store executable and the library directories its process searches.
#[derive(Clone, Debug)]
pub struct Executable {
    pub path: PathBuf,
    pub library_paths: Vec<PathBuf>,
}

/// Installs `audiocpp_server` and the CUDA libraries it links.
pub async fn speech_server() -> Result<Executable> {
    let package = AudioCpp::discover(&Hardware::discover())?;
    let mut library_paths = Vec::new();
    for library in LIBRARIES {
        for directory in library.library_directories().await? {
            if !library_paths.contains(&directory) {
                library_paths.push(directory);
            }
        }
    }
    let path = package.install().await?.join(package.executable());
    Ok(Executable {
        path,
        library_paths,
    })
}

/// `audiocpp_server`; runs out of process, so consumers get an executable,
/// not loaded libraries.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, strum::Display, strum::EnumProperty)]
enum AudioCpp {
    #[strum(
        serialize = "windows-cuda",
        props(
            asset = "x86_64-pc-windows-msvc-cuda.tar.gz",
            executable = "audiocpp_server.exe"
        )
    )]
    WindowsCuda,
    #[strum(
        serialize = "linux-cuda",
        props(
            asset = "x86_64-unknown-linux-gnu-cuda.tar.gz",
            executable = "audiocpp_server"
        )
    )]
    LinuxCuda,
}

impl AudioCpp {
    fn discover(hardware: &Hardware) -> Result<Self> {
        if !hardware.supports_cuda() {
            bail!("speech requires an NVIDIA GPU with CUDA");
        }
        if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            Ok(Self::WindowsCuda)
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            Ok(Self::LinuxCuda)
        } else {
            bail!("speech is not available on this platform")
        }
    }

    fn executable(self) -> &'static str {
        self.get_str("executable")
            .expect("audio.cpp package has an executable")
    }
}

impl sealed::Sealed for AudioCpp {}

impl Package for AudioCpp {
    async fn install(self) -> Result<PathBuf> {
        let target = Store::root()
            .join("audiocpp")
            .join(RELEASE)
            .join(self.to_string());
        Store::directory(
            target,
            move |path| path.join(self.executable()).is_file(),
            move |stage| async move {
                let asset = self
                    .get_str("asset")
                    .expect("audio.cpp package has an asset");
                let url =
                    format!("https://github.com/{REPOSITORY}/releases/download/{RELEASE}/{asset}");
                let archive = tempfile::Builder::new().suffix(".tar.gz").tempfile()?;
                download::fetch(&url, archive.path()).await?;
                extract(archive.path(), &stage, FILES)
            },
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Release archives come from `tar -C stage .`: root entries with a `./` prefix.
    #[test]
    fn installs_the_release_archive_layout() {
        let directory = tempfile::tempdir().unwrap();
        let archive = directory.path().join("x86_64-pc-windows-msvc-cuda.tar.gz");
        {
            let encoder = flate2::write::GzEncoder::new(
                std::fs::File::create(&archive).unwrap(),
                flate2::Compression::fast(),
            );
            let mut builder = tar::Builder::new(encoder);
            for name in [
                "./audiocpp_server.exe",
                "./LICENSE",
                "./LICENSE-ggml",
                "./notes.txt",
            ] {
                let mut header = tar::Header::new_gnu();
                header.set_size(1);
                header.set_mode(0o755);
                header.set_cksum();
                builder.append_data(&mut header, name, &b"x"[..]).unwrap();
            }
            builder.into_inner().unwrap().finish().unwrap();
        }
        let stage = directory.path().join("stage");
        std::fs::create_dir(&stage).unwrap();
        extract(&archive, &stage, FILES).unwrap();
        assert!(stage.join(AudioCpp::WindowsCuda.executable()).is_file());
        assert!(stage.join("LICENSE-ggml").is_file());
        assert!(!stage.join("notes.txt").exists());
    }
}
