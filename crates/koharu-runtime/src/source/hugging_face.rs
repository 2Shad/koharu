use std::path::PathBuf;

use crate::{Store, download};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum RepositoryKind {
    Model,
    Dataset,
}

/// An immutable file snapshot hosted by Hugging Face.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct HuggingFaceFile<'a> {
    kind: RepositoryKind,
    repository: &'a str,
    revision: &'a str,
    filename: &'a str,
}

impl<'a> HuggingFaceFile<'a> {
    #[must_use]
    pub const fn pinned(repository: &'a str, revision: &'a str, filename: &'a str) -> Self {
        Self {
            kind: RepositoryKind::Model,
            repository,
            revision,
            filename,
        }
    }

    #[must_use]
    pub const fn pinned_dataset(repository: &'a str, revision: &'a str, filename: &'a str) -> Self {
        Self {
            kind: RepositoryKind::Dataset,
            ..Self::pinned(repository, revision, filename)
        }
    }

    #[must_use]
    pub fn exists(self) -> bool {
        self.path().is_file()
    }

    /// Downloads the pinned revision into the store over the shared HTTP client.
    ///
    /// `hf-hub` is deliberately not used: its client builder always creates an
    /// internal reqwest client with default TLS settings and issues the download
    /// probe through it, which bypasses the caller's TLS configuration and
    /// fails certificate verification on Android. The public resolve endpoint
    /// redirects to the CDN, so a plain ranged GET reproduces the same bytes.
    #[tracing::instrument(skip_all)]
    pub async fn resolve(self) -> anyhow::Result<PathBuf> {
        let url = self.url();
        Store::file(self.path(), move |stage| async move {
            download::fetch(&url, &stage).await
        })
        .await
    }

    fn url(self) -> String {
        let base = match self.kind {
            RepositoryKind::Model => "https://huggingface.co",
            RepositoryKind::Dataset => "https://huggingface.co/datasets",
        };
        format!(
            "{base}/{repository}/resolve/{revision}/{filename}",
            repository = self.repository,
            revision = self.revision,
            filename = self.filename,
        )
    }

    fn path(self) -> PathBuf {
        Store::root()
            .join("hugging-face")
            .join(match self.kind {
                RepositoryKind::Model => "models",
                RepositoryKind::Dataset => "datasets",
            })
            .join(self.repository.replace(['/', '\\'], "--"))
            .join("snapshots")
            .join(self.revision)
            .join(self.filename)
    }
}
